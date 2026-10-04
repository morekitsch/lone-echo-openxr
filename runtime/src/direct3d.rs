//! Shared OpenXR frame/input bridge for the games' D3D11 and D3D12 renderers.
use openxr::{self as xr, Graphics};

pub enum Direct3D {}
pub struct Binding {
    pub device: *mut core::ffi::c_void,
    pub queue: *mut core::ffi::c_void,
}

impl Graphics for Direct3D {
    type Requirements = xr::d3d::Requirements;
    type SessionCreateInfo = Binding;
    type Format = u32;
    type SwapchainImage = *mut core::ffi::c_void;
    fn raise_format(x: i64) -> u32 { x as u32 }
    fn lower_format(x: u32) -> i64 { x.into() }
    fn requirements(i: &xr::Instance, s: xr::SystemId) -> xr::Result<Self::Requirements> {
        if i.exts().khr_d3d11_enable.is_some() {
            xr::D3D11::requirements(i, s)
        } else { xr::D3D12::requirements(i, s) }
    }
    unsafe fn create_session(i: &xr::Instance, s: xr::SystemId, b: &Binding) -> xr::Result<xr::sys::Session> {
        unsafe {
            if b.queue.is_null() {
                xr::D3D11::create_session(i, s, &xr::d3d::SessionCreateInfoD3D11 { device: b.device.cast() })
            } else {
                xr::D3D12::create_session(i, s, &xr::d3d::SessionCreateInfoD3D12 { device: b.device.cast(), queue: b.queue.cast() })
            }
        }
    }
    fn enumerate_swapchain_images(s: &xr::Swapchain<Self>) -> xr::Result<Vec<Self::SwapchainImage>> {
        // Both D3D image structures have the same C layout; only their type differs.
        let mut count = 0;
        let enumerate = s.instance().fp().enumerate_swapchain_images;
        let result = unsafe { enumerate(s.as_raw(), 0, &mut count, core::ptr::null_mut()) };
        if result.into_raw() < 0 { return Err(result); }
        let ty = if s.instance().exts().khr_d3d11_enable.is_some() {
            xr::sys::SwapchainImageD3D11KHR::TYPE
        } else { xr::sys::SwapchainImageD3D12KHR::TYPE };
        let mut images = vec![xr::sys::SwapchainImageD3D11KHR { ty, next: core::ptr::null_mut(), texture: core::ptr::null_mut() }; count as usize];
        let result = unsafe { enumerate(s.as_raw(), count, &mut count, images.as_mut_ptr().cast()) };
        if result.into_raw() < 0 { return Err(result); }
        Ok(images.into_iter().take(count as usize).map(|i| i.texture.cast()).collect())
    }
}
