//! LibOVR permits default views on non-typeless color chains. OpenXR can return
//! typeless storage, including when we negotiate a different HDR format.
//! Supply typed RTV/SRV descriptions only for textures tagged by our bridge.
//! No texture copy or additional graphics/runtime layer is required.
use core::{ffi::c_void, mem::size_of};
use std::collections::BTreeMap;
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
use windows::core::{GUID, HRESULT, Interface};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

const VIEW_DATA: GUID = GUID::from_u128(0xbaa14086_6924_4366_a0e3_30eb2d1bf2e6);
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ViewData {
    requested: DXGI_FORMAT,
    srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    rtv: D3D12_RENDER_TARGET_VIEW_DESC,
}
type SrvFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *const D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_CPU_DESCRIPTOR_HANDLE);
type RtvFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *const D3D12_RENDER_TARGET_VIEW_DESC, D3D12_CPU_DESCRIPTOR_HANDLE);
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
        return Err(format!("pin D3D12 view callback: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

unsafe fn install(device: &ID3D12Device) -> Result<(), String> {
    let table = unsafe { *(device.as_raw() as *const *mut ID3D12Device_Vtbl) };
    let mut registry = ORIGINALS.lock().map_err(|_| "D3D12 hook registry poisoned")?;
    if registry.contains_key(&(table as usize)) { return Ok(()); }
    unsafe { pin(hook_srv as *const c_void)?; pin((*table).CreateShaderResourceView as *const c_void)?; }
    let originals = unsafe { Originals { srv: (*table).CreateShaderResourceView, rtv: (*table).CreateRenderTargetView } };
    // Only touch the two view entries; preserve all other device methods.
    let start = unsafe { core::ptr::addr_of_mut!((*table).CreateShaderResourceView) };
    let end = unsafe { core::ptr::addr_of_mut!((*table).CreateRenderTargetView).add(1) };
    let bytes = end as usize - start as usize;
    let mut old = 0;
    if unsafe { VirtualProtect(start.cast(), bytes, 0x40, &mut old) } == 0 {
        return Err(format!("protect D3D12 view table: {}", std::io::Error::last_os_error()));
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
        crate::capi::log_call("D3D12 view table protection restore failed");
    }
    crate::capi::log_call("D3D12 typed swapchain view compatibility installed");
    Ok(())
}

unsafe fn original(device: *mut c_void) -> Originals {
    let table = unsafe { *(device as *const usize) };
    ORIGINALS.lock().unwrap_or_else(|e| e.into_inner())[&table]
}

unsafe fn data(resource: *mut c_void) -> Option<ViewData> {
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }?;
    let mut data = ViewData::default();
    let mut size = size_of::<ViewData>() as u32;
    unsafe { resource.GetPrivateData(&VIEW_DATA, &mut size, Some((&mut data as *mut ViewData).cast())) }.ok()?;
    (size == size_of::<ViewData>() as u32).then_some(data)
}

pub unsafe fn device_status(device: *mut c_void) -> i32 {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.unwrap();
    unsafe { device.GetDeviceRemovedReason() }.err().map(|e| e.code().0).unwrap_or(0)
}

unsafe fn log_view(device: *mut c_void, kind: &str, input: Option<DXGI_FORMAT>, output: DXGI_FORMAT) {
    let hr = HRESULT(unsafe { device_status(device) });
    if hr.is_err() || LOG_COUNT.fetch_add(1, Ordering::Relaxed) < 24 {
        crate::capi::log_call(&format!("D3D12 swapchain {kind} requested={:?} selected={} hr={:#x}", input.map(|f| f.0), output.0, hr.0));
    }
}

unsafe extern "system" fn hook_srv(device: *mut c_void, resource: *mut c_void, desc: *const D3D12_SHADER_RESOURCE_VIEW_DESC, out: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let original = unsafe { original(device) }.srv;
    if let Some(data) = unsafe { data(resource) } {
        let mut adjusted = unsafe { desc.as_ref() }.copied().unwrap_or(data.srv);
        if desc.is_null() || adjusted.Format == DXGI_FORMAT_UNKNOWN || adjusted.Format == data.requested {
            adjusted.Format = data.srv.Format;
        }
        unsafe { original(device, resource, &adjusted, out) };
        unsafe { log_view(device, "SRV", desc.as_ref().map(|d| d.Format), adjusted.Format); }
        return;
    }
    unsafe { original(device, resource, desc, out) }
}

unsafe extern "system" fn hook_rtv(device: *mut c_void, resource: *mut c_void, desc: *const D3D12_RENDER_TARGET_VIEW_DESC, out: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let original = unsafe { original(device) }.rtv;
    if let Some(data) = unsafe { data(resource) } {
        let mut adjusted = unsafe { desc.as_ref() }.copied().unwrap_or(data.rtv);
        if desc.is_null() || adjusted.Format == DXGI_FORMAT_UNKNOWN || adjusted.Format == data.requested {
            adjusted.Format = data.rtv.Format;
        }
        unsafe { original(device, resource, &adjusted, out) };
        unsafe { log_view(device, "RTV", desc.as_ref().map(|d| d.Format), adjusted.Format); }
        return;
    }
    unsafe { original(device, resource, desc, out) }
}

/// Tag real runtime textures before exposing them to the game. Dimensions and
/// subresource defaults come from the actual texture; explicit game subresource
/// selections are preserved by the callbacks.
pub unsafe fn prepare(images: &[usize], requested: u32, selected: u32) -> Result<(), String> {
    for &image in images {
        let raw = image as *mut c_void;
        let texture = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or("null D3D12 image")?;
        let desc = unsafe { texture.GetDesc() };
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D {
            return Err("OpenXR returned a non-2D D3D12 image".into());
        }
        let mut data = ViewData { requested: DXGI_FORMAT(requested as i32), ..Default::default() };
        data.srv.Format = DXGI_FORMAT(selected as i32);
        data.rtv.Format = data.srv.Format;
        data.srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        match (u32::from(desc.DepthOrArraySize) > 1, desc.SampleDesc.Count > 1) {
            (false, false) => {
                data.srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
                data.srv.Anonymous.Texture2D = D3D12_TEX2D_SRV { MostDetailedMip: 0, MipLevels: u32::from(desc.MipLevels), ..Default::default() };
                data.rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
            }
            (true, false) => {
                data.srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2DARRAY;
                data.srv.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_SRV { MostDetailedMip: 0, MipLevels: u32::from(desc.MipLevels), FirstArraySlice: 0, ArraySize: u32::from(desc.DepthOrArraySize), ..Default::default() };
                data.rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2DARRAY;
                data.rtv.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 0, ArraySize: u32::from(desc.DepthOrArraySize), ..Default::default() };
            }
            (false, true) => {
                data.srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2DMS;
                data.rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2DMS;
            }
            (true, true) => {
                data.srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY;
                data.srv.Anonymous.Texture2DMSArray = D3D12_TEX2DMS_ARRAY_SRV { FirstArraySlice: 0, ArraySize: u32::from(desc.DepthOrArraySize), ..Default::default() };
                data.rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2DMSARRAY;
                data.rtv.Anonymous.Texture2DMSArray = D3D12_TEX2DMS_ARRAY_RTV { FirstArraySlice: 0, ArraySize: u32::from(desc.DepthOrArraySize), ..Default::default() };
            }
        }
        unsafe { texture.SetPrivateData(&VIEW_DATA, size_of::<ViewData>() as u32, Some((&data as *const ViewData).cast())) }.map_err(|e| e.to_string())?;
        crate::capi::log_call(&format!("D3D12 swapchain texture storage={} requested={requested} selected={selected} {}x{} array={} mips={} samples={}", desc.Format.0, desc.Width, desc.Height, u32::from(desc.DepthOrArraySize), u32::from(desc.MipLevels), desc.SampleDesc.Count));
        let mut device: Option<ID3D12Device> = None;
        unsafe { texture.GetDevice(&mut device) }.map_err(|e| e.to_string())?;
        let device = device.ok_or("D3D12 image has no device")?;
        unsafe { install(&device)? };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::ManuallyDrop;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use crate::d3d12_states::{ColorStates, transition, finish_queue};

    #[test]
    #[ignore = "requires a Windows D3D12 device; can run under Wine without a headset"]
    fn real_d3d12_color_depth_and_handoff() {
        unsafe {
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(None::<&windows::core::IUnknown>, D3D_FEATURE_LEVEL_11_0, &mut device).unwrap();
            let device = device.unwrap();
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() };
            let color_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: 32, Height: 32, DepthOrArraySize: 1, MipLevels: 1,
                Format: DXGI_FORMAT_R8G8B8A8_TYPELESS,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, ..Default::default()
            };
            let mut color: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &color_desc, D3D12_RESOURCE_STATE_RENDER_TARGET, None, &mut color).unwrap();
            let color = color.unwrap();
            prepare(&[color.as_raw() as usize], 29, 29).unwrap();
            // Repeat registration as happens when a game recreates its chain.
            prepare(&[color.as_raw() as usize], 29, 29).unwrap();
            let rtv_heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 1, ..Default::default()
            }).unwrap();
            let srv_heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 1, ..Default::default()
            }).unwrap();
            let rtv = rtv_heap.GetCPUDescriptorHandleForHeapStart();
            device.CreateRenderTargetView(&color, None, rtv);
            device.CreateShaderResourceView(&color, None, srv_heap.GetCPUDescriptorHandleForHeapStart());
            device.GetDeviceRemovedReason().unwrap();

            // Match LE2: typeless D24S8 storage, a typed optimized clear and DSV,
            // and the depth bind flag translated from Oculus's bit layout.
            let depth_desc = D3D12_RESOURCE_DESC {
                Format: DXGI_FORMAT_R24G8_TYPELESS,
                Flags: D3D12_RESOURCE_FLAGS(crate::capi::d3d12_resource_flags(4) as i32),
                ..color_desc
            };
            let clear = D3D12_CLEAR_VALUE { Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } } };
            let mut depth: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &depth_desc, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, Some(&clear), &mut depth).unwrap();
            let depth = depth.unwrap();
            let dsv_heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV, NumDescriptors: 1, ..Default::default()
            }).unwrap();
            let dsv = dsv_heap.GetCPUDescriptorHandleForHeapStart();
            device.CreateDepthStencilView(&depth, Some(&D3D12_DEPTH_STENCIL_VIEW_DESC {
                Format: DXGI_FORMAT_D24_UNORM_S8_UINT, ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D, ..Default::default()
            }), dsv);

            let queue: ID3D12CommandQueue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()).unwrap();
            let states = ColorStates::new(&queue, &[color.as_raw() as usize]).unwrap();
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).unwrap();
            let commands: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None::<&ID3D12PipelineState>).unwrap();
            transition(&commands, &color, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET);
            transition(&commands, &depth, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_DEPTH_WRITE);
            commands.ClearRenderTargetView(rtv, &[0.0, 1.0, 0.0, 1.0], None);
            commands.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL, 1.0, 0, None);
            transition(&commands, &color, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
            transition(&commands, &depth, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
            commands.Close().unwrap();
            for _ in 0..3 {
                states.acquire(0);
                queue.ExecuteCommandLists(&[Some(commands.cast().unwrap())]);
                states.release(0);
                states.finish().unwrap();
            }

            let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
            let mut bytes = 0;
            device.GetCopyableFootprints(&color_desc, 0, 1, 0, Some(&mut footprint), None, None, Some(&mut bytes));
            let mut readback: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(&D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..heap }, D3D12_HEAP_FLAG_NONE,
                &D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: bytes, Height: 1, DepthOrArraySize: 1, MipLevels: 1,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR, ..Default::default() },
                D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback).unwrap();
            let readback = readback.unwrap();
            let copy: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None::<&ID3D12PipelineState>).unwrap();
            transition(&copy, &color, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
            let mut src = D3D12_TEXTURE_COPY_LOCATION { pResource: ManuallyDrop::new(Some(color.clone())), Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 } };
            let mut dst = D3D12_TEXTURE_COPY_LOCATION { pResource: ManuallyDrop::new(Some(readback.clone())), Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint } };
            copy.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
            ManuallyDrop::drop(&mut src.pResource); ManuallyDrop::drop(&mut dst.pResource);
            transition(&copy, &color, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET);
            copy.Close().unwrap();
            queue.ExecuteCommandLists(&[Some(copy.cast().unwrap())]);
            finish_queue(&queue).unwrap();
            let mut mapped = core::ptr::null_mut();
            readback.Map(0, None, Some(&mut mapped)).unwrap();
            let pixel = core::ptr::read_unaligned(mapped.cast::<[u8; 4]>());
            readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            assert_eq!(pixel, [0, 255, 0, 255]);
            device.GetDeviceRemovedReason().unwrap();
            println!("D3D12: typed default RTV/SRV, D24S8 create/clear, three frame handoffs and pixel readback {pixel:?} passed");
        }
    }
}
