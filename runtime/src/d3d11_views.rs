//! LibOVR permits default views on non-typeless color chains. OpenXR can return
//! typeless storage, including when we negotiate a different HDR format.
//! Supply typed RTV/SRV descriptions only for textures tagged by our bridge.
//! No texture copy or additional graphics/runtime layer is required.
use core::{ffi::c_void, mem::size_of};
use std::collections::BTreeMap;
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
use windows::core::{GUID, HRESULT, Interface};
use windows::Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*};

const VIEW_DATA: GUID = GUID::from_u128(0x3f2585b6_1af2_4daa_a732_68f49a9785a1);
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ViewData {
    requested: DXGI_FORMAT,
    srv: D3D11_SHADER_RESOURCE_VIEW_DESC,
    rtv: D3D11_RENDER_TARGET_VIEW_DESC,
}
type SrvFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *const D3D11_SHADER_RESOURCE_VIEW_DESC, *mut *mut c_void) -> HRESULT;
type RtvFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *const D3D11_RENDER_TARGET_VIEW_DESC, *mut *mut c_void) -> HRESULT;
#[derive(Clone, Copy)]
struct Originals { srv: SrvFn, rtv: RtvFn }
static ORIGINALS: Mutex<BTreeMap<usize, Originals>> = Mutex::new(BTreeMap::new());
static LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
}

