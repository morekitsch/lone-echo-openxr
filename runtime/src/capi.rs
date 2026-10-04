//! Bootstrap LibOVR exports and call tracing.
//!
//! This is intentionally a virtual, non-rendering session. Its purpose is to
//! identify Echo's required call sequence before OpenXR/D3D code is introduced.

use core::ffi::c_char;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::time::Instant;

use crate::abi::{
    OVR_AUDIO_MAX_DEVICE_STR_SIZE, OVR_EYE_LEFT, OVR_SUCCESS, OvrErrorInfo, OvrEyeRenderDesc,
    OvrEyeType, OvrFovPort, OvrGraphicsLuid, OvrHapticsBuffer, OvrHmdDesc, OvrInitParams,
    OvrInputState, OvrLayerEyeFov, OvrLayerHeader, OvrResult, OvrSession, OvrSessionStatus,
    OvrSizei, OvrTextureSwapChain, OvrTextureSwapChainDesc, OvrTrackerDesc, OvrTrackerPose,
    OvrTrackingState, OvrVector2f, OvrVector3f, OvrVersionString,
};

static CLIENT_MINOR_VERSION: AtomicU32 = AtomicU32::new(94);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static TRACKING_ORIGIN: AtomicI32 = AtomicI32::new(0); // LibOVR defaults to eye level.
static LOGGING_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static BUFFERED_LOGGING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static LOG_BUFFER: std::sync::Mutex<Option<crate::log_buffer::LogBuffer>> = std::sync::Mutex::new(None);
static LAST_LOGGED_TRACKING_STATUS: AtomicI32 = AtomicI32::new(-1);
static XR_REFRESH_RATE_BITS: AtomicU32 = AtomicU32::new(90.0f32.to_bits());
static XR_EYE_WIDTH: AtomicI32 = AtomicI32::new(1832);
static XR_EYE_HEIGHT: AtomicI32 = AtomicI32::new(1920);
// Nominal values are used until a cache from a prior real graphics session is
// available. Values are positive LibOVR tangent magnitudes.
static XR_LEFT_FOV_TANS: [AtomicU32; 4] = [
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
];
static XR_RIGHT_FOV_TANS: [AtomicU32; 4] = [
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
    AtomicU32::new(1.0f32.to_bits()),
];
static XR_LEFT_EYE_OFFSET: [AtomicU32; 3] = [
    AtomicU32::new((-0.032f32).to_bits()),
    AtomicU32::new(0.0f32.to_bits()),
    AtomicU32::new(0.0f32.to_bits()),
];
static XR_RIGHT_EYE_OFFSET: [AtomicU32; 3] = [
    AtomicU32::new(0.032f32.to_bits()),
    AtomicU32::new(0.0f32.to_bits()),
    AtomicU32::new(0.0f32.to_bits()),
];
static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub(crate) fn set_openxr_refresh_rate(rate: f32) {
    if rate.is_finite() && rate > 1.0 {
        XR_REFRESH_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
    }
}

pub(crate) fn set_openxr_eye_resolution(width: i32, height: i32) {
    if width > 0 && height > 0 {
        XR_EYE_WIDTH.store(width, Ordering::Relaxed);
        XR_EYE_HEIGHT.store(height, Ordering::Relaxed);
    }
}

pub(crate) fn set_openxr_eye_fovs(fovs: [OvrFovPort; 2]) {
    for (target, fov) in [(&XR_LEFT_FOV_TANS, fovs[0]), (&XR_RIGHT_FOV_TANS, fovs[1])] {
        let values = [fov.up_tan, fov.down_tan, fov.left_tan, fov.right_tan];
        if values.iter().all(|value| value.is_finite() && *value > 0.0) {
            for (slot, value) in target.iter().zip(values) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

fn openxr_eye_fov(tans: &[AtomicU32; 4]) -> OvrFovPort {
    OvrFovPort {
        up_tan: f32::from_bits(tans[0].load(Ordering::Relaxed)),
        down_tan: f32::from_bits(tans[1].load(Ordering::Relaxed)),
        left_tan: f32::from_bits(tans[2].load(Ordering::Relaxed)),
        right_tan: f32::from_bits(tans[3].load(Ordering::Relaxed)),
    }
}

pub(crate) fn set_openxr_eye_offsets(offsets: [OvrVector3f; 2]) {
    for (target, offset) in [
        (&XR_LEFT_EYE_OFFSET, offsets[0]),
        (&XR_RIGHT_EYE_OFFSET, offsets[1]),
    ] {
        let values = [offset.x, offset.y, offset.z];
        if values.iter().all(|value| value.is_finite()) {
            for (slot, value) in target.iter().zip(values) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

fn openxr_eye_offset(offset: &[AtomicU32; 3]) -> OvrVector3f {
    OvrVector3f {
        x: f32::from_bits(offset[0].load(Ordering::Relaxed)),
        y: f32::from_bits(offset[1].load(Ordering::Relaxed)),
        z: f32::from_bits(offset[2].load(Ordering::Relaxed)),
    }
}
#[cfg(windows)]
static XR_ADAPTER_LUID: std::sync::Mutex<Option<[u8; 8]>> = std::sync::Mutex::new(None);
static SESSION_TOKEN: u8 = 1;
static VERSION: &[u8] = b"LibOVR OpenXR shim (bootstrap)\0";

fn monotonic_time_seconds() -> f64 {
    PROCESS_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs_f64()
}

#[cfg(windows)]
fn current_time_seconds() -> f64 {
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            return session.openxr_time_seconds();
        }
    }
    monotonic_time_seconds()
}

#[cfg(not(windows))]
fn current_time_seconds() -> f64 {
    monotonic_time_seconds()
}

#[repr(C)]
struct D3d11Texture2dDesc {
    width: u32,
    height: u32,
    mip_levels: u32,
    array_size: u32,
    format: u32,
    sample_count: u32,
    sample_quality: u32,
    usage: u32,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

type CreateTexture2d = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const D3d11Texture2dDesc,
    *const core::ffi::c_void,
    *mut *mut core::ffi::c_void,
) -> i32;
type QueryInterface = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const Guid,
    *mut *mut core::ffi::c_void,
) -> i32;
type Release = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;
type GetDevice = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const Guid,
    *mut *mut core::ffi::c_void,
) -> i32;
type CreateCommittedResource = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const D3d12HeapProperties,
    u32,
    *const D3d12ResourceDesc,
    u32,
    *const core::ffi::c_void,
    *const Guid,
    *mut *mut core::ffi::c_void,
) -> i32;

#[repr(C)]
struct D3d12HeapProperties {
    heap_type: u32,
    cpu_page_property: u32,
    memory_pool_preference: u32,
    creation_node_mask: u32,
    visible_node_mask: u32,
}

#[repr(C)]
struct D3d12ClearValue {
    format: u32,
    depth: f32,
    stencil: u8,
    // D3D12_CLEAR_VALUE's union is 16 bytes (RGBA floats), even for depth.
    _padding: [u8; 11],
}

#[repr(C)]
struct D3d12ResourceDesc {
    dimension: u32,
    alignment: u64,
    width: u64,
    height: u32,
    depth_or_array_size: u16,
    mip_levels: u16,
    format: u32,
    sample_count: u32,
    sample_quality: u32,
    layout: u32,
    flags: u32,
}

// Oculus bind flags use bit 1 for UAV and bit 2 for depth; D3D12 swaps them.
pub(crate) fn d3d12_resource_flags(bind_flags: u32) -> u32 {
    (bind_flags & 1) | ((bind_flags & 2) << 1) | ((bind_flags & 4) >> 1)
}

std::thread_local! {
    static LAST_ERROR: std::cell::Cell<OvrErrorInfo> = const { std::cell::Cell::new(OvrErrorInfo {
        result: OVR_SUCCESS, error_string: [0; 512],
    }) };
}

fn graphics_error(message: String) -> OvrResult {
    log_call(&message);
    let mut info = OvrErrorInfo { result: -1006, error_string: [0; 512] }; // ovrError_ServiceError
    for (dest, byte) in info.error_string.iter_mut().take(511).zip(message.bytes()) {
        *dest = byte as core::ffi::c_char;
    }
    LAST_ERROR.set(info);
    info.result
}

const IID_ID3D12_COMMAND_QUEUE: Guid = Guid {
    data1: 0x0ec8_70a6,
    data2: 0x5d7e,
    data3: 0x4c22,
    data4: [0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed],
};
const IID_ID3D12_DEVICE: Guid = Guid {
    data1: 0x1898_19f1,
    data2: 0x1db6,
    data3: 0x4b57,
    data4: [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7],
};
const IID_ID3D12_RESOURCE: Guid = Guid {
    data1: 0x6964_42be,
    data2: 0xa72e,
    data3: 0x4059,
    data4: [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad],
};

/// LibOVR swapchain state. Echo creates separate color and depth chains; each
/// needs stable opaque identity and independently-owned D3D resources.
#[derive(Debug)]
struct SwapChainState {
    textures: Vec<usize>,
    current_index: i32,
    /// OpenXR chooses this chain's image through xrAcquireSwapchainImage.
    openxr_color: bool,
}

static SWAP_CHAINS: std::sync::Mutex<Vec<Box<SwapChainState>>> = std::sync::Mutex::new(Vec::new());

// OpenXR session lifetime is process-wide, matching LibOVR's singleton HMD
// session. The frame bridge will use this retained session rather than the old
// create-and-destroy diagnostic probe.
#[cfg(windows)]
static XR_SESSION: std::sync::Mutex<Option<crate::openxr_backend::Direct3DSession>> =
    std::sync::Mutex::new(None);

#[cfg(windows)]
unsafe fn ensure_direct3d_session(
    device: *mut core::ffi::c_void,
    queue: *mut core::ffi::c_void,
) -> Result<(), String> {
    let mut slot = XR_SESSION
        .lock()
        .map_err(|_| "OpenXR session lock poisoned".to_owned())?;
    if slot.is_none() {
        let mut session = unsafe { crate::openxr_backend::create_direct3d_session(device, queue) }?;
        session.set_tracking_origin(TRACKING_ORIGIN.load(Ordering::Acquire))?;
        *slot = Some(session);
    }
    Ok(())
}

fn register_swap_chain(
    textures: Vec<usize>,
    openxr_color: bool,
) -> Result<OvrTextureSwapChain, OvrResult> {
    let mut chains = SWAP_CHAINS.lock().map_err(|_| -1000)?;
    let chain = Box::new(SwapChainState {
        textures,
        current_index: 0,
        openxr_color,
    });
    let handle = (&*chain as *const SwapChainState).cast_mut().cast();
    chains.push(chain);
    Ok(handle)
}

fn swap_chain_state_mut(
    chains: &mut [Box<SwapChainState>],
    chain: OvrTextureSwapChain,
) -> Result<&mut SwapChainState, OvrResult> {
    chains
        .iter_mut()
        .find(|candidate| std::ptr::eq(&***candidate, chain.cast()))
        .map(|candidate| &mut **candidate)
        .ok_or(-1005)
}

fn swap_chain_texture(chain: OvrTextureSwapChain, index: i32) -> Result<usize, OvrResult> {
    if index < 0 {
        return Err(-1005);
    }
    let chains = SWAP_CHAINS.lock().map_err(|_| -1000)?;
    chains
        .iter()
        .find(|candidate| std::ptr::eq(&***candidate, chain.cast()))
        .and_then(|candidate| candidate.textures.get(index as usize).copied())
        .ok_or(-1005)
}

pub(crate) fn log_call(name: &str) {
    // Per-frame file I/O is intentionally opt-in. Set this in Steam launch
    // options (or the launcher environment) when collecting diagnostics.
    if !*LOGGING_ENABLED.get_or_init(|| {
        std::env::var_os("LIBOVR_OPENXR_LOG").is_some_and(|value| value != "0" && !value.is_empty())
    }) {
        return;
    }
    // Opt-in timing investigation: keep the most recent 32 MiB in memory and
    // write only at normal shutdown. A crash can lose this trace. The mutex and
    // formatting still cost time; this is lower overhead, not uninstrumented.
    if *BUFFERED_LOGGING.get_or_init(|| {
        std::env::var("LIBOVR_OPENXR_LOG").is_ok_and(|value| value == "buffered")
    }) {
        if let Ok(mut buffer) = LOG_BUFFER.lock() {
            buffer.get_or_insert_with(|| crate::log_buffer::LogBuffer::new(32 * 1024 * 1024))
                .push(format!("[{:.6} pid={} {:?}] {name}\n", monotonic_time_seconds(), std::process::id(), std::thread::current().id()));
        }
        return;
    }
    // Keep each record intact when input and render threads log concurrently.
    // This clock is independent of XR session creation and reference spaces.
    static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_guard) = LOG_LOCK.lock() else { return; };
    let line = format!("[{:.6} pid={} {:?}] {name}\n", monotonic_time_seconds(), std::process::id(), std::thread::current().id());
    write_log(&line);
}

