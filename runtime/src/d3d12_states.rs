//! LibOVR hands D3D12 textures to the game in PIXEL_SHADER_RESOURCE state.
//! OpenXR hands color images over in RENDER_TARGET state and requires them
//! returned that way. Record each boundary transition once per image.
use core::{ffi::c_void, mem::ManuallyDrop};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

pub struct ColorStates {
    queue: ID3D12CommandQueue,
    // Retain the allocator until the recorded lists have finished on the GPU.
    _allocator: ID3D12CommandAllocator,
    acquire: Vec<ID3D12CommandList>,
    release: Vec<ID3D12CommandList>,
}

pub unsafe fn transition(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
    let mut barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 { Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: ManuallyDrop::new(Some(resource.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: before, StateAfter: after,
        }) },
        ..Default::default()
    };
    unsafe {
        list.ResourceBarrier(core::slice::from_ref(&barrier));
        ManuallyDrop::drop(&mut (*barrier.Anonymous.Transition).pResource);
    }
}

impl ColorStates {
    pub unsafe fn new(queue: &ID3D12CommandQueue, images: &[usize]) -> Result<Self, String> {
        let mut device: Option<ID3D12Device> = None;
        unsafe { queue.GetDevice(&mut device) }.map_err(|e| e.to_string())?;
        let device = device.ok_or("D3D12 queue has no device")?;
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.map_err(|e| e.to_string())?;
        let record = |resource: &ID3D12Resource, before, after| -> Result<ID3D12CommandList, String> {
            let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None::<&ID3D12PipelineState>) }.map_err(|e| e.to_string())?;
            unsafe { transition(&list, resource, before, after); list.Close() }.map_err(|e| e.to_string())?;
            list.cast().map_err(|e| e.to_string())
        };
        let mut acquire = Vec::new();
        let mut release = Vec::new();
        for &raw in images {
            let raw = raw as *mut c_void;
            let resource = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or("null D3D12 image")?;
            acquire.push(record(resource, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)?);
            release.push(record(resource, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET)?);
        }
        crate::capi::log_call("D3D12 OpenXR/LibOVR image-state transitions prepared");
        Ok(Self { queue: queue.clone(), _allocator: allocator, acquire, release })
    }

    pub fn acquire(&self, image: u32) {
        unsafe { self.queue.ExecuteCommandLists(&[Some(self.acquire[image as usize].clone())]); }
    }
    pub fn release(&self, image: u32) {
        unsafe { self.queue.ExecuteCommandLists(&[Some(self.release[image as usize].clone())]); }
    }
    /// Called only when replacing the swapchain, never as a per-frame CPU wait.
    pub fn finish(&self) -> Result<(), String> { unsafe { finish_queue(&self.queue) } }
}

pub unsafe fn finish_queue(queue: &ID3D12CommandQueue) -> Result<(), String> {
    let mut device: Option<ID3D12Device> = None;
    unsafe { queue.GetDevice(&mut device) }.map_err(|e| e.to_string())?;
    let device = device.ok_or("D3D12 queue has no device")?;
    let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.map_err(|e| e.to_string())?;
    unsafe { queue.Signal(&fence, 1) }.map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        unsafe { device.GetDeviceRemovedReason() }.map_err(|e| format!("D3D12 device removed: {e}"))?;
        if unsafe { fence.GetCompletedValue() } >= 1 { return Ok(()); }
        if start.elapsed() > std::time::Duration::from_secs(5) { return Err("D3D12 queue completion timed out".into()); }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