// The table and callbacks remain valid for the process lifetime. Pin both
// modules so a game shutdown/reinitialization cannot leave dangling callbacks.
unsafe fn pin(address: *const c_void) -> Result<(), String> {
    let mut module = core::ptr::null_mut();
    if unsafe { GetModuleHandleExW(0x1 | 0x4, address.cast(), &mut module) } == 0 {
        return Err(format!("pin D3D11 view callback: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

unsafe fn install(device: &ID3D11Device) -> Result<(), String> {
    let table = unsafe { *(device.as_raw() as *const *mut ID3D11Device_Vtbl) };
    let mut registry = ORIGINALS.lock().map_err(|_| "D3D11 hook registry poisoned")?;
    if registry.contains_key(&(table as usize)) { return Ok(()); }
    unsafe { pin(hook_srv as *const c_void)?; pin((*table).CreateShaderResourceView as *const c_void)?; }
    let originals = unsafe { Originals { srv: (*table).CreateShaderResourceView, rtv: (*table).CreateRenderTargetView } };
    // Only touch the two view entries; preserve all other device methods.
    let start = unsafe { core::ptr::addr_of_mut!((*table).CreateShaderResourceView) };
    let end = unsafe { core::ptr::addr_of_mut!((*table).CreateRenderTargetView).add(1) };
    let bytes = end as usize - start as usize;
    let mut old = 0;
    if unsafe { VirtualProtect(start.cast(), bytes, 0x40, &mut old) } == 0 {
        return Err(format!("protect D3D11 view table: {}", std::io::Error::last_os_error()));
    }
    registry.insert(table as usize, originals);
    // Aligned pointer-sized stores are atomic on the supported Windows x64 ABI.
    // Callbacks acquire this mutex before reading their original functions.
    unsafe {
        (&*start.cast::<AtomicUsize>()).store(hook_srv as *const () as usize, Ordering::SeqCst);
        (&*core::ptr::addr_of_mut!((*table).CreateRenderTargetView).cast::<AtomicUsize>())
            .store(hook_rtv as *const () as usize, Ordering::SeqCst);
    }
    let mut ignored = 0;
    if unsafe { VirtualProtect(start.cast(), bytes, old, &mut ignored) } == 0 {
        crate::capi::log_call("D3D11 view table protection restore failed");
    }
    crate::capi::log_call("D3D11 typed swapchain view compatibility installed");
    Ok(())
}

unsafe fn original(device: *mut c_void) -> Originals {
    let table = unsafe { *(device as *const usize) };
    ORIGINALS.lock().unwrap_or_else(|e| e.into_inner())[&table]
}

unsafe fn data(resource: *mut c_void) -> Option<ViewData> {
    let resource = unsafe { ID3D11Resource::from_raw_borrowed(&resource) }?;
    let mut data = ViewData::default();
    let mut size = size_of::<ViewData>() as u32;
    unsafe { resource.GetPrivateData(&VIEW_DATA, &mut size, Some((&mut data as *mut ViewData).cast())) }.ok()?;
    (size == size_of::<ViewData>() as u32).then_some(data)
}

fn log_view(kind: &str, input: Option<DXGI_FORMAT>, output: DXGI_FORMAT, hr: HRESULT) {
    if hr.is_err() || LOG_COUNT.fetch_add(1, Ordering::Relaxed) < 24 {
        crate::capi::log_call(&format!("D3D11 swapchain {kind} requested={:?} selected={} hr={:#x}", input.map(|f| f.0), output.0, hr.0));
    }
}

unsafe extern "system" fn hook_srv(device: *mut c_void, resource: *mut c_void, desc: *const D3D11_SHADER_RESOURCE_VIEW_DESC, out: *mut *mut c_void) -> HRESULT {
    let original = unsafe { original(device) }.srv;
    if let Some(data) = unsafe { data(resource) } {
        let mut adjusted = unsafe { desc.as_ref() }.copied().unwrap_or(data.srv);
        if desc.is_null() || adjusted.Format == DXGI_FORMAT_UNKNOWN || adjusted.Format == data.requested {
            adjusted.Format = data.srv.Format;
        }
        let result = unsafe { original(device, resource, &adjusted, out) };
        log_view("SRV", unsafe { desc.as_ref() }.map(|d| d.Format), adjusted.Format, result);
        return result;
    }
    unsafe { original(device, resource, desc, out) }
}

unsafe extern "system" fn hook_rtv(device: *mut c_void, resource: *mut c_void, desc: *const D3D11_RENDER_TARGET_VIEW_DESC, out: *mut *mut c_void) -> HRESULT {
    let original = unsafe { original(device) }.rtv;
    if let Some(data) = unsafe { data(resource) } {
        let mut adjusted = unsafe { desc.as_ref() }.copied().unwrap_or(data.rtv);
        if desc.is_null() || adjusted.Format == DXGI_FORMAT_UNKNOWN || adjusted.Format == data.requested {
            adjusted.Format = data.rtv.Format;
        }
        let result = unsafe { original(device, resource, &adjusted, out) };
        log_view("RTV", unsafe { desc.as_ref() }.map(|d| d.Format), adjusted.Format, result);
        return result;
    }
    unsafe { original(device, resource, desc, out) }
}

/// Tag real runtime textures before exposing them to the game. Dimensions and
/// subresource defaults come from the actual texture; explicit game subresource
/// selections are preserved by the callbacks.
pub unsafe fn prepare(images: &[usize], requested: u32, selected: u32) -> Result<(), String> {
    for &image in images {
        let raw = image as *mut c_void;
        let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or("null D3D11 image")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let mut data = ViewData { requested: DXGI_FORMAT(requested as i32), ..Default::default() };
        data.srv.Format = DXGI_FORMAT(selected as i32);
        data.rtv.Format = data.srv.Format;
        match (desc.ArraySize > 1, desc.SampleDesc.Count > 1) {
            (false, false) => {
                data.srv.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2D;
                data.srv.Anonymous.Texture2D = D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: desc.MipLevels };
                data.rtv.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
            }
            (true, false) => {
                data.srv.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2DARRAY;
                data.srv.Anonymous.Texture2DArray = D3D11_TEX2D_ARRAY_SRV { MostDetailedMip: 0, MipLevels: desc.MipLevels, FirstArraySlice: 0, ArraySize: desc.ArraySize };
                data.rtv.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
                data.rtv.Anonymous.Texture2DArray = D3D11_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 0, ArraySize: desc.ArraySize };
            }
            (false, true) => {
                data.srv.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2DMS;
                data.rtv.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DMS;
            }
            (true, true) => {
                data.srv.ViewDimension = D3D_SRV_DIMENSION_TEXTURE2DMSARRAY;
                data.srv.Anonymous.Texture2DMSArray = D3D11_TEX2DMS_ARRAY_SRV { FirstArraySlice: 0, ArraySize: desc.ArraySize };
                data.rtv.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DMSARRAY;
                data.rtv.Anonymous.Texture2DMSArray = D3D11_TEX2DMS_ARRAY_RTV { FirstArraySlice: 0, ArraySize: desc.ArraySize };
            }
        }
        unsafe { texture.SetPrivateData(&VIEW_DATA, size_of::<ViewData>() as u32, Some((&data as *const ViewData).cast())) }.map_err(|e| e.to_string())?;
        crate::capi::log_call(&format!("D3D11 swapchain texture storage={} requested={requested} selected={selected} {}x{} array={} mips={} samples={}", desc.Format.0, desc.Width, desc.Height, desc.ArraySize, desc.MipLevels, desc.SampleDesc.Count));
        let device = unsafe { texture.GetDevice() }.map_err(|e| e.to_string())?;
        unsafe { install(&device)? };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{Foundation::HMODULE, Graphics::Dxgi::IDXGIAdapter};

    #[test]
    #[ignore = "requires a Windows D3D11 device; also runnable under Wine without a headset"]
    fn real_d3d11_swapchain_views() {
        unsafe {
            let mut device = None;
            D3D11CreateDevice(None::<&IDXGIAdapter>, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0), None, D3D11_SDK_VERSION, Some(&mut device), None, None).unwrap();
            let device = device.unwrap();
            let texture = |format, array, samples| {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: 32, Height: 32, MipLevels: 1, ArraySize: array, Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: samples, Quality: 0 },
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..Default::default()
                };
                let mut out = None;
                device.CreateTexture2D(&desc, None, Some(&mut out)).unwrap();
                out.unwrap()
            };
            let hdr = texture(DXGI_FORMAT_R16G16B16A16_TYPELESS, 1, 1);
            // This is the failure mode: native D3D11 cannot infer a typed view.
            // Allocate actual views: Wine's D3D11 implementation does not
            // support the native validation-only (null output) form reliably.
            let mut view = None;
            let baseline = device.CreateRenderTargetView(&hdr, None, Some(&mut view));
            println!("untagged typeless default RTV: {baseline:?}");
            assert!(baseline.is_err());
            prepare(&[hdr.as_raw() as usize], 26, 10).unwrap();
            for explicit in [false, true] {
                let rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC {
                    Format: DXGI_FORMAT_R11G11B10_FLOAT, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                };
                let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R11G11B10_FLOAT, ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
                };
                let mut rtv = None;
                let mut srv = None;
                device.CreateRenderTargetView(&hdr, explicit.then_some(&rtv_desc as *const _), Some(&mut rtv)).unwrap();
                device.CreateShaderResourceView(&hdr, explicit.then_some(&srv_desc as *const _), Some(&mut srv)).unwrap();
                let mut r = D3D11_RENDER_TARGET_VIEW_DESC::default();
                let mut s = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
                rtv.unwrap().GetDesc(&mut r); srv.unwrap().GetDesc(&mut s);
                assert_eq!(r.Format, DXGI_FORMAT_R16G16B16A16_FLOAT);
                assert_eq!(s.Format, DXGI_FORMAT_R16G16B16A16_FLOAT);
            }
            // Verify the corrected RTV can write real nonblack pixels, not just
            // return a successful HRESULT.
            let context = device.GetImmediateContext().unwrap();
            let mut rtv = None;
            device.CreateRenderTargetView(&hdr, None, Some(&mut rtv)).unwrap();
            context.ClearRenderTargetView(rtv.as_ref().unwrap(), &[0.25, 0.5, 1.0, 1.0]);
            let mut readback_desc = D3D11_TEXTURE2D_DESC::default();
            hdr.GetDesc(&mut readback_desc);
            readback_desc.Usage = D3D11_USAGE_STAGING;
            readback_desc.BindFlags = 0;
            readback_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            let mut readback = None;
            device.CreateTexture2D(&readback_desc, None, Some(&mut readback)).unwrap();
            let readback = readback.unwrap();
            context.CopyResource(&readback, &hdr);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            let pixel = core::ptr::read_unaligned(mapped.pData.cast::<[u16; 4]>());
            context.Unmap(&readback, 0);
            assert_eq!(pixel, [0x3400, 0x3800, 0x3c00, 0x3c00]);
            println!("HDR render/readback pixel: {pixel:x?}");
            // The shared device table must not alter ordinary game resources.
            let plain = texture(DXGI_FORMAT_R8G8B8A8_UNORM, 1, 1);
            let mut rtv = None;
            let mut srv = None;
            device.CreateRenderTargetView(&plain, None, Some(&mut rtv)).unwrap();
            device.CreateShaderResourceView(&plain, None, Some(&mut srv)).unwrap();
            let untagged = texture(DXGI_FORMAT_R16G16B16A16_TYPELESS, 1, 1);
            let mut rejected = None;
            assert!(device.CreateRenderTargetView(&untagged, None, Some(&mut rejected)).is_err());

            for (array, samples) in [(2, 1), (1, 4), (2, 4)] {
                let tex = texture(DXGI_FORMAT_R16G16B16A16_TYPELESS, array, samples);
                prepare(&[tex.as_raw() as usize], 26, 10).unwrap();
                let mut rtv = None;
                let mut srv = None;
                device.CreateRenderTargetView(&tex, None, Some(&mut rtv)).unwrap();
                device.CreateShaderResourceView(&tex, None, Some(&mut srv)).unwrap();
                if samples == 1 {
                    let desc = D3D11_RENDER_TARGET_VIEW_DESC {
                        Format: DXGI_FORMAT_R11G11B10_FLOAT, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DARRAY,
                        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2DArray: D3D11_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 1, ArraySize: 1 } },
                    };
                    let mut view = None;
                    device.CreateRenderTargetView(&tex, Some(&desc), Some(&mut view)).unwrap();
                    let mut actual = D3D11_RENDER_TARGET_VIEW_DESC::default();
                    view.unwrap().GetDesc(&mut actual);
                    assert_eq!(actual.Anonymous.Texture2DArray.FirstArraySlice, 1);
                    assert_eq!(actual.Anonymous.Texture2DArray.ArraySize, 1);
                }
            }
            println!("D3D11 typed views: default/explicit HDR, untagged isolation, array/MSAA, subresources passed");
        }
    }
}
