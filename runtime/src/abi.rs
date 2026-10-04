//! Audited-in-stages C ABI definitions for the LibOVR CAPI 1.94 bootstrap.
//!
//! Only structures used by the bootstrap exports belong here. Every added type
//! needs a layout test against the official CAPI headers before it is relied on
//! for rendering or input.

use core::ffi::{c_char, c_void};

pub type OvrResult = i32;
pub type OvrBool = u8;
pub type OvrSession = *mut c_void;

pub const OVR_SUCCESS: OvrResult = 0;

/// Public x64 CAPI initialization parameters; RequestVersion selects the ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrInitParams {
    pub flags: u32,
    pub requested_minor_version: u32,
    pub log_callback: *const c_void,
    pub user_data: usize,
    pub connection_timeout_ms: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrGraphicsLuid {
    pub reserved: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OvrErrorInfo {
    pub result: OvrResult,
    pub error_string: [c_char; 512],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrSessionStatus {
    pub is_visible: OvrBool,
    pub hmd_present: OvrBool,
    pub hmd_mounted: OvrBool,
    pub display_lost: OvrBool,
    pub should_quit: OvrBool,
    pub should_recenter: OvrBool,
    pub has_input_focus: OvrBool,
    pub overlay_present: OvrBool,
    pub depth_requested: OvrBool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrSizei {
    pub w: i32,
    pub h: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrFovPort {
    pub up_tan: f32,
    pub down_tan: f32,
    pub left_tan: f32,
    pub right_tan: f32,
}

pub type OvrEyeType = i32;
pub type OvrTextureSwapChain = *mut c_void;
pub const OVR_EYE_LEFT: OvrEyeType = 0;
pub const OVR_EYE_RIGHT: OvrEyeType = 1;
pub const OVR_AUDIO_MAX_DEVICE_STR_SIZE: usize = 128;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrVector2i {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrRecti {
    pub pos: OvrVector2i,
    pub size: OvrSizei,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrVector2f {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrVector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrQuatf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrPosef {
    pub orientation: OvrQuatf,
    pub position: OvrVector3f,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrPoseStatef {
    pub pose: OvrPosef,
    pub angular_velocity: OvrVector3f,
    pub linear_velocity: OvrVector3f,
    pub angular_acceleration: OvrVector3f,
    pub linear_acceleration: OvrVector3f,
    pub time_in_seconds: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrTrackingState {
    pub head_pose: OvrPoseStatef,
    pub status_flags: u32,
    pub hand_poses: [OvrPoseStatef; 2],
    pub hand_status_flags: [u32; 2],
    pub calibrated_origin: OvrPosef,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrHapticsBuffer {
    pub samples: *const core::ffi::c_void,
    pub samples_count: i32,
    pub submit_mode: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrInputState {
    pub time_in_seconds: f64,
    pub buttons: u32,
    pub touches: u32,
    pub index_trigger: [f32; 2],
    pub hand_trigger: [f32; 2],
    pub thumbstick: [OvrVector2f; 2],
    pub controller_type: u32,
    pub index_trigger_no_deadzone: [f32; 2],
    pub hand_trigger_no_deadzone: [f32; 2],
    pub thumbstick_no_deadzone: [OvrVector2f; 2],
    pub index_trigger_raw: [f32; 2],
    pub hand_trigger_raw: [f32; 2],
    pub thumbstick_raw: [OvrVector2f; 2],
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrTrackerPose {
    pub status_flags: u32,
    pub pose: OvrPosef,
    pub leveled_pose: OvrPosef,
    pub reserved: [u8; 4],
}

/// CAPI 1.94 `ovrTrackerDesc` returned by value from `ovr_GetTrackerDesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrTrackerDesc {
    pub frustum_hfov_in_radians: f32,
    pub frustum_vfov_in_radians: f32,
    pub frustum_near_z_in_meters: f32,
    pub frustum_far_z_in_meters: f32,
}

/// Prefix common to all LibOVR composition layers.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OvrLayerHeader {
    pub layer_type: i32,
    pub flags: u32,
    // CAPI reserves this full 128-byte header tail; omitting it shifts every
    // EyeFov/EyeFovDepth member and makes submitted viewport data appear zero.
    pub reserved: [c_char; 128],
}

impl Default for OvrLayerHeader {
    fn default() -> Self {
        Self {
            layer_type: 0,
            flags: 0,
            reserved: [0; 128],
        }
    }
}

/// CAPI 1.94 `ovrLayerEyeFov`, the stereo color layer submitted by Echo.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OvrLayerEyeFov {
    pub header: OvrLayerHeader,
    pub color_texture: [OvrTextureSwapChain; 2],
    pub viewport: [OvrRecti; 2],
    pub fov: [OvrFovPort; 2],
    pub render_pose: [OvrPosef; 2],
    pub sensor_sample_time: f64,
}

/// CAPI 1.94 `ovrEyeRenderDesc`; returned by value by `ovr_GetRenderDesc2`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrEyeRenderDesc {
    pub eye: OvrEyeType,
    pub fov: OvrFovPort,
    pub distorted_viewport: OvrRecti,
    pub pixels_per_tan_angle_at_center: OvrVector2f,
    /// Echo's `ovr_GetRenderDesc2` ABI uses the modern full eye transform.
    /// Returning only the obsolete three-float offset shifts these bytes into
    /// the quaternion field, which corrupts Echo's RenderPose calculation.
    pub hmd_to_eye_pose: OvrPosef,
}

/// CAPI 1.94 `ovrTextureSwapChainDesc`. DX texture creation only needs this
/// layout at this stage; D3D texture allocation is deliberately deferred.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OvrTextureSwapChainDesc {
    pub texture_type: i32,
    pub format: i32,
    pub array_size: i32,
    pub width: i32,
    pub height: i32,
    pub mip_levels: i32,
    pub sample_count: i32,
    pub static_image: OvrBool,
    pub misc_flags: u32,
    pub bind_flags: u32,
}

/// CAPI 1.94 `ovrHmdDesc`.  Its layout is required by `ovr_GetHmdDesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OvrHmdDesc {
    pub hmd_type: i32,
    pub padding0: i32,
    pub product_name: [c_char; 64],
    pub manufacturer: [c_char; 64],
    pub vendor_id: i16,
    pub product_id: i16,
    pub serial_number: [c_char; 24],
    pub firmware_major: i16,
    pub firmware_minor: i16,
    pub available_hmd_caps: u32,
    pub default_hmd_caps: u32,
    pub available_tracking_caps: u32,
    pub default_tracking_caps: u32,
    pub default_eye_fov: [OvrFovPort; 2],
    pub max_eye_fov: [OvrFovPort; 2],
    pub resolution: OvrSizei,
    pub display_refresh_rate: f32,
    pub padding1: i32,
}

pub type OvrVersionString = *const c_char;