fn write_log(line: &str) {
    let temp = std::env::var_os("TEMP").unwrap_or_else(|| "C:\\windows\\temp".into());
    let temp_path = std::path::PathBuf::from(temp).join("libovr-openxr.log");
    // Always write adjacent to echovr.exe too. Wine's TEMP may be an
    // unmapped/host-specific path in a new Steam prefix, while current_exe is
    // the portable deployment location for this DLL.
    let game_path = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("libovr-openxr.log")));
    for path in std::iter::once(temp_path).chain(game_path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Initialize(_params: *const OvrInitParams) -> OvrResult {
    log_call("ovr_Initialize");
    let minor = if !_params.is_null() && unsafe { (*_params).flags } & 4 != 0 {
        unsafe { (*_params).requested_minor_version }
    } else { 94 };
    CLIENT_MINOR_VERSION.store(minor, Ordering::Release);
    log_call(&format!("CAPI requested version=1.{minor}"));
    match crate::hmd_cache::load() {
        Ok(Some(cache)) => {
            set_openxr_eye_fovs(cache.fovs());
            set_openxr_eye_offsets(cache.eye_offsets());
            let (width, height) = cache.eye_size();
            set_openxr_eye_resolution(width, height);
            set_openxr_refresh_rate(cache.refresh_rate());
            log_call("OpenXR HMD cache loaded");
        }
        Ok(None) => log_call("OpenXR HMD cache not found; using startup defaults"),
        Err(error) => log_call(&format!("OpenXR HMD cache ignored: {error}")),
    }
    #[cfg(windows)]
    match crate::openxr_backend::adapter_luid() {
        Ok(luid) => {
            if let Ok(mut cached) = XR_ADAPTER_LUID.lock() {
                *cached = Some(luid);
            }
            log_call("openxr graphics adapter LUID discovered");
        }
        Err(error) => log_call(&format!("openxr graphics adapter LUID unavailable: {error}")),
    }
    if std::env::var_os("LIBOVR_OPENXR_PROBE").is_some() {
        log_call(&format!(
            "openxr env XR_RUNTIME_JSON={:?} XDG_RUNTIME_DIR={:?}",
            std::env::var_os("XR_RUNTIME_JSON"),
            std::env::var_os("XDG_RUNTIME_DIR")
        ));
        match crate::openxr_backend::probe() {
            Ok(capabilities) => log_call(&format!(
                "openxr probe d3d11={} d3d12={} mnd_headless={}",
                capabilities.d3d11, capabilities.d3d12, capabilities.monado_headless
            )),
            Err(error) => log_call(&format!("openxr probe failed: {error}")),
        }
    }
    INITIALIZED.store(true, Ordering::Release);
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Shutdown() {
    log_call("ovr_Shutdown");
    INITIALIZED.store(false, Ordering::Release);
    if let Ok(mut buffer) = LOG_BUFFER.lock() {
        if let Some(buffer) = buffer.as_mut() { write_log(&buffer.drain()); }
    }
}

/// # Safety
/// `session` must reference writable CAPI storage; `luid`, if non-null, must
/// reference writable `ovrGraphicsLuid` storage.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_Create(
    session: *mut OvrSession,
    luid: *mut OvrGraphicsLuid,
) -> OvrResult {
    log_call("ovr_Create");
    if !INITIALIZED.load(Ordering::Acquire) || session.is_null() {
        return -1004; // ovrError_NotInitialized
    }
    unsafe {
        *session = (&SESSION_TOKEN as *const u8).cast_mut().cast();
        if !luid.is_null() {
            #[cfg(windows)]
            {
                *luid = XR_ADAPTER_LUID
                    .lock()
                    .ok()
                    .and_then(|cached| *cached)
                    .map(|reserved| OvrGraphicsLuid { reserved })
                    .unwrap_or_default();
            }
            #[cfg(not(windows))]
            {
                *luid = OvrGraphicsLuid::default();
            }
        }
    }
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackingState(
    _session: OvrSession,
    absolute_time: f64,
    _latency_marker: u8,
) -> OvrTrackingState {
    log_call("ovr_GetTrackingState");
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            let pose = session.head_pose;
            let head_pose = crate::abi::OvrPosef {
                orientation: crate::abi::OvrQuatf {
                    x: pose.orientation.x,
                    y: pose.orientation.y,
                    z: pose.orientation.z,
                    w: pose.orientation.w,
                },
                position: OvrVector3f {
                    x: pose.position.x,
                    y: pose.position.y,
                    z: pose.position.z,
                },
            };
            // Echo's legacy Rift-S provider path requires the historical
            // connection bits in addition to the standard status bits. Do not
            // withdraw those during WiVRn startup/transient tracking loss;
            // that makes pnsovr abort platform initialization before frames.
            // Echo's legacy Rift-S provider path requires the historical
            // connection bits in addition to the standard status bits. Do not
            // withdraw those during WiVRn startup/transient tracking loss;
            // that makes pnsovr abort platform initialization before frames.
            let status_flags: u32 = 0xe3;
            if LAST_LOGGED_TRACKING_STATUS.swap(status_flags as i32, Ordering::Relaxed)
                != status_flags as i32
            {
                log_call(&format!(
                    "ovr_GetTrackingState OpenXR status_flags={status_flags:#x} head_flags={:?}",
                    session.head_location_flags
                ));
            }
            return OvrTrackingState {
                head_pose: crate::abi::OvrPoseStatef {
                    pose: head_pose,
                    time_in_seconds: absolute_time,
                    ..Default::default()
                },
                hand_poses: core::array::from_fn(|index| crate::abi::OvrPoseStatef {
                    pose: crate::abi::OvrPosef {
                        orientation: crate::abi::OvrQuatf {
                            x: session.hand_poses[index].orientation.x,
                            y: session.hand_poses[index].orientation.y,
                            z: session.hand_poses[index].orientation.z,
                            w: session.hand_poses[index].orientation.w,
                        },
                        position: OvrVector3f {
                            x: session.hand_poses[index].position.x,
                            y: session.hand_poses[index].position.y,
                            z: session.hand_poses[index].position.z,
                        },
                    },
                    angular_velocity: OvrVector3f {
                        x: session.hand_angular_velocity[index].x,
                        y: session.hand_angular_velocity[index].y,
                        z: session.hand_angular_velocity[index].z,
                    },
                    linear_velocity: OvrVector3f {
                        x: session.hand_linear_velocity[index].x,
                        y: session.hand_linear_velocity[index].y,
                        z: session.hand_linear_velocity[index].z,
                    },
                    time_in_seconds: absolute_time,
                    ..Default::default()
                }),
                hand_status_flags: [0x3; 2],
                calibrated_origin: {
                    let origin = session.origin_offsets[session.tracking_origin as usize];
                    crate::abi::OvrPosef {
                        orientation: crate::abi::OvrQuatf { x: origin.orientation.x, y: origin.orientation.y, z: origin.orientation.z, w: origin.orientation.w },
                        position: OvrVector3f { x: origin.position.x, y: origin.position.y, z: origin.position.z },
                    }
                },
                status_flags,
                ..Default::default()
            };
        }
    }
    OvrTrackingState {
        head_pose: crate::abi::OvrPoseStatef {
            pose: crate::abi::OvrPosef {
                orientation: crate::abi::OvrQuatf {
                    w: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            time_in_seconds: absolute_time,
            ..Default::default()
        },
        calibrated_origin: crate::abi::OvrPosef {
            orientation: crate::abi::OvrQuatf {
                w: 1.0,
                ..Default::default()
            },
            ..Default::default()
        },
        // Report both tracking and connection: Echo treats a tracked-but-not-
        // connected HMD as a sensor/device failure.
        // ovrStatus_OrientationTracked | ovrStatus_PositionTracked |
        // ovrStatus_OrientationConnected | ovrStatus_PositionConnected |
        // ovrStatus_HmdConnected.
        status_flags: 0xe3,
        ..Default::default()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetInputState(
    _session: OvrSession,
    controller_type: u32,
    input_state: *mut OvrInputState,
) -> OvrResult {
    log_call("ovr_GetInputState");
    if input_state.is_null() {
        return -1005;
    }
    let selected = if controller_type == 0xff || controller_type == u32::MAX { 3 } else { controller_type & 3 };
    #[allow(unused_mut)]
    let mut state = OvrInputState {
        controller_type: selected,
        ..Default::default()
    };
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            state.time_in_seconds = session.openxr_time_seconds();
            for hand in 0..2 {
                if selected & (1 << hand) == 0 { continue; }
                // ovrButton: A/B/RThumb/RShoulder and X/Y/LThumb/LShoulder.
                // The simple-controller select action remains a useful trigger
                // fallback for runtimes without a vendor controller profile.
                if session.hand_primary[hand] || session.hand_select[hand] {
                    state.buttons |= if hand == 0 { 0x0000_0100 } else { 0x0000_0001 };
                }
                if session.hand_secondary[hand] {
                    state.buttons |= if hand == 0 { 0x0000_0200 } else { 0x0000_0002 };
                }
                if session.hand_thumbstick_click[hand] {
                    state.buttons |= if hand == 0 { 0x0000_0400 } else { 0x0000_0004 };
                }
                if session.hand_menu[hand] {
                    state.buttons |= if hand == 0 { 0x0010_0000 } else { 0 };
                }
                // ovrTouch: A/B/RThumb/RIndex and X/Y/LThumb/LIndex.
                if session.hand_primary_touch[hand] {
                    state.touches |= if hand == 0 { 0x0000_0100 } else { 0x0000_0001 };
                }
                if session.hand_secondary_touch[hand] {
                    state.touches |= if hand == 0 { 0x0000_0200 } else { 0x0000_0002 };
                }
                if session.hand_thumbstick_touch[hand] {
                    state.touches |= if hand == 0 { 0x0000_0400 } else { 0x0000_0004 };
                }
                let trigger = session.hand_trigger[hand].max(if session.hand_select[hand] {
                    1.0
                } else {
                    0.0
                });
                state.index_trigger[hand] = trigger;
                state.index_trigger_no_deadzone[hand] = trigger;
                if session.hand_trigger_touch[hand] || trigger > 0.0 {
                    state.touches |= if hand == 0 { 0x0000_1000 } else { 0x0000_0010 };
                } else {
                    // LibOVR exposes pointing separately from trigger contact.
                    // Echo uses this pose bit for a straight index finger; it
                    // is not implied by the analog trigger value or grip.
                    state.touches |= if hand == 0 { 0x0000_2000 } else { 0x0000_0020 };
                }
                state.hand_trigger[hand] = session.hand_squeeze[hand];
                state.hand_trigger_no_deadzone[hand] = session.hand_squeeze[hand];
                state.index_trigger_raw[hand] = trigger;
                state.hand_trigger_raw[hand] = session.hand_squeeze[hand];
                state.thumbstick[hand] = OvrVector2f {
                    x: session.hand_thumbstick[hand].x,
                    y: session.hand_thumbstick[hand].y,
                };
                state.thumbstick_no_deadzone[hand] = state.thumbstick[hand];
                state.thumbstick_raw[hand] = state.thumbstick[hand];
                if session.hand_select[hand]
                    || session.hand_primary[hand]
                    || trigger > 0.0
                    || session.hand_squeeze[hand] > 0.0
                {
                    log_call(&format!(
                        "ovr_GetInputState {} buttons={:#x} trigger={:.2} squeeze={:.2} stick=({:.2},{:.2})",
                        if hand == 0 { "left" } else { "right" },
                        state.buttons,
                        trigger,
                        session.hand_squeeze[hand],
                        state.thumbstick[hand].x,
                        state.thumbstick[hand].y,
                    ));
                }
            }
        }
    }
    unsafe {
        write_input_state(input_state.cast(), &state, CLIENT_MINOR_VERSION.load(Ordering::Acquire));
    };
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackerDesc(
    _session: OvrSession,
    tracker_desc_index: u32,
) -> OvrTrackerDesc {
    log_call("ovr_GetTrackerDesc");
    if tracker_desc_index >= tracker_count(CLIENT_MINOR_VERSION.load(Ordering::Acquire)) {
        return OvrTrackerDesc::default();
    }
    // CV1 constellation-camera envelope; Echo uses this to classify the
    // synthetic trackers as room-scale cameras rather than unknown devices.
    OvrTrackerDesc {
        frustum_hfov_in_radians: 1.75,
        frustum_vfov_in_radians: 1.40,
        frustum_near_z_in_meters: 0.4,
        frustum_far_z_in_meters: 2.5,
    }
}

/// # Safety
/// All output arrays must hold `device_count` entries when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetDevicePoses(
    _session: OvrSession,
    device_types: *const i32,
    device_count: i32,
    absolute_time: f64,
    out_device_poses: *mut crate::abi::OvrPoseStatef,
) -> OvrResult {
    log_call("ovr_GetDevicePoses");
    if device_count < 0
        || (device_count > 0 && (device_types.is_null() || out_device_poses.is_null()))
    {
        return -1005;
    }
    let tracking = ovr_GetTrackingState(_session, absolute_time, 0);
    for index in 0..device_count as usize {
        let device_type = unsafe { *device_types.add(index) };
        // ovrTrackedDeviceType is a bitmask: HMD=1, LTouch=2, RTouch=4.
        unsafe {
            *out_device_poses.add(index) = match device_type {
                0x0001 => tracking.head_pose,
                0x0002 => tracking.hand_poses[0],
                0x0004 => tracking.hand_poses[1],
                _ => crate::abi::OvrPoseStatef {
                    pose: tracking.head_pose.pose,
                    time_in_seconds: absolute_time,
                    ..Default::default()
                },
            };
            let pose = (*out_device_poses.add(index)).pose;
            log_call(&format!(
                "ovr_GetDevicePoses type={device_type:#x} pos=({:.3},{:.3},{:.3}) quat=({:.3},{:.3},{:.3},{:.3})",
                pose.position.x,
                pose.position.y,
                pose.position.z,
                pose.orientation.x,
                pose.orientation.y,
                pose.orientation.z,
                pose.orientation.w,
            ));
        }
    }
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackerPose(
    _session: OvrSession,
    tracker_pose_index: u32,
) -> OvrTrackerPose {
    log_call("ovr_GetTrackerPose");
    if tracker_pose_index >= tracker_count(CLIENT_MINOR_VERSION.load(Ordering::Acquire)) {
        return OvrTrackerPose::default();
    }
    // Do not collapse the three reported CV1 cameras onto the origin: Echo's
    // hardware screen de-duplicates coincident trackers and showed one red
    // sensor as a result. These are room-scale camera locations in LibOVR's
    // local coordinate system.
    let (position, orientation) = match tracker_pose_index {
        // Facing -Z toward the player at the local origin.
        0 => (
            OvrVector3f {
                x: 0.0,
                y: 1.8,
                z: 1.5,
            },
            crate::abi::OvrQuatf {
                w: 1.0,
                ..Default::default()
            },
        ),
        // Side cameras are yawed inward, so their optical axes intersect the
        // HMD rather than pointing parallel to the front camera.
        1 => (
            OvrVector3f {
                x: -1.5,
                y: 1.8,
                z: 0.3,
            },
            crate::abi::OvrQuatf {
                y: -0.634,
                w: 0.773,
                ..Default::default()
            },
        ),
        _ => (
            OvrVector3f {
                x: 1.5,
                y: 1.8,
                z: 0.3,
            },
            crate::abi::OvrQuatf {
                y: 0.634,
                w: 0.773,
                ..Default::default()
            },
        ),
    };
    let pose = crate::abi::OvrPosef {
        orientation,
        position,
    };
    OvrTrackerPose {
        pose,
        leveled_pose: pose,
        // ovrTracker_Connected | ovrTracker_PoseTracked.
        status_flags: 0x24,
        reserved: [0; 4],
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetPredictedDisplayTime(_session: OvrSession, _frame_index: i64) -> f64 {
    log_call("ovr_GetPredictedDisplayTime");
    begin_legacy_frame(_session, _frame_index);
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            if let Some(frame_state) = session.frame_state {
                // OpenXR XrTime and LibOVR's time values are both expressed
                // in seconds from a monotonic runtime clock, not wall time.
                return frame_state.predicted_display_time.as_nanos() as f64 / 1_000_000_000.0;
            }
        }
    }
    // There is no predicted frame before the graphics session is created.
    current_time_seconds()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTextureSwapChainCurrentIndex(
    _session: OvrSession,
    _chain: OvrTextureSwapChain,
    index: *mut i32,
) -> OvrResult {
    log_call("ovr_GetTextureSwapChainCurrentIndex");
    begin_legacy_frame(_session, 0);
    if index.is_null() {
        return -1005;
    }
    let chains = match SWAP_CHAINS.lock() {
        Ok(chains) => chains,
        Err(_) => return -1000,
    };
    let current = match chains
        .iter()
        .find(|candidate| std::ptr::eq(&***candidate, _chain.cast()))
    {
        Some(chain) => chain.current_index,
        None => return -1005,
    };
    unsafe { *index = current };
    log_call(&format!("ovr_GetTextureSwapChainCurrentIndex chain={_chain:p} index={current}"));
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_CommitTextureSwapChain(
    _session: OvrSession,
    _chain: OvrTextureSwapChain,
) -> OvrResult {
    log_call("ovr_CommitTextureSwapChain");
    let mut chains = match SWAP_CHAINS.lock() {
        Ok(chains) => chains,
        Err(_) => return -1000,
    };
    let chain = match swap_chain_state_mut(&mut chains, _chain) {
        Ok(chain) => chain,
        Err(error) => return error,
    };
    log_call(&format!("ovr_CommitTextureSwapChain chain={_chain:p} index={} openxr_color={}", chain.current_index, chain.openxr_color));
    // OpenXR selects the color image in ovr_WaitToBeginFrame. Advancing that
    // index here would make Echo render into a different image than the one
    // released to the compositor. The local depth chain still cycles here.
    if !chain.openxr_color {
        chain.current_index = (chain.current_index + 1) % chain.textures.len() as i32;
    }
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_WaitToBeginFrame(_session: OvrSession, _frame_index: i64) -> OvrResult {
    log_call(&format!("ovr_WaitToBeginFrame frame={_frame_index}"));
    #[cfg(windows)]
    {
        let mut acquired_index = None;
        if let Ok(mut slot) = XR_SESSION.lock() {
            if let Some(session) = slot.as_mut() {
                if let Err(error) = session.poll_events().and_then(|()| session.wait_frame()) {
                    log_call(&format!("openxr wait frame failed: {error}"));
                }
                acquired_index = session.color_image.map(|index| index as i32);
            }
        }
        // Echo queries both its color and depth chains after this call. Its
        // chains have identical image counts, so keep their indices aligned.
        if let Some(index) = acquired_index {
            if let Ok(mut chains) = SWAP_CHAINS.lock() {
                for chain in chains.iter_mut() {
                    chain.current_index = index;
                }
            }
        }
    }
    log_call(&format!("ovr_WaitToBeginFrame complete frame={_frame_index}"));
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_BeginFrame(_session: OvrSession, _frame_index: i64) -> OvrResult {
    log_call(&format!("ovr_BeginFrame frame={_frame_index}"));
    #[cfg(windows)]
    if let Ok(mut slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_mut() {
            if let Err(error) = session.begin_frame() {
                log_call(&format!("openxr begin frame failed: {error}"));
            } else if let Some(index) = session.color_image.map(|index| index as i32) {
                // Acquisition now occurs after xrBeginFrame. Keep LibOVR's
                // color and depth handles aligned with the image Echo should
                // render into for this frame.
                if let Ok(mut chains) = SWAP_CHAINS.lock() {
                    for chain in chains.iter_mut() {
                        chain.current_index = index;
                    }
                }
            }
        }
    }
    log_call(&format!("ovr_BeginFrame complete frame={_frame_index}"));
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_EndFrame(
    _session: OvrSession,
    _frame_index: i64,
    _view_scale_desc: *const core::ffi::c_void,
    layer_ptr_list: *const *const core::ffi::c_void,
    layer_count: u32,
) -> OvrResult {
    log_call(&format!("ovr_EndFrame frame={_frame_index}"));
    // Record the real stereo submission topology before replacing the virtual
    // swapchain with OpenXR-owned D3D12 images. Echo supplies LibOVR-owned,
    // valid pointers for the duration of this call.
    if !layer_ptr_list.is_null() && layer_count != 0 {
        let layer = unsafe { *layer_ptr_list };
        if !layer.is_null() {
            let header = unsafe { &*layer.cast::<OvrLayerHeader>() };
            log_call(&format!(
                "ovr_EndFrame layer_type={} flags={:#x} count={layer_count}",
                header.layer_type, header.flags
            ));
            // `ovrLayerEyeFov` (1) and `ovrLayerEyeFovDepth` (2) share this
            // color-layer prefix; Echo currently submits the latter.
            if matches!(header.layer_type, 1 | 2) {
                let eye_fov = unsafe { &*layer.cast::<OvrLayerEyeFov>() };
                log_call(&format!(
                    "ovr_EndFrame EyeFov{} chains=({:p},{:p}) left={}x{}+{},{} right={}x{}+{},{}",
                    if header.layer_type == 2 { "Depth" } else { "" },
                    eye_fov.color_texture[0],
                    eye_fov.color_texture[1],
                    eye_fov.viewport[0].size.w,
                    eye_fov.viewport[0].size.h,
                    eye_fov.viewport[0].pos.x,
                    eye_fov.viewport[0].pos.y,
                    eye_fov.viewport[1].size.w,
                    eye_fov.viewport[1].size.h,
                    eye_fov.viewport[1].pos.x,
                    eye_fov.viewport[1].pos.y,
                ));
                log_call(&format!(
                    "ovr_EndFrame Fov left=({:.3},{:.3},{:.3},{:.3}) right=({:.3},{:.3},{:.3},{:.3})",
                    eye_fov.fov[0].left_tan,
                    eye_fov.fov[0].right_tan,
                    eye_fov.fov[0].up_tan,
                    eye_fov.fov[0].down_tan,
                    eye_fov.fov[1].left_tan,
                    eye_fov.fov[1].right_tan,
                    eye_fov.fov[1].up_tan,
                    eye_fov.fov[1].down_tan,
                ));
                log_call(&format!(
                    "ovr_EndFrame RenderPose left=p({:.3},{:.3},{:.3}) q({:.3},{:.3},{:.3},{:.3}) right=p({:.3},{:.3},{:.3}) q({:.3},{:.3},{:.3},{:.3})",
                    eye_fov.render_pose[0].position.x,
                    eye_fov.render_pose[0].position.y,
                    eye_fov.render_pose[0].position.z,
                    eye_fov.render_pose[0].orientation.x,
                    eye_fov.render_pose[0].orientation.y,
                    eye_fov.render_pose[0].orientation.z,
                    eye_fov.render_pose[0].orientation.w,
                    eye_fov.render_pose[1].position.x,
                    eye_fov.render_pose[1].position.y,
                    eye_fov.render_pose[1].position.z,
                    eye_fov.render_pose[1].orientation.x,
                    eye_fov.render_pose[1].orientation.y,
                    eye_fov.render_pose[1].orientation.z,
                    eye_fov.render_pose[1].orientation.w,
                ));
            } else {
                log_call("ovr_EndFrame unsupported layer type");
            }
        }
    }
    #[cfg(windows)]
    if let Ok(mut slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_mut() {
            let submitted_views = if !layer_ptr_list.is_null() && layer_count != 0 {
                let layer = unsafe { *layer_ptr_list };
                if !layer.is_null() {
                    let header = unsafe { &*layer.cast::<OvrLayerHeader>() };
                    if matches!(header.layer_type, 1 | 2) {
                        let eye_fov = unsafe { &*layer.cast::<OvrLayerEyeFov>() };
                        let rects = core::array::from_fn(|eye| {
                            let viewport = eye_fov.viewport[eye];
                            openxr::Rect2Di {
                                offset: openxr::Offset2Di {
                                    x: viewport.pos.x,
                                    y: viewport.pos.y,
                                },
                                extent: openxr::Extent2Di {
                                    width: viewport.size.w,
                                    height: viewport.size.h,
                                },
                            }
                        });
                        // CAPI tangents map directly to OpenXR angles. CAPI's
                        // up/left are positive tangents, while OpenXR stores
                        // signed angles from the forward axis.
                        let fovs = core::array::from_fn(|eye| {
                            let fov = eye_fov.fov[eye];
                            openxr::Fovf {
                                angle_left: (-fov.left_tan).atan(),
                                angle_right: fov.right_tan.atan(),
                                angle_up: fov.up_tan.atan(),
                                angle_down: (-fov.down_tan).atan(),
                            }
                        });
                        // Error/UI submissions contain an all-zero layer. Do
                        // not pass invalid OpenXR rectangles in that case.
                        (rects
                            .iter()
                            .all(|rect| rect.extent.width > 0 && rect.extent.height > 0))
                        .then_some((rects, fovs))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if let Err(error) = session.end_frame(submitted_views) {
                log_call(&format!("openxr end frame failed: {error}"));
            }
        }
    }
    log_call(&format!("ovr_EndFrame complete frame={_frame_index}"));
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_SubmitControllerVibration(
    _session: OvrSession,
    controller_type: u32,
    buffer: *const OvrHapticsBuffer,
) -> OvrResult {
    log_call("ovr_SubmitControllerVibration");
    if buffer.is_null() {
        return -1005;
    }
    let buffer = unsafe { &*buffer };
    if !(0..=256).contains(&buffer.samples_count)
        || (buffer.samples_count > 0 && buffer.samples.is_null())
    {
        return -1005;
    }
    if buffer.samples_count == 0 {
        return OVR_SUCCESS;
    }
    // Touch haptic samples are 8-bit amplitudes at 320 Hz. OpenXR accepts one
    // amplitude/duration pulse, so preserve the buffer's mean energy and span.
    let samples = unsafe {
        core::slice::from_raw_parts(buffer.samples.cast::<u8>(), buffer.samples_count as usize)
    };
    let amplitude = samples
        .iter()
        .map(|&sample| sample as f32 / 255.0)
        .sum::<f32>()
        / samples.len() as f32;
    let duration = std::time::Duration::from_secs_f64(samples.len() as f64 / 320.0);
    #[cfg(not(windows))]
    let _ = (controller_type, amplitude, duration);
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            if let Err(error) = session.submit_haptic(controller_type, amplitude, duration) {
                log_call(&format!("OpenXR haptic submission failed: {error}"));
            }
        }
    }
    OVR_SUCCESS
}

/// Echo uses this before creating its world renderer. The unresolved export
/// previously had a `void` ABI, so the game observed an arbitrary register
/// value and could reject the virtual sensor setup.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackerCount(_session: OvrSession) -> u32 {
    log_call("ovr_GetTrackerCount");
    tracker_count(CLIENT_MINOR_VERSION.load(Ordering::Acquire))
}

fn tracker_count(minor: u32) -> u32 {
    // CAPI clients predating inside-out tracking interpret zero external
    // trackers as tracking loss. Revive uses this same 1.37 boundary for
    // virtual sensors. Actual head/hand poses still come from OpenXR.
    if minor < 37 { 3 } else { 0 }
}

fn compatible_hmd_type(minor: u32) -> i32 {
    // Older clients do not recognize the Rift S enum (Revive's 1.38 rule).
    if minor < 38 { 14 } else { 16 } // CV1 / Rift S
}

/// Report the two Touch-style controllers exposed by the OpenXR runtime. Input
/// values remain neutral until action bindings are added, but connectivity must
/// be truthful and deterministic for Echo's device validation.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetConnectedControllerTypes(_session: OvrSession) -> u32 {
    log_call("ovr_GetConnectedControllerTypes");
    0x3 // ovrControllerType_LTouch | ovrControllerType_RTouch
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackingOriginType(_session: OvrSession) -> i32 {
    log_call("ovr_GetTrackingOriginType");
    TRACKING_ORIGIN.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_SetTrackingOriginType(_session: OvrSession, origin: i32) -> OvrResult {
    log_call(&format!("ovr_SetTrackingOriginType origin={origin}"));
    if !(0..=1).contains(&origin) {
        return -1005;
    }
    #[cfg(windows)]
    {
        let mut slot = match XR_SESSION.lock() { Ok(s) => s, Err(_) => return -1000 };
        if let Some(session) = slot.as_mut() {
            if let Err(e) = session.set_tracking_origin(origin) { return graphics_error(format!("SetTrackingOrigin: {e}")); }
        }
    }
    TRACKING_ORIGIN.store(origin, Ordering::Release);
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_RecenterTrackingOrigin(_session: OvrSession) -> OvrResult {
    log_call("ovr_RecenterTrackingOrigin");
    #[cfg(windows)]
    {
        let mut slot = match XR_SESSION.lock() { Ok(s) => s, Err(_) => return -1000 };
        if let Some(session) = slot.as_mut() {
            if let Err(e) = session.recenter_tracking() { return graphics_error(format!("RecenterTrackingOrigin: {e}")); }
        }
    }
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_SpecifyTrackingOrigin(_session: OvrSession, pose: crate::abi::OvrPosef) -> OvrResult {
    log_call(&format!("ovr_SpecifyTrackingOrigin pose={pose:?}"));
    #[cfg(windows)]
    {
        let mut slot = match XR_SESSION.lock() { Ok(s) => s, Err(_) => return -1000 };
        if let Some(session) = slot.as_mut() {
            let offset = openxr::Posef {
                orientation: openxr::Quaternionf { x: pose.orientation.x, y: pose.orientation.y, z: pose.orientation.z, w: pose.orientation.w },
                position: openxr::Vector3f { x: pose.position.x, y: pose.position.y, z: pose.position.z },
            };
            if let Err(e) = session.specify_tracking_origin(offset) { return graphics_error(format!("SpecifyTrackingOrigin: {e}")); }
        }
    }
    OVR_SUCCESS
}

/// `ovr_GetPerfStats` returns an `ovrResult`; it was previously emitted as a
/// void resolver stub, leaving Echo to read an arbitrary failure code.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetPerfStats(
    _session: OvrSession,
    out_stats: *mut core::ffi::c_void,
) -> OvrResult {
    log_call("ovr_GetPerfStats");
    if out_stats.is_null() { return -1005; }
    unsafe { crate::perf_stats::write_empty(out_stats.cast(), CLIENT_MINOR_VERSION.load(Ordering::Acquire)); }
    OVR_SUCCESS
}

/// CAPI property setters return `ovrBool`, not void.
#[unsafe(no_mangle)]
pub extern "system" fn ovr_SetBool(
    _session: OvrSession,
    _property_name: *const c_char,
    _value: u8,
) -> u8 {
    log_call("ovr_SetBool");
    1
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_Destroy(_session: OvrSession) {
    log_call("ovr_Destroy");
}

/// # Safety
/// `status` must reference writable CAPI `ovrSessionStatus` storage.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetSessionStatus(
    _session: OvrSession,
    status: *mut OvrSessionStatus,
) -> OvrResult {
    log_call("ovr_GetSessionStatus");
    if status.is_null() {
        return -1005; // ovrError_InvalidParameter
    }
    let value = OvrSessionStatus {
        is_visible: 1, hmd_present: 1, hmd_mounted: 1, has_input_focus: 1,
        ..OvrSessionStatus::default()
    };
    unsafe { write_session_status(status.cast(), &value, CLIENT_MINOR_VERSION.load(Ordering::Acquire)); }
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetHmdDesc(_session: OvrSession) -> OvrHmdDesc {
    log_call("ovr_GetHmdDesc");
    let refresh_rate = f32::from_bits(XR_REFRESH_RATE_BITS.load(Ordering::Relaxed));
    let mut product = [0; 64];
    for (slot, byte) in product.iter_mut().zip(b"Oculus Rift S") {
        *slot = *byte as c_char;
    }
    let mut manufacturer = [0; 64];
    for (slot, byte) in manufacturer.iter_mut().zip(b"Oculus") {
        *slot = *byte as c_char;
    }
    let mut serial = [0; 24];
    for (slot, byte) in serial.iter_mut().zip(b"OPENXR-RIFTS-0001") {
        *slot = *byte as c_char;
    }
    // `ovr_Initialize` makes a short XR_MND_headless session when supported,
    // so Echo receives the runtime projection before it creates its D3D queue.
    // These remain nominal only if that optional probe is unavailable.
    let left_fov = openxr_eye_fov(&XR_LEFT_FOV_TANS);
    let right_fov = openxr_eye_fov(&XR_RIGHT_FOV_TANS);
    OvrHmdDesc {
        hmd_type: compatible_hmd_type(CLIENT_MINOR_VERSION.load(Ordering::Acquire)),
        product_name: product,
        manufacturer,
        vendor_id: 0x2833,
        product_id: 0x021e,
        serial_number: serial,
        firmware_major: 1,
        firmware_minor: 0,
        resolution: crate::abi::OvrSizei {
            w: XR_EYE_WIDTH.load(Ordering::Relaxed) * 2,
            h: XR_EYE_HEIGHT.load(Ordering::Relaxed),
        },
        default_eye_fov: [left_fov, right_fov],
        max_eye_fov: [left_fov, right_fov],
        display_refresh_rate: refresh_rate,
        available_tracking_caps: 0x50,
        default_tracking_caps: 0x50,
        ..unsafe { core::mem::zeroed() }
    }
}

/// # Safety
/// `error_info`, if non-null, must reference writable CAPI error-info storage.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetLastErrorInfo(error_info: *mut OvrErrorInfo) {
    log_call("ovr_GetLastErrorInfo");
    if !error_info.is_null() {
        unsafe {
            *error_info = LAST_ERROR.get();
        };
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetVersionString() -> OvrVersionString {
    log_call("ovr_GetVersionString");
    VERSION.as_ptr().cast::<c_char>()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTimeInSeconds() -> f64 {
    log_call("ovr_GetTimeInSeconds");
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            return session.openxr_time_seconds();
        }
    }
    current_time_seconds()
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetFovTextureSize(
    _session: OvrSession,
    eye: OvrEyeType,
    fov: OvrFovPort,
    pixels_per_display_pixel: f32,
) -> OvrSizei {
    let (recommended, default_fov) = eye_render_geometry(eye);
    let size = crate::render_size::texture_size(recommended, default_fov, fov, pixels_per_display_pixel);
    log_call(&format!("ovr_GetFovTextureSize eye={eye} recommended={}x{} density={pixels_per_display_pixel} requested={fov:?} default={default_fov:?} result={}x{}",
        recommended.w, recommended.h, size.w, size.h));
    size
}

fn eye_render_geometry(eye: OvrEyeType) -> (OvrSizei, OvrFovPort) {
    (OvrSizei { w: XR_EYE_WIDTH.load(Ordering::Relaxed), h: XR_EYE_HEIGHT.load(Ordering::Relaxed) },
     openxr_eye_fov(if eye == OVR_EYE_LEFT { &XR_LEFT_FOV_TANS } else { &XR_RIGHT_FOV_TANS }))
}

fn write_audio_guid(out_guid: *mut u16, guid: Option<&[u16]>, direction: &str) -> OvrResult {
    if out_guid.is_null() {
        return -1005;
    }
    unsafe { core::ptr::write_bytes(out_guid, 0, OVR_AUDIO_MAX_DEVICE_STR_SIZE) };
    if let Some(guid) = guid {
        // The CAPI buffer is fixed-size and includes its terminating NUL.
        let count = guid.len().min(OVR_AUDIO_MAX_DEVICE_STR_SIZE - 1);
        unsafe { core::ptr::copy_nonoverlapping(guid.as_ptr(), out_guid, count) };
        log_call(&format!("ovr audio {direction} GUID configured"));
    } else {
        // Empty means WAVE_MAPPER/default device, letting PipeWire/Wine route
        // to the headset sink/source selected by the user.
        log_call(&format!(
            "ovr audio {direction} uses Windows default device"
        ));
    }
    OVR_SUCCESS
}

/// # Safety
/// `out_guid` must point to `OVR_AUDIO_MAX_DEVICE_STR_SIZE` UTF-16 code units.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceOutGuidStr(out_guid: *mut u16) -> OvrResult {
    log_call("ovr_GetAudioDeviceOutGuidStr");
    let configured = crate::config::user_identity().audio_output_guid.as_deref();
    #[cfg(windows)]
    let detected: Option<Vec<u16>> = crate::config::default_audio_device_id(true);
    #[cfg(not(windows))]
    let detected: Option<Vec<u16>> = None;
    write_audio_guid(out_guid, configured.or(detected.as_deref()), "output")
}

/// # Safety
/// `out_guid` must point to `OVR_AUDIO_MAX_DEVICE_STR_SIZE` UTF-16 code units.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceInGuidStr(out_guid: *mut u16) -> OvrResult {
    log_call("ovr_GetAudioDeviceInGuidStr");
    let configured = crate::config::user_identity().audio_input_guid.as_deref();
    #[cfg(windows)]
    let detected: Option<Vec<u16>> = crate::config::default_audio_device_id(false);
    #[cfg(not(windows))]
    let detected: Option<Vec<u16>> = None;
    write_audio_guid(out_guid, configured.or(detected.as_deref()), "input")
}

/// GUID-returning CAPI calls cannot represent a textual device endpoint.
/// A zero GUID deliberately requests Wine's default multimedia endpoint.
unsafe fn write_default_audio_guid(out_guid: *mut Guid) -> OvrResult {
    if out_guid.is_null() {
        return -1005;
    }
    unsafe { core::ptr::write_bytes(out_guid, 0, 1) };
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceOutGuid(out_guid: *mut Guid) -> OvrResult {
    log_call("ovr_GetAudioDeviceOutGuid");
    unsafe { write_default_audio_guid(out_guid) }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceInGuid(out_guid: *mut Guid) -> OvrResult {
    log_call("ovr_GetAudioDeviceInGuid");
    unsafe { write_default_audio_guid(out_guid) }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceOutWaveId(out_id: *mut u32) -> OvrResult {
    log_call("ovr_GetAudioDeviceOutWaveId");
    if out_id.is_null() {
        return -1005;
    }
    unsafe { *out_id = u32::MAX }; // WAVE_MAPPER
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetAudioDeviceInWaveId(out_id: *mut u32) -> OvrResult {
    log_call("ovr_GetAudioDeviceInWaveId");
    if out_id.is_null() {
        return -1005;
    }
    unsafe { *out_id = u32::MAX }; // WAVE_MAPPER
    OVR_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetRenderDesc2(
    _session: OvrSession,
    eye: OvrEyeType,
    fov: OvrFovPort,
) -> OvrEyeRenderDesc {
    log_call("ovr_GetRenderDesc2");
    let (recommended, default_fov) = eye_render_geometry(eye);
    let pixels_per_tan = crate::render_size::pixels_per_tan(recommended, default_fov);
    let mut fov = fov;
    #[cfg(windows)]
    if let Ok(slot) = XR_SESSION.lock() {
        if let Some(session) = slot.as_ref() {
            if session
                .head_location_flags
                .contains(openxr::SpaceLocationFlags::ORIENTATION_VALID)
            {
                let runtime_fov = session.view_fovs[if eye == OVR_EYE_LEFT { 0 } else { 1 }];
                fov = OvrFovPort {
                    up_tan: runtime_fov.angle_up.tan(),
                    down_tan: -runtime_fov.angle_down.tan(),
                    left_tan: -runtime_fov.angle_left.tan(),
                    right_tan: runtime_fov.angle_right.tan(),
                };
            }
        }
    }
    OvrEyeRenderDesc {
        eye,
        fov,
        pixels_per_tan_angle_at_center: pixels_per_tan,
        hmd_to_eye_pose: {
            #[cfg(windows)]
            if let Ok(slot) = XR_SESSION.lock() {
                if let Some(session) = slot.as_ref() {
                    let eye_pose = session.view_poses[if eye == OVR_EYE_LEFT { 0 } else { 1 }];
                    let offset =
                        crate::openxr_backend::hmd_to_eye_offset(session.head_pose, eye_pose);
                    return OvrEyeRenderDesc {
                        eye,
                        fov,
                        pixels_per_tan_angle_at_center: pixels_per_tan,
                        hmd_to_eye_pose: {
                            log_call(&format!(
                                "ovr_GetRenderDesc2 eye={} runtime hmd_to_eye=({:.4},{:.4},{:.4})",
                                eye, offset.x, offset.y, offset.z
                            ));
                            crate::abi::OvrPosef {
                                orientation: crate::abi::OvrQuatf {
                                    w: 1.0,
                                    ..Default::default()
                                },
                                position: OvrVector3f {
                                    x: offset.x,
                                    y: offset.y,
                                    z: offset.z,
                                },
                            }
                        },
                        ..OvrEyeRenderDesc::default()
                    };
                }
            }
            crate::abi::OvrPosef {
                orientation: crate::abi::OvrQuatf {
                    w: 1.0,
                    ..Default::default()
                },
                position: if eye == OVR_EYE_LEFT {
                    openxr_eye_offset(&XR_LEFT_EYE_OFFSET)
                } else {
                    openxr_eye_offset(&XR_RIGHT_EYE_OFFSET)
                },
            }
        },
        ..OvrEyeRenderDesc::default()
    }
}

/// # Safety
/// `out_chain` must be writable. The D3D device and descriptor are accepted
/// for bootstrap only; actual D3D11 resources are implemented in the next
/// compositor stage.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_CreateTextureSwapChainDX(
    _session: OvrSession,
    _d3d_device: *mut core::ffi::c_void,
    _desc: *const OvrTextureSwapChainDesc,
    out_chain: *mut OvrTextureSwapChain,
) -> OvrResult {
    log_call("ovr_CreateTextureSwapChainDX");
    if out_chain.is_null() {
        return -1005;
    }
    if _d3d_device.is_null() || _desc.is_null() {
        return -1005;
    }
    let desc = unsafe { *_desc };
    log_call(&format!("ovr_CreateTextureSwapChainDX descriptor: {desc:?}"));
    let device_vtable = unsafe { *(_d3d_device as *const *const *const core::ffi::c_void) };
    let query_device: QueryInterface = unsafe { core::mem::transmute(*device_vtable.add(0)) };
    let mut queue = core::ptr::null_mut();
    let queue_result = unsafe { query_device(_d3d_device, &IID_ID3D12_COMMAND_QUEUE, &mut queue) };
    if queue_result >= 0 {
        let queue_vtable = unsafe { *(queue as *const *const *const core::ffi::c_void) };
        let get_device: GetDevice = unsafe { core::mem::transmute(*queue_vtable.add(7)) };
        let mut d3d12_device = core::ptr::null_mut();
        let device_result = unsafe { get_device(queue, &IID_ID3D12_DEVICE, &mut d3d12_device) };
        if device_result < 0 {
            log_call(&format!(
                "ovr_CreateTextureSwapChainDX D3D12 GetDevice failed hr={device_result:#x}"
            ));
            let release: Release = unsafe { core::mem::transmute(*queue_vtable.add(2)) };
            unsafe { release(queue) };
            return device_result;
        }
        #[cfg(windows)]
        match unsafe { ensure_direct3d_session(d3d12_device, queue) } {
            Ok(()) => log_call("openxr D3D12 session retained"),
            Err(error) => {
                let release: Release = unsafe { core::mem::transmute(*queue_vtable.add(2)) };
                unsafe { release(queue) };
                let table = unsafe { *(d3d12_device as *const *const *const core::ffi::c_void) };
                let release: Release = unsafe { core::mem::transmute(*table.add(2)) };
                unsafe { release(d3d12_device) };
                return graphics_error(format!("OpenXR D3D12 session creation failed: {error}"));
            }
        }
        let release: Release = unsafe { core::mem::transmute(*queue_vtable.add(2)) };
        unsafe { release(queue) };
        // ovrTextureFormat values are an Oculus enum, not raw DXGI_FORMAT
        // values. Echo uses RGBA8 sRGB (5) for color and D24S8 (12) for its
        // depth chain. D3D12 requires a typeless resource for the latter.
        let (resource_format, clear_format) = match desc.format {
            4 => (28, 28),  // R8G8B8A8_UNORM
            5 => (29, 29),  // R8G8B8A8_UNORM_SRGB
            6 => (87, 87),  // B8G8R8A8_UNORM
            7 => (91, 91),  // B8G8R8A8_UNORM_SRGB
            10 => (10, 10), // R16G16B16A16_FLOAT
            11 => (53, 55), // R16_TYPELESS resource, D16_UNORM view
            12 => (44, 45), // R24G8_TYPELESS resource, D24_UNORM_S8_UINT view
            13 => (39, 40), // R32_TYPELESS resource, D32_FLOAT view
            14 => (19, 20), // R32G8X24_TYPELESS, D32_FLOAT_S8X24_UINT
            25 => (26, 26), // R11G11B10_FLOAT
            _ => (desc.format as u32, desc.format as u32),
        };
        let resource_desc = D3d12ResourceDesc {
            dimension: 3, // D3D12_RESOURCE_DIMENSION_TEXTURE2D
            alignment: 0,
            width: desc.width.max(1) as u64,
            height: desc.height.max(1) as u32,
            depth_or_array_size: desc.array_size.max(1) as u16,
            mip_levels: desc.mip_levels.max(1) as u16,
            format: resource_format,
            sample_count: desc.sample_count.max(1) as u32,
            sample_quality: 0,
            layout: 0, // D3D12_TEXTURE_LAYOUT_UNKNOWN
            flags: d3d12_resource_flags(desc.bind_flags),
        };
        let heap = D3d12HeapProperties {
            heap_type: 1,
            cpu_page_property: 0,
            memory_pool_preference: 0,
            creation_node_mask: 1,
            visible_node_mask: 1,
        };
        let device_vtable = unsafe { *(d3d12_device as *const *const *const core::ffi::c_void) };
        let create: CreateCommittedResource =
            unsafe { core::mem::transmute(*device_vtable.add(27)) };
        let depth_clear = D3d12ClearValue {
            format: clear_format,
            depth: 1.0,
            stencil: 0,
            _padding: [0; 11],
        };
        let optimized_clear = if resource_desc.flags & 0x2 != 0 {
            (&depth_clear as *const D3d12ClearValue).cast()
        } else {
            core::ptr::null()
        };
        #[cfg(windows)]
        if desc.bind_flags & 1 != 0 {
            if let Ok(mut slot) = XR_SESSION.lock() {
                if let Some(session) = slot.as_mut() {
                    match session.create_color_swapchain(
                        resource_desc.format,
                        desc.width.max(1) as u32,
                        desc.height.max(1) as u32,
                        desc.array_size.max(1) as u32,
                        desc.mip_levels.max(1) as u32,
                        desc.sample_count.max(1) as u32,
                        desc.misc_flags & 1 == 0,
                    ) {
                        Ok(textures) if !textures.is_empty() => {
                            let release: Release =
                                unsafe { core::mem::transmute(*device_vtable.add(2)) };
                            unsafe { release(d3d12_device) };
                            let chain = match register_swap_chain(textures, true) {
                                Ok(chain) => chain,
                                Err(error) => return error,
                            };
                            unsafe { *out_chain = chain };
                            log_call("ovr_CreateTextureSwapChainDX using OpenXR D3D12 images");
                            return OVR_SUCCESS;
                        }
                        Ok(images) => log_call(&format!(
                            "OpenXR color swapchain returned only {} images",
                            images.len()
                        )),
                        Err(error) => {
                            log_call(&format!("OpenXR color swapchain creation failed: {error}"))
                        }
                    }
                }
            }
            let release: Release = unsafe { core::mem::transmute(*device_vtable.add(2)) };
            unsafe { release(d3d12_device) };
            return graphics_error("OpenXR D3D12 color swapchain creation failed; see preceding runtime error".into());
        }
        let mut textures = [0; 3];
        for texture in &mut textures {
            let mut created = core::ptr::null_mut();
            let result = unsafe {
                create(
                    d3d12_device,
                    &heap,
                    0,
                    &resource_desc,
                    0x80, // LibOVR exposes D3D12 images in PIXEL_SHADER_RESOURCE state.
                    optimized_clear,
                    &IID_ID3D12_RESOURCE,
                    &mut created,
                )
            };
            if result < 0 {
                #[cfg(windows)]
                let reason = unsafe { crate::d3d12_views::device_status(d3d12_device) };
                #[cfg(not(windows))]
                let reason = 0;
                for &created in &textures {
                    if created != 0 {
                        let raw = created as *mut core::ffi::c_void;
                        let table = unsafe { *(raw as *const *const *const core::ffi::c_void) };
                        let release: Release = unsafe { core::mem::transmute(*table.add(2)) };
                        unsafe { release(raw) };
                    }
                }
                let release: Release = unsafe { core::mem::transmute(*device_vtable.add(2)) };
                unsafe { release(d3d12_device) };
                return graphics_error(format!(
                    "D3D12 CreateCommittedResource failed hr={result:#x} device_reason={reason:#x} format={} flags={:#x} {}x{}",
                    resource_desc.format, resource_desc.flags, resource_desc.width, resource_desc.height,
                ));
            }
            *texture = created as usize;
        }
        let release: Release = unsafe { core::mem::transmute(*device_vtable.add(2)) };
        unsafe { release(d3d12_device) };
        log_call(&format!(
            "ovr_CreateTextureSwapChainDX D3D12 {}x{} array={} mip={} samples={} format={} bind={:#x} flags={:#x}",
            resource_desc.width,
            resource_desc.height,
            resource_desc.depth_or_array_size,
            resource_desc.mip_levels,
            resource_desc.sample_count,
            resource_desc.format,
            desc.bind_flags,
            resource_desc.flags
        ));
        let chain = match register_swap_chain(textures.to_vec(), false) {
            Ok(chain) => chain,
            Err(error) => return error,
        };
        unsafe { *out_chain = chain };
        return OVR_SUCCESS;
    }
    let format = match d3d11_texture_format(desc.format) { Some(f) => f, None => return -1005 };
    #[cfg(windows)]
    {
        if let Err(error) = unsafe { ensure_direct3d_session(_d3d_device, core::ptr::null_mut()) } {
            log_call(&format!("OpenXR D3D11 session creation failed: {error}"));
            return -1004;
        }
        if desc.bind_flags & 1 != 0 {
            let mut slot = match XR_SESSION.lock() { Ok(s) => s, Err(_) => return -1000 };
            let session = slot.as_mut().unwrap();
            match session.create_color_swapchain(format, desc.width.max(1) as u32, desc.height.max(1) as u32, desc.array_size.max(1) as u32, desc.mip_levels.max(1) as u32, desc.sample_count.max(1) as u32, desc.misc_flags & 1 == 0) {
                Ok(images) if !images.is_empty() => {
                    let chain = match register_swap_chain(images, true) { Ok(c) => c, Err(e) => return e };
                    unsafe { *out_chain = chain };
                    log_call("ovr_CreateTextureSwapChainDX using OpenXR D3D11 images");
                    return OVR_SUCCESS;
                }
                Err(e) => log_call(&format!("OpenXR D3D11 swapchain failed: {e}")),
                _ => {},
            }
            return -1004;
        }
    }
    let d3d_desc = D3d11Texture2dDesc {
        width: desc.width.max(1) as u32,
        height: desc.height.max(1) as u32,
        mip_levels: desc.mip_levels.max(1) as u32,
        array_size: desc.array_size.max(1) as u32,
        format,
        sample_count: desc.sample_count.max(1) as u32,
        sample_quality: 0,
        usage: 0, // D3D11_USAGE_DEFAULT
        bind_flags: d3d11_bind_flags(desc.bind_flags),
        cpu_access_flags: 0,
        misc_flags: 0,
    };
    let vtable = unsafe { *(_d3d_device as *const *const *const core::ffi::c_void) };
    let create: CreateTexture2d = unsafe { core::mem::transmute(*vtable.add(5)) };
    let mut textures = [0; 3];
    for texture in &mut textures {
        let mut created = core::ptr::null_mut();
        let result = unsafe { create(_d3d_device, &d3d_desc, core::ptr::null(), &mut created) };
        if result < 0 {
            log_call(&format!(
                "ovr_CreateTextureSwapChainDX failed hr={result:#x} format={} {}x{} bind={:#x}",
                d3d_desc.format, d3d_desc.width, d3d_desc.height, d3d_desc.bind_flags
            ));
            return result;
        }
        *texture = created as usize;
    }
    let chain = match register_swap_chain(textures.to_vec(), false) {
        Ok(chain) => chain,
        Err(error) => return error,
    };
    unsafe { *out_chain = chain };
    OVR_SUCCESS
}

/// # Safety
/// `out_length` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetTextureSwapChainLength(
    _session: OvrSession,
    _chain: OvrTextureSwapChain,
    out_length: *mut i32,
) -> OvrResult {
    log_call("ovr_GetTextureSwapChainLength");
    if out_length.is_null() {
        return -1005;
    }
    let mut chains = match SWAP_CHAINS.lock() { Ok(c) => c, Err(_) => return -1000 };
    let chain = match swap_chain_state_mut(&mut chains, _chain) { Ok(c) => c, Err(e) => return e };
    unsafe { *out_length = chain.textures.len() as i32 };
    OVR_SUCCESS
}

/// # Safety
/// `out_buffer` must be writable and `iid` must identify a D3D interface.
///
/// LibOVR declares `IID` by value (a 16-byte GUID), not as a GUID pointer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetTextureSwapChainBufferDX(
    _session: OvrSession,
    _chain: OvrTextureSwapChain,
    index: i32,
    iid: Guid,
    out_buffer: *mut *mut core::ffi::c_void,
) -> OvrResult {
    log_call("ovr_GetTextureSwapChainBufferDX");
    if out_buffer.is_null() {
        return -1005;
    }
    let texture = match swap_chain_texture(_chain, index) {
        Ok(texture) => texture as *mut core::ffi::c_void,
        Err(error) => return error,
    };
    if texture.is_null() {
        return -1004;
    }
    let vtable = unsafe { *(texture as *const *const *const core::ffi::c_void) };
    let query: QueryInterface = unsafe { core::mem::transmute(*vtable.add(0)) };
    let result = unsafe { query(texture, &iid, out_buffer) };
    if result < 0 {
        log_call(&format!(
            "ovr_GetTextureSwapChainBufferDX failed hr={result:#x} iid={iid:?}"
        ));
        return result;
    }
    log_call(&format!(
        "ovr_GetTextureSwapChainBufferDX ok iid={iid:?} buffer={:p}",
        unsafe { *out_buffer }
    ));
    OVR_SUCCESS
}

/// # Safety
/// Descriptor/buffer must address their CAPI structures. Output arrays must
/// have their declared capacities and must not overlap the structures/each other.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_GetFovStencil(
    session: OvrSession,
    descriptor: *const crate::abi::OvrFovStencilDesc,
    mesh_buffer: *mut crate::abi::OvrFovStencilMeshBuffer,
) -> OvrResult {
    let result = (|| {
        if session != (&SESSION_TOKEN as *const u8).cast_mut().cast() { return -1002; }
        if descriptor.is_null() || mesh_buffer.is_null() { return -1005; }
        if std::env::var("LIBOVR_OPENXR_VISIBILITY_MASK").as_deref() == Ok("0") {
            log_call("ovr_GetFovStencil fallback: disabled by environment");
            return -1009;
        }
        let desc = unsafe { descriptor.read_unaligned() };
        log_call(&format!("ovr_GetFovStencil request eye={} type={} flags={} fov={:?}", desc.eye, desc.stencil_type, desc.stencil_flags, desc.fov));
        if let Err(error) = crate::visibility_mask::validate_desc(&desc) { return error; }
        #[cfg(windows)]
        {
            let Ok(mut slot) = XR_SESSION.lock() else {
                log_call("ovr_GetFovStencil fallback: session lock poisoned");
                return -1009;
            };
            let Some(session) = slot.as_mut() else {
                log_call("ovr_GetFovStencil fallback: graphics session not created");
                return -1009;
            };
            let mesh = session.visibility_mask(desc.eye as usize, desc.stencil_type.min(2) as usize)
                .and_then(|mask| {
                    let mesh = crate::visibility_mask::convert(&desc, mask);
                    if let Err(error) = &mesh {
                        log_call(&format!("ovr_GetFovStencil fallback: mask conversion result={error}"));
                    }
                    mesh
                });
            match mesh {
                Ok(mesh) => unsafe { crate::visibility_mask::write_mesh(&mesh, mesh_buffer) },
                Err(error) => error,
            }
        }
        #[cfg(not(windows))]
        { -1009 }
    })();
    log_call(&format!("ovr_GetFovStencil result={result}"));
    if result == OVR_SUCCESS { return result; }
    let message = match result {
        -1002 => "FOV stencil request has no valid session",
        -1005 => "Invalid FOV stencil descriptor or output buffer",
        _ => "No usable OpenXR visibility mask for this FOV stencil request",
    };
    let mut info = OvrErrorInfo { result, error_string: [0; 512] };
    for (dest, byte) in info.error_string.iter_mut().zip(message.bytes()) {
        *dest = byte as c_char;
    }
    LAST_ERROR.set(info);
    info.result
}

macro_rules! unresolved_exports {
    ($($name:ident),* $(,)?) => {$(
        #[unsafe(no_mangle)]
        pub extern "system" fn $name() {
            log_call(stringify!($name));
        }
    )*};
}

// Resolver-complete exports for this Echo executable. They are intentionally
// inert until each signature/behavior is implemented and tested.
unresolved_exports!(
    ovr_ClearShouldRecenterFlag,
    ovr_CreateMirrorTextureDX,
    ovr_CreateMirrorTextureGL,
    ovr_CreateMirrorTextureWithOptionsDX,
    ovr_CreateMirrorTextureWithOptionsGL,
    ovr_CreateMirrorTextureWithOptionsVk,
    ovr_CreateTextureSwapChainGL,
    ovr_CreateTextureSwapChainVk,
    ovr_DestroyMirrorTexture,
    ovr_EnableExtension,
    ovr_GetBoundaryDimensions,
    ovr_GetBoundaryGeometry,
    ovr_GetBoundaryVisible,
    ovr_GetControllerVibrationState,
    ovr_GetDeviceExtensionsVk,
    ovr_GetExternalCameras,
    ovr_GetHmdColorDesc,
    ovr_GetInstanceExtensionsVk,
    ovr_GetMirrorTextureBufferDX,
    ovr_GetMirrorTextureBufferGL,
    ovr_GetMirrorTextureBufferVk,
    ovr_GetSessionPhysicalDeviceVk,
    ovr_GetTextureSwapChainBufferGL,
    ovr_GetTextureSwapChainBufferVk,
    ovr_GetTextureSwapChainDesc,
    ovr_GetTouchHapticsDesc,
    ovr_IdentifyClient,
    ovr_IsExtensionSupported,
    ovr_Lookup,
    ovr_ReportClientInfo,
    ovr_RequestBoundaryVisible,
    ovr_ResetBoundaryLookAndFeel,
    ovr_ResetPerfStats,
    ovr_SetBoundaryLookAndFeel,
    ovr_SetClientColorDesc,
    ovr_SetControllerVibration,
    ovr_SetExternalCameraProperties,
    ovr_SetFloat,
    ovr_SetFloatArray,
    ovr_SetInt,
    ovr_SetString,
    ovr_SetSynchronizationQueueVk,
    ovr_SubmitFrame2,
    ovr_TestBoundary,
    ovr_TestBoundaryPoint,
    ovr_TraceMessage,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_create_returns_a_session() {
        assert_eq!(ovr_Initialize(core::ptr::null()), OVR_SUCCESS);
        let mut session = core::ptr::null_mut();
        assert_eq!(
            unsafe { ovr_Create(&mut session, core::ptr::null_mut()) },
            OVR_SUCCESS
        );
        assert!(!session.is_null());
        ovr_Shutdown();
    }

    #[test]
    fn version_is_c_string() {
        let text = std::ffi::CStr::from_bytes_with_nul(VERSION).expect("NUL terminated version");
        assert!(text.to_str().expect("utf-8 version").contains("OpenXR"));
    }

    #[test]
    fn unsupported_stencil_returns_failure_without_touching_caller_storage() {
        let descriptor = crate::abi::OvrFovStencilDesc {
            fov: OvrFovPort { left_tan: 1.0, right_tan: 1.0, up_tan: 1.0, down_tan: 1.0 },
            ..Default::default()
        };
        let mut mesh = [0xa5u8; 64];
        let call: unsafe extern "system" fn(OvrSession, *const crate::abi::OvrFovStencilDesc, *mut crate::abi::OvrFovStencilMeshBuffer) -> OvrResult = ovr_GetFovStencil;
        let session = (&SESSION_TOKEN as *const u8).cast_mut().cast();
        for _ in 0..3 {
            assert_eq!(unsafe { call(session, &descriptor, mesh.as_mut_ptr().cast()) }, -1009);
            assert_eq!(mesh, [0xa5; 64]);
            assert_eq!(LAST_ERROR.get().result, -1009);
        }
        assert_eq!(unsafe { call(core::ptr::null_mut(), core::ptr::null(), core::ptr::null_mut()) }, -1002);
        assert_eq!(unsafe { call(session, core::ptr::null(), core::ptr::null_mut()) }, -1005);
    }
}

// LibOVR 1.10 submits a whole frame in one call. Begin the OpenXR frame before
// the game asks for its prediction or render target; SubmitFrame finishes it.
fn begin_legacy_frame(_session: OvrSession, _frame: i64) {
    #[cfg(windows)]
    {
        let needs_begin = XR_SESSION.lock().ok().and_then(|s| s.as_ref().map(|s| s.instance.exts().khr_d3d11_enable.is_some() && !s.frame_begun)).unwrap_or(false);
        if needs_begin {
            ovr_WaitToBeginFrame(_session, _frame);
            ovr_BeginFrame(_session, _frame);
        }
    }
}

#[repr(C)]
pub struct LegacyEyeRenderDesc {
    eye: OvrEyeType,
    fov: OvrFovPort,
    viewport: crate::abi::OvrRecti,
    pixels_per_tan: crate::abi::OvrVector2f,
    eye_offset: OvrVector3f,
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetRenderDesc(session: OvrSession, eye: OvrEyeType, fov: OvrFovPort) -> LegacyEyeRenderDesc {
    let d = ovr_GetRenderDesc2(session, eye, fov);
    LegacyEyeRenderDesc { eye: d.eye, fov: d.fov, viewport: d.distorted_viewport, pixels_per_tan: d.pixels_per_tan_angle_at_center, eye_offset: d.hmd_to_eye_pose.position }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_SubmitFrame(session: OvrSession, frame: i64, scale: *const core::ffi::c_void, layers: *const *const OvrLayerHeader, count: u32) -> OvrResult {
    if count > 16 || (count != 0 && layers.is_null()) { return -1005; }
    // CAPI 1.10 has an 8-byte layer header. Later CAPI versions added a
    // 128-byte reserved tail; reading the old layer as the new one overruns it.
    let mut converted = Vec::new();
    for index in 0..count as usize {
        let layer = unsafe { *layers.add(index) };
        if layer.is_null() { continue; }
        let header = unsafe { &*layer.cast::<LegacyLayerHeader>() };
        if !matches!(header.layer_type, 1 | 2) { continue; }
        let old = unsafe { &*layer.cast::<LegacyLayerEyeFov>() };
        converted.push(old.to_current());
    }
    let pointers: Vec<*const core::ffi::c_void> = converted.iter().map(|l| (l as *const OvrLayerEyeFov).cast()).collect();
    begin_legacy_frame(session, frame);
    unsafe { ovr_EndFrame(session, frame, scale, pointers.as_ptr(), pointers.len() as u32) }
}

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetTrackingStateWithSensorData(session: OvrSession, time: f64, latency: u8, _sensor: *mut core::ffi::c_void) -> OvrTrackingState { ovr_GetTrackingState(session, time, latency) }

#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetBool(_session: OvrSession, _name: *const core::ffi::c_char, default: u8) -> u8 { default }
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetInt(_session: OvrSession, _name: *const core::ffi::c_char, default: i32) -> i32 { default }
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetFloat(_session: OvrSession, _name: *const core::ffi::c_char, default: f32) -> f32 { default }
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetFloatArray(_session: OvrSession, _name: *const core::ffi::c_char, _values: *mut f32, _capacity: u32) -> u32 { 0 }
#[unsafe(no_mangle)]
pub extern "system" fn ovr_GetString(_session: OvrSession, _name: *const core::ffi::c_char, default: *const core::ffi::c_char) -> *const core::ffi::c_char { default }

/// Oculus texture enums are not DXGI enum values. Depth resources are typeless
/// because the game creates both depth-stencil and shader-resource views.
fn d3d11_texture_format(format: i32) -> Option<u32> {
    Some(match format {
        4 => 28, 5 => 29, 6 => 87, 7 => 91, 8 => 88, 9 => 93,
        10 => 10, 25 => 26, // RGBA16F and R11G11B10F (Lone Echo I HDR)
        11 => 53, 12 => 44, 13 => 39, 14 => 19,
        _ => return None,
    })
}
fn d3d11_bind_flags(flags: u32) -> u32 {
    0x8 // Every LibOVR texture is shader-readable.
        | if flags & 1 != 0 { 0x20 } else { 0 }
        | if flags & 2 != 0 { 0x80 } else { 0 }
        | if flags & 4 != 0 { 0x40 } else { 0 }
}

#[cfg(test)]
mod d3d11_tests {
    use super::*;
    #[test]
    fn legacy_hdr_and_depth_resources_match_d3d11() {
        assert_eq!(d3d11_texture_format(25), Some(26));
        assert_eq!(d3d11_texture_format(12), Some(44));
        assert_eq!(d3d11_texture_format(0), None);
        assert_eq!(d3d11_bind_flags(1), 0x28);
        assert_eq!(d3d11_bind_flags(2), 0x88);
        assert_eq!(d3d11_bind_flags(4), 0x48);
    }

    #[test]
    fn d3d12_depth_abi_and_flags() {
        assert_eq!(core::mem::size_of::<D3d12ClearValue>(), 20);
        assert_eq!(core::mem::size_of::<D3d12ResourceDesc>(), 56);
        assert_eq!(d3d12_resource_flags(4), 2); // Depth attachment must be allowed.
        assert_eq!(d3d12_resource_flags(1), 1);
        assert_eq!(d3d12_resource_flags(2), 4);
    }

    #[test]
    fn graphics_errors_reach_legacy_error_dialogs() {
        let message = "D3D12 CreateCommittedResource failed hr=0x887a0005";
        let code = graphics_error(message.into());
        let mut info = OvrErrorInfo { result: 0, error_string: [0; 512] };
        unsafe { ovr_GetLastErrorInfo(&mut info) };
        assert_eq!(info.result, code);
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(info.error_string.as_ptr()) }.to_str().unwrap(), message);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LegacyLayerHeader { layer_type: i32, flags: u32 }
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LegacyLayerEyeFov {
    header: LegacyLayerHeader,
    color_texture: [OvrTextureSwapChain; 2],
    viewport: [crate::abi::OvrRecti; 2],
    fov: [OvrFovPort; 2],
    render_pose: [crate::abi::OvrPosef; 2],
    sensor_sample_time: f64,
}
impl LegacyLayerEyeFov {
    fn to_current(&self) -> OvrLayerEyeFov {
        OvrLayerEyeFov {
            header: OvrLayerHeader { layer_type: self.header.layer_type, flags: self.header.flags, ..Default::default() },
            color_texture: self.color_texture, viewport: self.viewport, fov: self.fov,
            render_pose: self.render_pose, sensor_sample_time: self.sensor_sample_time,
        }
    }
}
#[cfg(test)]
mod legacy_layer_tests {
    use super::*;
    #[test]
    fn old_layer_keeps_eye_rectangles_and_fovs() {
        assert_eq!(std::mem::size_of::<LegacyLayerHeader>(), 8);
        assert_eq!(std::mem::offset_of!(LegacyLayerEyeFov, color_texture), 8);
        assert_eq!(std::mem::size_of::<LegacyLayerEyeFov>(), 152);
        let mut old = LegacyLayerEyeFov::default();
        old.header.layer_type = 1;
        old.viewport[0].size.w = 2000;
        old.viewport[1].pos.x = 2000;
        old.fov[0].left_tan = 1.2;
        old.sensor_sample_time = 12.5;
        let new = old.to_current();
        assert_eq!(new.viewport[0].size.w, 2000);
        assert_eq!(new.viewport[1].pos.x, 2000);
        assert_eq!(new.fov[0].left_tan, 1.2);
        assert_eq!(new.sensor_sample_time, 12.5);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn ovr_DestroyTextureSwapChain(_session: OvrSession, handle: OvrTextureSwapChain) {
    log_call("ovr_DestroyTextureSwapChain");
    let chain = SWAP_CHAINS.lock().ok().and_then(|mut chains| {
        let index = chains.iter().position(|c| std::ptr::eq(&**c, handle.cast()))?;
        Some(chains.remove(index))
    });
    if let Some(chain) = chain {
        if chain.openxr_color {
            #[cfg(windows)]
            if let Ok(mut slot) = XR_SESSION.lock() {
                if let Some(session) = slot.as_mut() {
                    if let Err(e) = session.retire_color_swapchain() { log_call(&format!("OpenXR retire swapchain failed: {e}")); }
                }
            }
        } else {
            for texture in chain.textures {
                if texture != 0 {
                    let object = texture as *mut core::ffi::c_void;
                    let vtable = unsafe { *(object as *const *const *const core::ffi::c_void) };
                    let release: Release = unsafe { core::mem::transmute(*vtable.add(2)) };
                    unsafe { release(object) };
                }
            }
        }
    }
}

fn input_state_size(minor: u32) -> usize {
    if minor < 7 { 56 } else if minor < 11 { 88 } else { core::mem::size_of::<OvrInputState>() }
}
unsafe fn write_input_state(destination: *mut u8, value: &OvrInputState, minor: u32) {
    unsafe { core::ptr::copy_nonoverlapping((value as *const OvrInputState).cast::<u8>(), destination, input_state_size(minor)); }
}
unsafe fn write_session_status(destination: *mut u8, value: &OvrSessionStatus, minor: u32) {
    let size = if minor <= 20 { 8 } else { core::mem::size_of::<OvrSessionStatus>() };
    unsafe { core::ptr::copy_nonoverlapping((value as *const OvrSessionStatus).cast::<u8>(), destination, size); }
}
#[cfg(test)]
mod client_version_tests {
    use super::*;
    #[test]
    fn legacy_clients_receive_virtual_sensors() {
        for (minor, count) in [(12, 3), (36, 3), (37, 0), (94, 0)] {
            assert_eq!(tracker_count(minor), count);
        }
        assert_eq!(compatible_hmd_type(12), 14);
        assert_eq!(compatible_hmd_type(37), 14);
        assert_eq!(compatible_hmd_type(38), 16);
        assert_eq!(compatible_hmd_type(94), 16);
    }
    #[test]
    fn input_writes_preserve_bytes_beyond_the_callers_version() {
        for (minor, length) in [(6,56),(10,88),(12,120),(94,120)] {
            let mut bytes = [0xa5u8; 136];
            let value = OvrInputState { buttons: 1, index_trigger: [0.0,1.0], ..Default::default() };
            unsafe { write_input_state(bytes.as_mut_ptr(), &value, minor); }
            assert_eq!(u32::from_ne_bytes(bytes[8..12].try_into().unwrap()),1);
            assert!(bytes[length..].iter().all(|b| *b==0xa5));
        }
    }
    #[test]
    fn legacy_session_status_preserves_the_next_field() {
        let mut bytes = [0xa5u8; 16];
        let value = OvrSessionStatus { has_input_focus: 1, ..Default::default() };
        unsafe { write_session_status(bytes.as_mut_ptr(), &value, 12); }
        assert_eq!(bytes[6], 1);
        assert!(bytes[8..].iter().all(|b| *b==0xa5));
    }
}
