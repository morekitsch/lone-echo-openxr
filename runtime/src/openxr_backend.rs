//! Dynamic OpenXR-loader probing used before the CAPI frame bridge is enabled.
//!
//! This deliberately creates only an OpenXR instance. Graphics sessions and
//! swapchains remain owned by the upcoming D3D bridge.

#[cfg(windows)]
use crate::direct3d::{Direct3D, Binding};

use openxr::{ApplicationInfo, Entry, ExtensionSet};

/// Capabilities required for the Windows D3D11 CAPI bridge and for automated
/// Monado headless testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenXrCapabilities {
    pub d3d11: bool,
    pub d3d12: bool,
    pub monado_headless: bool,
}

/// Errors are strings because loader/runtime failures are operational details
/// intended for the shim trace, not CAPI result codes.

/// The OpenXR objects that must outlive every LibOVR frame and swapchain. Keeping
/// the waiter and stream together also enforces the OpenXR frame ordering.
#[cfg(windows)]
pub struct Direct3DSession {
    pub instance: openxr::Instance,
    pub system: openxr::SystemId,
    pub session: openxr::Session<Direct3D>,
    pub waiter: openxr::FrameWaiter,
    pub stream: openxr::FrameStream<Direct3D>,
    pub space: openxr::Space,
    pub local_base: openxr::Space,
    pub floor_base: Option<openxr::Space>,
    pub tracking_origin: i32,
    pub origin_offsets: [openxr::Posef; 2],
    pub tracking_time: Option<openxr::Time>,
    pub recenter_pending: bool,
    pub view_space: openxr::Space,
    pub head_pose: openxr::Posef,
    pub head_location_flags: openxr::SpaceLocationFlags,
    pub view_poses: [openxr::Posef; 2],
    pub view_fovs: [openxr::Fovf; 2],
    pub hand_poses: [openxr::Posef; 2],
    pub hand_location_flags: [openxr::SpaceLocationFlags; 2],
    pub input_action_set: openxr::ActionSet,
    pub select_action: openxr::Action<bool>,
    pub primary_action: openxr::Action<bool>,
    pub secondary_action: openxr::Action<bool>,
    pub menu_action: openxr::Action<bool>,
    pub thumbstick_click_action: openxr::Action<bool>,
    pub primary_touch_action: openxr::Action<bool>,
    pub secondary_touch_action: openxr::Action<bool>,
    pub thumbstick_touch_action: openxr::Action<bool>,
    pub trigger_touch_action: openxr::Action<bool>,
    pub haptic_action: openxr::Action<openxr::Haptic>,
    pub trigger_action: openxr::Action<f32>,
    pub squeeze_action: openxr::Action<f32>,
    pub thumbstick_action: openxr::Action<openxr::Vector2f>,
    pub hand_spaces: [openxr::Space; 2],
    pub hand_select: [bool; 2],
    pub hand_primary: [bool; 2],
    pub hand_secondary: [bool; 2],
    pub hand_menu: [bool; 2],
    pub hand_thumbstick_click: [bool; 2],
    pub hand_primary_touch: [bool; 2],
    pub hand_secondary_touch: [bool; 2],
    pub hand_thumbstick_touch: [bool; 2],
    pub hand_trigger_touch: [bool; 2],
    pub hand_trigger: [f32; 2],
    pub hand_squeeze: [f32; 2],
    pub hand_thumbstick: [openxr::Vector2f; 2],
    pub last_logged_hand_poses: [openxr::Posef; 2],
    pub last_logged_head_pose: openxr::Posef,
    pub last_logged_view_poses: [openxr::Posef; 2],
    pub active_profiles: [Option<openxr::Path>; 2],
    /// Velocities supplied by xrLocateSpace in the tracking reference space.
    pub hand_linear_velocity: [openxr::Vector3f; 2],
    pub hand_angular_velocity: [openxr::Vector3f; 2],
    pub running: bool,
    pub frame_state: Option<openxr::FrameState>,
    /// Views located for the current frame. Reuse these at submission so the
    /// game and compositor receive poses for the same predicted display time.
    pub frame_views: Option<Vec<openxr::View>>,
    /// Avoid filesystem I/O on every frame after capturing valid real views.
    pub hmd_cache_written: bool,
    pub frame_begun: bool,
    pub color_swapchain: Option<openxr::Swapchain<Direct3D>>,
    pub color_image: Option<u32>,
    pub color_extent: Option<(u32, u32)>,
    pub d3d12_queue: Option<windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue>,
    pub d3d12_color_states: Option<crate::d3d12_states::ColorStates>,
    /// Raw eye-space masks, cached until the runtime signals a change.
    pub visibility_masks: [[Option<openxr::VisibilityMask>; 3]; 2],
}

#[cfg(windows)]
impl core::fmt::Debug for Direct3DSession {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("Direct3DSession")
    }
}

/// Discover the adapter selected by the active OpenXR runtime before Echo
/// creates its D3D12 queue. LibOVR exposes these bytes from `ovr_Create`.
#[cfg(windows)]
pub fn adapter_luid() -> Result<[u8; 8], String> {
    crate::capi::log_call("OpenXR loading standard loader");
    let entry = unsafe { Entry::load() }.map_err(|error| error.to_string())?;
    crate::capi::log_call("OpenXR loader loaded; enumerating extensions");
    let extensions = entry
        .enumerate_extensions()
        .map_err(|error| error.to_string())?;
    let d3d11 = std::env::var("LONE_ECHO_RENDERER").as_deref() == Ok("d3d11");
    if (d3d11 && !extensions.khr_d3d11_enable) || (!d3d11 && !extensions.khr_d3d12_enable) {
        return Err("XR_KHR_d3d12_enable unavailable".into());
    }
    let mut requested = ExtensionSet::default();
    requested.khr_d3d12_enable = !d3d11;
    requested.khr_d3d11_enable = d3d11;
    requested.fb_display_refresh_rate = extensions.fb_display_refresh_rate;
    let instance = entry
        .create_instance(
            &ApplicationInfo {
                application_name: "libovr-openxr",
                application_version: 1,
                engine_name: "Echo VR",
                engine_version: 1,
                api_version: openxr::Version::new(1, 0, 0),
            },
            &requested,
            &[],
        )
        .map_err(|error| error.to_string())?;
    let system = instance
        .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|error| error.to_string())?;
    if let Ok(views) = instance
        .enumerate_view_configuration_views(system, openxr::ViewConfigurationType::PRIMARY_STEREO)
    {
        if let Some(view) = views.first() {
            crate::capi::set_openxr_eye_resolution(
                view.recommended_image_rect_width as i32,
                view.recommended_image_rect_height as i32,
            );
        }
    }
    let requirements = instance
        .graphics_requirements::<Direct3D>(system)
        .map_err(|error| error.to_string())?;
    // Do not create a pre-game session: cached data is used at startup, then
    // refreshed only by the retained real D3D12 session below.
    // Windows LUID is an 8-byte C structure; LibOVR uses the same representation.
    Ok(unsafe { core::mem::transmute(requirements.adapter_luid) })
}

/// Create the retained OpenXR session used by the LibOVR frame bridge.
#[cfg(windows)]
pub unsafe fn create_direct3d_session(
    device: *mut core::ffi::c_void,
    queue: *mut core::ffi::c_void,
) -> Result<Direct3DSession, String> {
    crate::capi::log_call("OpenXR loading standard loader");
    let entry = unsafe { Entry::load() }.map_err(|error| error.to_string())?;
    crate::capi::log_call("OpenXR loader loaded; enumerating extensions");
    let extensions = entry
        .enumerate_extensions()
        .map_err(|error| error.to_string())?;
    let d3d11 = queue.is_null();
    if (d3d11 && !extensions.khr_d3d11_enable) || (!d3d11 && !extensions.khr_d3d12_enable) {
        return Err("XR_KHR_d3d12_enable unavailable".into());
    }
    let mut requested = ExtensionSet::default();
    requested.khr_d3d12_enable = !d3d11;
    requested.khr_d3d11_enable = d3d11;
    requested.fb_display_refresh_rate = extensions.fb_display_refresh_rate;
    requested.khr_visibility_mask = extensions.khr_visibility_mask;
    // Required for translating LibOVR's current-time API to OpenXR's clock.
    // Current Wine/Proton OpenXR implementations expose this extension.
    requested.khr_win32_convert_performance_counter_time = true;
    let instance = entry
        .create_instance(
            &ApplicationInfo {
                application_name: "libovr-openxr",
                application_version: 1,
                engine_name: "Echo VR",
                engine_version: 1,
                api_version: openxr::Version::new(1, 0, 0),
            },
            &requested,
            &[],
        )
        .map_err(|error| error.to_string())?;
    let system = instance
        .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|error| error.to_string())?;
    instance.graphics_requirements::<Direct3D>(system).map_err(|e| e.to_string())?;
    let info = Binding {
        device: device.cast(),
        queue: queue.cast(),
    };
    let (session, waiter, stream) =
        unsafe { instance.create_session::<Direct3D>(system, &info) }
            .map_err(|error| error.to_string())?;
    if extensions.fb_display_refresh_rate {
        if let Ok(rate) = session.get_display_refresh_rate() {
            crate::capi::set_openxr_refresh_rate(rate);
        }
    }
    let identity = openxr::Posef {
        orientation: openxr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        position: openxr::Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    };
    let space = session
        .create_reference_space(openxr::ReferenceSpaceType::LOCAL, identity)
        .map_err(|error| error.to_string())?;
    let view_space = session
        .create_reference_space(openxr::ReferenceSpaceType::VIEW, identity)
        .map_err(|error| error.to_string())?;
    let local_base = session.create_reference_space(openxr::ReferenceSpaceType::LOCAL, identity).map_err(|e| e.to_string())?;
    let floor_base = session.create_reference_space(openxr::ReferenceSpaceType::STAGE, identity).ok();
    let input_action_set = instance
        .create_action_set("echovr_input", "Echo VR controller poses", 0)
        .map_err(|error| error.to_string())?;
    let left_path = instance
        .string_to_path("/user/hand/left")
        .map_err(|error| error.to_string())?;
    let right_path = instance
        .string_to_path("/user/hand/right")
        .map_err(|error| error.to_string())?;
    // Explicit subaction paths prevent the runtime from resolving both action
    // spaces to its default (left) controller source.
    let hand_action = input_action_set
        .create_action::<openxr::Posef>("hand_pose", "Hand pose", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let select_action = input_action_set
        .create_action::<bool>("select", "Select", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let primary_action = input_action_set
        .create_action::<bool>("primary", "Primary", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let secondary_action = input_action_set
        .create_action::<bool>("secondary", "Secondary", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let menu_action = input_action_set
        .create_action::<bool>("menu", "Menu", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let thumbstick_click_action = input_action_set
        .create_action::<bool>(
            "thumbstick_click",
            "Thumbstick click",
            &[left_path, right_path],
        )
        .map_err(|error| error.to_string())?;
    let primary_touch_action = input_action_set
        .create_action::<bool>("primary_touch", "Primary touch", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let secondary_touch_action = input_action_set
        .create_action::<bool>(
            "secondary_touch",
            "Secondary touch",
            &[left_path, right_path],
        )
        .map_err(|error| error.to_string())?;
    let thumbstick_touch_action = input_action_set
        .create_action::<bool>(
            "thumbstick_touch",
            "Thumbstick touch",
            &[left_path, right_path],
        )
        .map_err(|error| error.to_string())?;
    let trigger_touch_action = input_action_set
        .create_action::<bool>("trigger_touch", "Trigger touch", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let haptic_action = input_action_set
        .create_action::<openxr::Haptic>("haptic", "Haptic output", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let trigger_action = input_action_set
        .create_action::<f32>("trigger", "Trigger", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let squeeze_action = input_action_set
        .create_action::<f32>("squeeze", "Squeeze", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    let thumbstick_action = input_action_set
        .create_action::<openxr::Vector2f>("thumbstick", "Thumbstick", &[left_path, right_path])
        .map_err(|error| error.to_string())?;
    instance
        .suggest_interaction_profile_bindings(
            instance
                .string_to_path("/interaction_profiles/khr/simple_controller")
                .map_err(|error| error.to_string())?,
            &[
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &select_action,
                    instance
                        .string_to_path("/user/hand/left/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &select_action,
                    instance
                        .string_to_path("/user/hand/right/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
            ],
        )
        .map_err(|error| error.to_string())?;
    instance
        .suggest_interaction_profile_bindings(
            instance
                .string_to_path("/interaction_profiles/oculus/touch_controller")
                .map_err(|error| error.to_string())?,
            &[
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/left/input/x/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/right/input/a/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_action,
                    instance
                        .string_to_path("/user/hand/left/input/y/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_action,
                    instance
                        .string_to_path("/user/hand/right/input/b/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &menu_action,
                    instance
                        .string_to_path("/user/hand/left/input/menu/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_touch_action,
                    instance
                        .string_to_path("/user/hand/left/input/x/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_touch_action,
                    instance
                        .string_to_path("/user/hand/right/input/a/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_touch_action,
                    instance
                        .string_to_path("/user/hand/left/input/y/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_touch_action,
                    instance
                        .string_to_path("/user/hand/right/input/b/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_touch_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_touch_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_touch_action,
                    instance
                        .string_to_path("/user/hand/left/input/trigger/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_touch_action,
                    instance
                        .string_to_path("/user/hand/right/input/trigger/touch")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/left/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/right/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/left/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/right/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &haptic_action,
                    instance
                        .string_to_path("/user/hand/left/output/haptic")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &haptic_action,
                    instance
                        .string_to_path("/user/hand/right/output/haptic")
                        .map_err(|error| error.to_string())?,
                ),
            ],
        )
        .map_err(|error| error.to_string())?;
    // Non-Oculus OpenXR controllers use the same abstract LibOVR Touch
    // semantics.  These cover the controller profiles commonly exposed by
    // SteamVR, WMR, and native OpenXR runtimes; unsupported suggestions are
    // harmless because profile selection happens at runtime.
    for (profile, bindings) in [
        (
            "/interaction_profiles/valve/index_controller",
            vec![
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/left/input/a/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/right/input/a/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_action,
                    instance
                        .string_to_path("/user/hand/left/input/b/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &secondary_action,
                    instance
                        .string_to_path("/user/hand/right/input/b/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &menu_action,
                    instance
                        .string_to_path("/user/hand/left/input/system/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/left/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/right/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/left/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/right/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
            ],
        ),
        (
            "/interaction_profiles/microsoft/motion_controller",
            vec![
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/left/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/right/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &menu_action,
                    instance
                        .string_to_path("/user/hand/left/input/menu/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/left/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/right/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/left/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &squeeze_action,
                    instance
                        .string_to_path("/user/hand/right/input/squeeze/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/left/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/right/input/thumbstick/click")
                        .map_err(|error| error.to_string())?,
                ),
            ],
        ),
        (
            "/interaction_profiles/htc/vive_controller",
            vec![
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/left/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &hand_action,
                    instance
                        .string_to_path("/user/hand/right/input/aim/pose")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/left/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &primary_action,
                    instance
                        .string_to_path("/user/hand/right/input/select/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &menu_action,
                    instance
                        .string_to_path("/user/hand/left/input/menu/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/left/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &trigger_action,
                    instance
                        .string_to_path("/user/hand/right/input/trigger/value")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/left/input/trackpad")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_action,
                    instance
                        .string_to_path("/user/hand/right/input/trackpad")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/left/input/trackpad/click")
                        .map_err(|error| error.to_string())?,
                ),
                openxr::Binding::new(
                    &thumbstick_click_action,
                    instance
                        .string_to_path("/user/hand/right/input/trackpad/click")
                        .map_err(|error| error.to_string())?,
                ),
            ],
        ),
    ] {
        if let Err(error) = instance.suggest_interaction_profile_bindings(
            instance
                .string_to_path(profile)
                .map_err(|error| error.to_string())?,
            &bindings,
        ) {
            // Optional profile mismatches must not prevent the base Touch or
            // simple-controller session from starting.
            crate::capi::log_call(&format!(
                "OpenXR optional interaction profile {profile} unavailable: {error}"
            ));
        }
    }
    session
        .attach_action_sets(&[&input_action_set])
        .map_err(|error| error.to_string())?;
    let hand_spaces = [
        hand_action
            .create_space(&session, left_path, identity)
            .map_err(|error| error.to_string())?,
        hand_action
            .create_space(&session, right_path, identity)
            .map_err(|error| error.to_string())?,
    ];
    Ok(Direct3DSession {
        visibility_masks: std::array::from_fn(|_| std::array::from_fn(|_| None)),
        instance,
        system,
        session,
        waiter,
        stream,
        space,
        view_space,
        local_base,
        floor_base,
        tracking_origin: 0,
        origin_offsets: [identity; 2],
        tracking_time: None,
        recenter_pending: true,
        head_pose: identity,
        head_location_flags: openxr::SpaceLocationFlags::EMPTY,
        view_poses: [identity; 2],
        view_fovs: [openxr::Fovf {
            angle_left: -std::f32::consts::FRAC_PI_4,
            angle_right: std::f32::consts::FRAC_PI_4,
            angle_up: std::f32::consts::FRAC_PI_4,
            angle_down: -std::f32::consts::FRAC_PI_4,
        }; 2],
        hand_poses: [identity; 2],
        hand_location_flags: [openxr::SpaceLocationFlags::EMPTY; 2],
        input_action_set,
        select_action,
        primary_action,
        secondary_action,
        menu_action,
        thumbstick_click_action,
        primary_touch_action,
        secondary_touch_action,
        thumbstick_touch_action,
        trigger_touch_action,
        haptic_action,
        trigger_action,
        squeeze_action,
        thumbstick_action,
        hand_spaces,
        hand_select: [false; 2],
        hand_primary: [false; 2],
        hand_secondary: [false; 2],
        hand_menu: [false; 2],
        hand_thumbstick_click: [false; 2],
        hand_primary_touch: [false; 2],
        hand_secondary_touch: [false; 2],
        hand_thumbstick_touch: [false; 2],
        hand_trigger_touch: [false; 2],
        hand_trigger: [0.0; 2],
        hand_squeeze: [0.0; 2],
        hand_thumbstick: [openxr::Vector2f { x: 0.0, y: 0.0 }; 2],
        last_logged_hand_poses: [identity; 2],
        last_logged_head_pose: identity,
        last_logged_view_poses: [identity; 2],
        active_profiles: [None; 2],
        hand_linear_velocity: [openxr::Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }; 2],
        hand_angular_velocity: [openxr::Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }; 2],
        running: false,
        frame_state: None,
        frame_views: None,
        hmd_cache_written: false,
        frame_begun: false,
        color_swapchain: None,
        d3d12_queue: {
            use windows::core::Interface;
            unsafe { windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned()
        },
        d3d12_color_states: None,
        color_image: None,
        color_extent: None,
    })
}

#[cfg(windows)]
fn valid_orientation(pose: openxr::Posef) -> bool {
    let q = pose.orientation;
    let norm = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    norm.is_finite() && norm > 0.5 && norm < 1.5
}

#[cfg(windows)]
pub fn hmd_to_eye_offset(head: openxr::Posef, eye: openxr::Posef) -> openxr::Vector3f {
    // Transform the LOCAL-space eye displacement into HMD/view space. LibOVR
    // requires this relative offset, not the absolute LOCAL eye position.
    let dx = eye.position.x - head.position.x;
    let dy = eye.position.y - head.position.y;
    let dz = eye.position.z - head.position.z;
    let q = head.orientation;
    // conjugate(q) * displacement * q
    let ix = q.w * dx - q.y * dz + q.z * dy;
    let iy = q.w * dy - q.z * dx + q.x * dz;
    let iz = q.w * dz - q.x * dy + q.y * dx;
    let iw = q.x * dx + q.y * dy + q.z * dz;
    openxr::Vector3f {
        x: ix * q.w + iw * q.x + iy * q.z - iz * q.y,
        y: iy * q.w + iw * q.y + iz * q.x - ix * q.z,
        z: iz * q.w + iw * q.z + ix * q.y - iy * q.x,
    }
}

#[cfg(windows)]
fn stable_pose(previous: openxr::Posef, location: openxr::SpaceLocation) -> openxr::Posef {
    let mut pose = previous;
    if location
        .location_flags
        .contains(openxr::SpaceLocationFlags::ORIENTATION_VALID)
        && valid_orientation(location.pose)
    {
        pose.orientation = location.pose.orientation;
    }
    if location
        .location_flags
        .contains(openxr::SpaceLocationFlags::POSITION_VALID)
        && location.pose.position.x.is_finite()
        && location.pose.position.y.is_finite()
        && location.pose.position.z.is_finite()
    {
        pose.position = location.pose.position;
    }
    // `previous` begins as identity, but retain this final guard if a future
    // runtime ever returns NaN or an invalid cached quaternion.
    if !valid_orientation(pose) {
        pose.orientation = openxr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
    }
    pose
}

#[cfg(windows)]
impl Direct3DSession {
    /// Return the current time in the same OpenXR clock domain used by frame
    /// predictions and action sampling.
    pub fn openxr_time_seconds(&self) -> f64 {
        self.instance
            .now()
            .expect("XR_KHR_win32_convert_performance_counter_time was requested")
            .as_nanos() as f64
            / 1_000_000_000.0
    }

    fn cache_live_hmd_data(&mut self, views: &[openxr::View]) {
        if self.hmd_cache_written || views.len() < 2 {
            return;
        }
        let required_flags = openxr::SpaceLocationFlags::ORIENTATION_VALID
            | openxr::SpaceLocationFlags::POSITION_VALID;
        if !self.head_location_flags.contains(required_flags) {
            return;
        }
        let [left, right, ..] = views else {
            return;
        };
        if !valid_orientation(left.pose)
            || !valid_orientation(right.pose)
            || ![left.pose.position, right.pose.position]
                .iter()
                .all(|position| {
                    position.x.is_finite() && position.y.is_finite() && position.z.is_finite()
                })
        {
            return;
        }
        let Some(view_config) = self
            .instance
            .enumerate_view_configuration_views(
                self.system,
                openxr::ViewConfigurationType::PRIMARY_STEREO,
            )
            .ok()
            .and_then(|views| views.into_iter().next())
        else {
            return;
        };
        let refresh_rate = self.session.get_display_refresh_rate().unwrap_or(90.0);
        let convert_fov = |fov: openxr::Fovf| crate::abi::OvrFovPort {
            up_tan: fov.angle_up.tan(),
            down_tan: -fov.angle_down.tan(),
            left_tan: -fov.angle_left.tan(),
            right_tan: fov.angle_right.tan(),
        };
        let convert_offset = |pose: openxr::Posef| {
            let offset = hmd_to_eye_offset(self.head_pose, pose);
            crate::abi::OvrVector3f {
                x: offset.x,
                y: offset.y,
                z: offset.z,
            }
        };
        let Some(cache) = crate::hmd_cache::HmdCache::new(
            [convert_fov(left.fov), convert_fov(right.fov)],
            [convert_offset(left.pose), convert_offset(right.pose)],
            view_config.recommended_image_rect_width as i32,
            view_config.recommended_image_rect_height as i32,
            refresh_rate,
        ) else {
            return;
        };
        self.hmd_cache_written = true;
        match crate::hmd_cache::save(cache) {
            Ok(()) => crate::capi::log_call("OpenXR HMD cache saved from live D3D12 views"),
            Err(error) => crate::capi::log_call(&format!("OpenXR HMD cache save failed: {error}")),
        }
    }

    fn change_tracking_space(&mut self, origin: i32, offset: openxr::Posef) -> Result<(), String> {
        if origin == 1 && self.floor_base.is_none() { return Err("OpenXR floor tracking space unavailable".into()); }
        let ty = if origin == 0 { openxr::ReferenceSpaceType::LOCAL } else { openxr::ReferenceSpaceType::STAGE };
        let new_space = self.session.create_reference_space(ty, offset).map_err(|e| e.to_string())?;
        if let Some(time) = self.tracking_time {
            let relative = self.space.locate(&new_space, time).map_err(|e| e.to_string())?;
            let valid = openxr::SpaceLocationFlags::POSITION_VALID | openxr::SpaceLocationFlags::ORIENTATION_VALID;
            if !relative.location_flags.contains(valid) { return Err("tracking origin transform unavailable".into()); }
            // Calls can occur between GetTrackingState and EndFrame. Rebase all
            // cached data so the game and compositor never mix coordinate frames.
            let transform = relative.pose;
            use crate::tracking_origin::{compose, rotate};
            self.head_pose = compose(transform, self.head_pose);
            for eye in &mut self.view_poses { *eye = compose(transform, *eye); }
            if let Some(views) = self.frame_views.as_mut() {
                for view in views { view.pose = compose(transform, view.pose); }
            }
            for i in 0..2 {
                self.hand_poses[i] = compose(transform, self.hand_poses[i]);
                self.hand_linear_velocity[i] = rotate(transform.orientation, self.hand_linear_velocity[i]);
                self.hand_angular_velocity[i] = rotate(transform.orientation, self.hand_angular_velocity[i]);
            }
        }
        self.space = new_space;
        self.tracking_origin = origin;
        self.origin_offsets[origin as usize] = offset;
        crate::capi::log_call(&format!("OpenXR tracking origin={} offset={offset:?}", if origin == 0 { "eye" } else { "floor" }));
        Ok(())
    }

    pub fn set_tracking_origin(&mut self, origin: i32) -> Result<(), String> {
        if !(0..=1).contains(&origin) { return Err("invalid tracking origin".into()); }
        if origin != self.tracking_origin { self.change_tracking_space(origin, self.origin_offsets[origin as usize])?; }
        Ok(())
    }

    fn recenter_at(&mut self, time: openxr::Time) -> Result<(), String> {
        let base = if self.tracking_origin == 0 { &self.local_base } else { self.floor_base.as_ref().ok_or("floor space unavailable")? };
        let location = self.view_space.locate(base, time).map_err(|e| e.to_string())?;
        let valid = openxr::SpaceLocationFlags::POSITION_VALID | openxr::SpaceLocationFlags::ORIENTATION_VALID;
        if !location.location_flags.contains(valid) { return Err("head pose unavailable for recentering".into()); }
        let offset = crate::tracking_origin::recentered_origin(location.pose, self.tracking_origin == 1)?;
        self.change_tracking_space(self.tracking_origin, offset)?;
        self.recenter_pending = false;
        Ok(())
    }

    pub fn recenter_tracking(&mut self) -> Result<(), String> {
        self.recenter_pending = true;
        if let Some(time) = self.tracking_time { self.recenter_at(time)?; }
        Ok(())
    }

    pub fn specify_tracking_origin(&mut self, offset: openxr::Posef) -> Result<(), String> {
        let q = offset.orientation;
        let norm = q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
        if !norm.is_finite() || (norm-1.0).abs()>0.01 || ![offset.position.x,offset.position.y,offset.position.z].iter().all(|v| v.is_finite()) {
            return Err("invalid tracking-origin pose".into());
        }
        let next = crate::tracking_origin::compose(self.origin_offsets[self.tracking_origin as usize], offset);
        self.change_tracking_space(self.tracking_origin, next)?;
        self.recenter_pending = false;
        Ok(())
    }

    /// Drive OpenXR's session state machine from the LibOVR frame thread.
    pub fn poll_events(&mut self) -> Result<(), String> {
        let mut storage = openxr::EventDataBuffer::new();
        while let Some(event) = self
            .instance
            .poll_event(&mut storage)
            .map_err(|error| error.to_string())?
        {
            if let openxr::Event::SessionStateChanged(changed) = event {
                match changed.state() {
                    openxr::SessionState::READY if !self.running => {
                        self.session
                            .begin(openxr::ViewConfigurationType::PRIMARY_STEREO)
                            .map_err(|error| error.to_string())?;
                        self.running = true;
                    }
                    openxr::SessionState::STOPPING if self.running => {
                        self.session.end().map_err(|error| error.to_string())?;
                        self.running = false;
                    }
                    openxr::SessionState::EXITING | openxr::SessionState::LOSS_PENDING => {
                        self.running = false;
                        crate::capi::log_call(&format!(
                            "OpenXR session state {:?}; exiting game",
                            changed.state()
                        ));
                        // LibOVR has no equivalent asynchronous quit callback
                        // for this path. Returning success while merely
                        // stopping frame submission leaves the game alive in
                        // a broken state, so terminate the process explicitly.
                        std::process::exit(0);
                    }
                    _ => {}
                }
            } else if let openxr::Event::VisibilityMaskChangedKHR(changed) = event {
                if changed.session() == self.session.as_raw()
                    && changed.view_configuration_type() == openxr::ViewConfigurationType::PRIMARY_STEREO
                    && changed.view_index() < 2
                {
                    self.visibility_masks[changed.view_index() as usize] = std::array::from_fn(|_| None);
                }
            }
        }
        Ok(())
    }

    pub fn visibility_mask(&mut self, eye: usize, kind: usize) -> Result<&openxr::VisibilityMask, i32> {
        use crate::visibility_mask::UNSUPPORTED;
        let Some(extension) = self.instance.exts().khr_visibility_mask.as_ref() else { return Err(UNSUPPORTED); };
        if eye >= 2 || kind >= 3 { return Err(crate::visibility_mask::INVALID); }
        if self.visibility_masks[eye][kind].is_none() {
            let mask_type = [openxr::VisibilityMaskTypeKHR::HIDDEN_TRIANGLE_MESH,
                             openxr::VisibilityMaskTypeKHR::VISIBLE_TRIANGLE_MESH,
                             openxr::VisibilityMaskTypeKHR::LINE_LOOP][kind];
            let mask = crate::visibility_mask::fetch(|info| unsafe {
                (extension.get_visibility_mask)(self.session.as_raw(),
                    openxr::ViewConfigurationType::PRIMARY_STEREO, eye as u32, mask_type, info)
            })?;
            crate::capi::log_call(&format!("OpenXR visibility mask eye={eye} kind={kind} vertices={} indices={}", mask.vertices.len(), mask.indices.len()));
            self.visibility_masks[eye][kind] = Some(mask);
        }
        self.visibility_masks[eye][kind].as_ref().ok_or(UNSUPPORTED)
    }

    pub fn create_color_swapchain(
        &mut self,
        format: u32,
        width: u32,
        height: u32,
        array_size: u32,
        mip_count: u32,
        sample_count: u32,
        typed_views: bool,
    ) -> Result<Vec<usize>, String> {
        // LE1 recreates its render targets while a legacy frame is already open.
        // Finish that frame before replacing its acquired swapchain image.
        self.retire_color_swapchain()?;
        let supported = self.session.enumerate_swapchain_formats().map_err(|e| e.to_string())?;
        let requested_format = format;
        let format = if supported.contains(&format) { format } else {
            let alternatives: &[u32] = match format {
                26 => &[10, 28], // Packed HDR -> RGBA16F, then linear RGBA8.
                10 => &[28],
                88 => &[87],
                93 => &[91],
                _ => &[],
            };
            alternatives.iter().copied().find(|f| supported.contains(f))
                .ok_or_else(|| format!("DXGI format {format} unavailable; runtime supports {supported:?}"))?
        };
        crate::capi::log_call(&format!("OpenXR color format requested={requested_format} selected={format} supported={supported:?}"));
        let swapchain = self
            .session
            .create_swapchain(&openxr::SwapchainCreateInfo {
                create_flags: openxr::SwapchainCreateFlags::EMPTY,
                usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT | openxr::SwapchainUsageFlags::SAMPLED,
                format,
                sample_count,
                width,
                height,
                face_count: 1,
                array_size,
                mip_count,
            })
            .map_err(|error| error.to_string())?;
        let images = swapchain
            .enumerate_images()
            .map_err(|error| error.to_string())?;
        let raw_images: Vec<usize> = images.into_iter().map(|image| image as usize).collect();
        if typed_views {
            // Keep storage owned by OpenXR. Only repair LibOVR's typed-view
            // contract, including formats negotiated above.
            unsafe {
                if self.instance.exts().khr_d3d11_enable.is_some() {
                    crate::d3d11_views::prepare(&raw_images, requested_format, format)?;
                } else {
                    crate::d3d12_views::prepare(&raw_images, requested_format, format)?;
                }
            }
        }
        if let Some(queue) = &self.d3d12_queue {
            self.d3d12_color_states = Some(unsafe { crate::d3d12_states::ColorStates::new(queue, &raw_images)? });
        }
        self.color_swapchain = Some(swapchain);
        self.color_extent = Some((width, height));
        Ok(raw_images)
    }

    pub fn retire_color_swapchain(&mut self) -> Result<(), String> {
        if let Some(image) = self.color_image.take() {
            if let Some(states) = &self.d3d12_color_states { states.release(image); }
            if let Some(swapchain) = self.color_swapchain.as_mut() {
                swapchain.release_image().map_err(|e| e.to_string())?;
            }
        }
        if self.frame_begun {
            if let Some(state) = self.frame_state.take() {
                self.stream.end(state.predicted_display_time, openxr::EnvironmentBlendMode::OPAQUE, &[])
                    .map_err(|e| e.to_string())?;
            }
            self.frame_begun = false;
        }
        self.frame_views = None;
        if let Some(states) = &self.d3d12_color_states { states.finish()?; }
        self.d3d12_color_states = None;
        self.color_swapchain = None;
        self.color_extent = None;
        Ok(())
    }

    pub fn submit_haptic(
        &self,
        controller_type: u32,
        amplitude: f32,
        duration: std::time::Duration,
    ) -> Result<(), String> {
        let duration = openxr::Duration::try_from(duration).map_err(|error| error.to_string())?;
        let event = openxr::HapticVibration::new()
            .amplitude(amplitude.clamp(0.0, 1.0))
            .frequency(openxr::FREQUENCY_UNSPECIFIED)
            .duration(duration);
        for (bit, path) in [
            (0x0001_u32, "/user/hand/left"),
            (0x0002_u32, "/user/hand/right"),
        ] {
            if controller_type & bit != 0 {
                let path = self
                    .instance
                    .string_to_path(path)
                    .map_err(|error| error.to_string())?;
                self.haptic_action
                    .apply_feedback(&self.session, path, &event)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    pub fn wait_frame(&mut self) -> Result<(), String> {
        if self.running {
            self.frame_state = Some(self.waiter.wait().map_err(|error| error.to_string())?);
            let state = self.frame_state.expect("frame state set");
            crate::capi::log_call(&format!("OpenXR wait ready predicted_ns={} period_ns={} should_render={}",
                state.predicted_display_time.as_nanos(), state.predicted_display_period.as_nanos(), state.should_render));
            let display_time = self
                .frame_state
                .expect("frame state set")
                .predicted_display_time;
            if self.recenter_pending {
                // Retry during startup until position and orientation are tracked.
                // Never turn a temporary identity/untracked pose into a calibration.
                if let Ok(location) = self.view_space.locate(&self.space, display_time) {
                    let tracked = openxr::SpaceLocationFlags::POSITION_TRACKED | openxr::SpaceLocationFlags::ORIENTATION_TRACKED;
                    if location.location_flags.contains(tracked) { let _ = self.recenter_at(display_time); }
                }
            }
            self.tracking_time = Some(display_time);
            if let Ok(location) = self.view_space.locate(&self.space, display_time) {
                self.head_pose = stable_pose(self.head_pose, location);
                self.head_location_flags = location.location_flags;
                let previous = self.last_logged_head_pose;
                let pose = self.head_pose;
                if (pose.position.x - previous.position.x).abs() > 0.002
                    || (pose.position.y - previous.position.y).abs() > 0.002
                    || (pose.position.z - previous.position.z).abs() > 0.002
                    || (pose.orientation.x - previous.orientation.x).abs() > 0.002
                    || (pose.orientation.y - previous.orientation.y).abs() > 0.002
                    || (pose.orientation.z - previous.orientation.z).abs() > 0.002
                    || (pose.orientation.w - previous.orientation.w).abs() > 0.002
                {
                    crate::capi::log_call(&format!(
                        "OpenXR HMD flags={:?} pos=({:.3},{:.3},{:.3}) quat=({:.3},{:.3},{:.3},{:.3})",
                        location.location_flags,
                        pose.position.x,
                        pose.position.y,
                        pose.position.z,
                        pose.orientation.x,
                        pose.orientation.y,
                        pose.orientation.z,
                        pose.orientation.w,
                    ));
                    self.last_logged_head_pose = pose;
                }
            }
            if let Ok((_, views)) = self.session.locate_views(
                openxr::ViewConfigurationType::PRIMARY_STEREO,
                display_time,
                &self.space,
            ) {
                self.frame_views = Some(views);
                if let Some(views) = self.frame_views.as_ref() {
                    for (index, view) in views.iter().take(2).enumerate() {
                        if valid_orientation(view.pose) {
                            self.view_poses[index] = view.pose;
                            self.view_fovs[index] = view.fov;
                            let previous = self.last_logged_view_poses[index];
                            if (view.pose.position.x - previous.position.x).abs() > 0.002
                                || (view.pose.position.y - previous.position.y).abs() > 0.002
                                || (view.pose.position.z - previous.position.z).abs() > 0.002
                            {
                                let offset = hmd_to_eye_offset(self.head_pose, view.pose);
                                crate::capi::log_call(&format!(
                                    "OpenXR {} view pos=({:.3},{:.3},{:.3}) hmd_to_eye=({:.4},{:.4},{:.4})",
                                    if index == 0 { "left" } else { "right" },
                                    view.pose.position.x,
                                    view.pose.position.y,
                                    view.pose.position.z,
                                    offset.x,
                                    offset.y,
                                    offset.z,
                                ));
                                self.last_logged_view_poses[index] = view.pose;
                            }
                        }
                    }
                }
                if !self.hmd_cache_written {
                    if let Some(views) = self.frame_views.clone() {
                        self.cache_live_hmd_data(&views);
                    }
                }
            }
            self.session
                .sync_actions(&[openxr::ActiveActionSet::new(&self.input_action_set)])
                .map_err(|error| error.to_string())?;
            for (index, hand_space) in self.hand_spaces.iter().enumerate() {
                self.hand_linear_velocity[index] = openxr::Vector3f::default();
                self.hand_angular_velocity[index] = openxr::Vector3f::default();
                if let Ok((location, velocity)) = hand_space.relate(&self.space, display_time) {
                    self.hand_poses[index] = stable_pose(self.hand_poses[index], location);
                    self.hand_location_flags[index] = location.location_flags;
                    self.hand_linear_velocity[index] = if velocity.velocity_flags.contains(openxr::SpaceVelocityFlags::LINEAR_VALID) { velocity.linear_velocity } else { openxr::Vector3f::default() };
                    self.hand_angular_velocity[index] = if velocity.velocity_flags.contains(openxr::SpaceVelocityFlags::ANGULAR_VALID) { velocity.angular_velocity } else { openxr::Vector3f::default() };
                    let previous = self.last_logged_hand_poses[index];
                    let pose = location.pose;
                    let changed = (pose.position.x - previous.position.x).abs() > 0.002
                        || (pose.position.y - previous.position.y).abs() > 0.002
                        || (pose.position.z - previous.position.z).abs() > 0.002
                        || (pose.orientation.x - previous.orientation.x).abs() > 0.002
                        || (pose.orientation.y - previous.orientation.y).abs() > 0.002
                        || (pose.orientation.z - previous.orientation.z).abs() > 0.002
                        || (pose.orientation.w - previous.orientation.w).abs() > 0.002;
                    if changed {
                        crate::capi::log_call(&format!(
                            "OpenXR {} grip flags={:?} pos=({:.3},{:.3},{:.3}) quat=({:.3},{:.3},{:.3},{:.3})",
                            if index == 0 { "left" } else { "right" },
                            location.location_flags,
                            pose.position.x,
                            pose.position.y,
                            pose.position.z,
                            pose.orientation.x,
                            pose.orientation.y,
                            pose.orientation.z,
                            pose.orientation.w,
                        ));
                        self.last_logged_hand_poses[index] = pose;
                    }
                }
                let hand_path = if index == 0 {
                    self.instance.string_to_path("/user/hand/left")
                } else {
                    self.instance.string_to_path("/user/hand/right")
                };
                if let Ok(hand_path) = hand_path {
                    if let Ok(profile) = self.session.current_interaction_profile(hand_path) {
                        // XR_NULL_PATH means no profile is active yet. It is
                        // not legal to pass to xrPathToString; WiVRn reports
                        // this transiently while controller activation settles.
                        if profile != openxr::Path::NULL
                            && self.active_profiles[index] != Some(profile)
                        {
                            let profile_name = self
                                .instance
                                .path_to_string(profile)
                                .unwrap_or_else(|_| "<unknown>".to_owned());
                            crate::capi::log_call(&format!(
                                "OpenXR {} interaction profile={profile_name}",
                                if index == 0 { "left" } else { "right" }
                            ));
                            self.active_profiles[index] = Some(profile);
                        }
                    }
                    if let Ok(state) = self.select_action.state(&self.session, hand_path) {
                        self.hand_select[index] = state.is_active && state.current_state;
                        if state.changed_since_last_sync {
                            crate::capi::log_call(&format!(
                                "OpenXR {} simple select active={} pressed={}",
                                if index == 0 { "left" } else { "right" },
                                state.is_active,
                                state.current_state,
                            ));
                        }
                    }
                    if let Ok(state) = self.primary_action.state(&self.session, hand_path) {
                        self.hand_primary[index] = state.is_active && state.current_state;
                    }
                    macro_rules! bool_state {
                        ($field:ident, $action:ident) => {
                            if let Ok(state) = self.$action.state(&self.session, hand_path) {
                                self.$field[index] = state.is_active && state.current_state;
                            }
                        };
                    }
                    bool_state!(hand_secondary, secondary_action);
                    bool_state!(hand_menu, menu_action);
                    bool_state!(hand_thumbstick_click, thumbstick_click_action);
                    bool_state!(hand_primary_touch, primary_touch_action);
                    bool_state!(hand_secondary_touch, secondary_touch_action);
                    bool_state!(hand_thumbstick_touch, thumbstick_touch_action);
                    bool_state!(hand_trigger_touch, trigger_touch_action);
                    if let Ok(state) = self.trigger_action.state(&self.session, hand_path) {
                        self.hand_trigger[index] = if state.is_active {
                            state.current_state
                        } else {
                            0.0
                        };
                    }
                    if let Ok(state) = self.squeeze_action.state(&self.session, hand_path) {
                        self.hand_squeeze[index] = if state.is_active {
                            state.current_state
                        } else {
                            0.0
                        };
                    }
                    if let Ok(state) = self.thumbstick_action.state(&self.session, hand_path) {
                        self.hand_thumbstick[index] = if state.is_active {
                            state.current_state
                        } else {
                            openxr::Vector2f { x: 0.0, y: 0.0 }
                        };
                    }
                }
            }

        }
        Ok(())
    }

    pub fn begin_frame(&mut self) -> Result<(), String> {
        if self.frame_state.is_some() {
            self.stream.begin().map_err(|error| error.to_string())?;
            if let Some(swapchain) = self.color_swapchain.as_mut() {
                let image = swapchain
                    .acquire_image()
                    .map_err(|error| error.to_string())?;
                swapchain
                    .wait_image(openxr::Duration::INFINITE)
                    .map_err(|error| error.to_string())?;
                self.color_image = Some(image);
                if let Some(states) = &self.d3d12_color_states { states.acquire(image); }
                crate::capi::log_call(&format!("OpenXR color acquired index={image}"));
            }
            self.frame_begun = true;
        }
        Ok(())
    }

    /// Finish the frame using Echo's submitted viewport and projection. Echo
    /// rendered these pixels with its layer FOV, so the compositor must use
    /// the same projection; only the poses remain runtime-located.
    pub fn end_frame(
        &mut self,
        submitted: Option<([openxr::Rect2Di; 2], [openxr::Fovf; 2])>,
    ) -> Result<(), String> {
        if self.frame_begun {
            let state = self
                .frame_state
                .take()
                .expect("frame state set before begin");
            let display_time = state.predicted_display_time;
            let runtime_views = if let Some(views) = self.frame_views.take() {
                views
            } else {
                // Defensive fallback for callers that bypass the normal
                // WaitToBeginFrame -> BeginFrame sequence.
                self.session
                    .locate_views(
                        openxr::ViewConfigurationType::PRIMARY_STEREO,
                        display_time,
                        &self.space,
                    )
                    .map_err(|error| error.to_string())?
                    .1
            };
            if runtime_views.len() >= 2 {
                crate::capi::log_call(&format!(
                    "OpenXR EndFrame runtime views left=p({:.3},{:.3},{:.3}) q({:.3},{:.3},{:.3},{:.3}) fov=({:.3},{:.3},{:.3},{:.3}) right=p({:.3},{:.3},{:.3}) q({:.3},{:.3},{:.3},{:.3}) fov=({:.3},{:.3},{:.3},{:.3})",
                    runtime_views[0].pose.position.x,
                    runtime_views[0].pose.position.y,
                    runtime_views[0].pose.position.z,
                    runtime_views[0].pose.orientation.x,
                    runtime_views[0].pose.orientation.y,
                    runtime_views[0].pose.orientation.z,
                    runtime_views[0].pose.orientation.w,
                    runtime_views[0].fov.angle_left.tan(),
                    runtime_views[0].fov.angle_right.tan(),
                    runtime_views[0].fov.angle_up.tan(),
                    runtime_views[0].fov.angle_down.tan(),
                    runtime_views[1].pose.position.x,
                    runtime_views[1].pose.position.y,
                    runtime_views[1].pose.position.z,
                    runtime_views[1].pose.orientation.x,
                    runtime_views[1].pose.orientation.y,
                    runtime_views[1].pose.orientation.z,
                    runtime_views[1].pose.orientation.w,
                    runtime_views[1].fov.angle_left.tan(),
                    runtime_views[1].fov.angle_right.tan(),
                    runtime_views[1].fov.angle_up.tan(),
                    runtime_views[1].fov.angle_down.tan(),
                ));
            }
            if let (Some(states), Some(image)) = (&self.d3d12_color_states, self.color_image) {
                states.release(image);
            }
            crate::capi::log_call(&format!("OpenXR color releasing index={:?}", self.color_image));
            if let Some(swapchain) = self.color_swapchain.as_mut() {
                swapchain
                    .release_image()
                    .map_err(|error| error.to_string())?;
            }
            self.color_image = None;
            if let (Some(swapchain), Some((width, height))) =
                (&self.color_swapchain, self.color_extent)
            {
                let projection_views: Vec<_> = (0..2)
                    .map(|eye| {
                        let view = &runtime_views[eye];
                        let rect = submitted
                            .as_ref()
                            .map(|(rects, _)| rects[eye])
                            .unwrap_or_else(|| {
                                let half_width = (width / 2) as i32;
                                openxr::Rect2Di {
                                    offset: openxr::Offset2Di {
                                        x: eye as i32 * half_width,
                                        y: 0,
                                    },
                                    extent: openxr::Extent2Di {
                                        width: half_width,
                                        height: height as i32,
                                    },
                                }
                            });
                        let fov = submitted
                            .as_ref()
                            .map(|(_, fovs)| fovs[eye])
                            .unwrap_or(view.fov);
                        // Some runtimes return a zero pose while their view
                        // state is not valid. xrEndFrame rejects that outright;
                        // retain the last valid view pose, or derive a valid
                        // eye pose from the current head pose until views are
                        // available.
                        let pose = if valid_orientation(view.pose) {
                            view.pose
                        } else if valid_orientation(self.view_poses[eye]) {
                            self.view_poses[eye]
                        } else {
                            let mut pose = self.head_pose;
                            pose.position.x += if eye == 0 { -0.032 } else { 0.032 };
                            pose
                        };
                        openxr::CompositionLayerProjectionView::new()
                            .pose(pose)
                            .fov(fov)
                            .sub_image(
                                openxr::SwapchainSubImage::new()
                                    .swapchain(swapchain)
                                    .image_array_index(0)
                                    .image_rect(rect),
                            )
                    })
                    .collect();
                let layer = openxr::CompositionLayerProjection::new()
                    .space(&self.space)
                    .views(&projection_views);
                self.stream
                    .end(
                        display_time,
                        openxr::EnvironmentBlendMode::OPAQUE,
                        &[&layer],
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                self.stream
                    .end(display_time, openxr::EnvironmentBlendMode::OPAQUE, &[])
                    .map_err(|error| error.to_string())?;
            }
            self.frame_begun = false;
        }
        Ok(())
    }
}

/// Create and immediately destroy a D3D12 OpenXR session. This is deliberately
/// diagnostic-only while the CAPI owns Echo's swapchains; it verifies that the
/// exact D3D12 device/queue passed by Echo are acceptable to the active runtime.
#[cfg(windows)]
pub unsafe fn probe_d3d12_session(
    device: *mut core::ffi::c_void,
    queue: *mut core::ffi::c_void,
) -> Result<(), String> {
    crate::capi::log_call("OpenXR loading standard loader");
    let entry = unsafe { Entry::load() }.map_err(|error| error.to_string())?;
    crate::capi::log_call("OpenXR loader loaded; enumerating extensions");
    let extensions = entry
        .enumerate_extensions()
        .map_err(|error| error.to_string())?;
    if !extensions.khr_d3d12_enable {
        return Err("XR_KHR_d3d12_enable unavailable".into());
    }
    let mut requested = ExtensionSet::default();
    requested.khr_d3d12_enable = true;
    let instance = entry
        .create_instance(
            &ApplicationInfo {
                application_name: "libovr-openxr",
                application_version: 1,
                engine_name: "Echo VR",
                engine_version: 1,
                api_version: openxr::Version::new(1, 0, 0),
            },
            &requested,
            &[],
        )
        .map_err(|error| error.to_string())?;
    let system = instance
        .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|error| error.to_string())?;
    let info = openxr::d3d::SessionCreateInfoD3D12 {
        device: device.cast(),
        queue: queue.cast(),
    };
    let _session = unsafe { instance.create_session::<openxr::D3D12>(system, &info) }
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn probe() -> Result<OpenXrCapabilities, String> {
    // `Entry::load` dynamically locates the platform OpenXR loader. This is
    // essential for Wine: no OpenXR import library is baked into our DLL.
    // Wine registers wineopenxr.dll as an OpenXR *runtime*. Applications must
    // still use the standard OpenXR loader, exactly as they do on Windows.
    crate::capi::log_call("OpenXR loading standard loader");
    let entry = unsafe { Entry::load() }.map_err(|error| error.to_string())?;
    crate::capi::log_call("OpenXR loader loaded; enumerating extensions");
    let extensions = entry
        .enumerate_extensions()
        .map_err(|error| error.to_string())?;
    let mut requested = ExtensionSet::default();
    requested.mnd_headless = extensions.mnd_headless;
    #[cfg(windows)]
    {
        requested.khr_d3d11_enable = extensions.khr_d3d11_enable;
        requested.khr_d3d12_enable = extensions.khr_d3d12_enable;
    }
    #[cfg(windows)]
    let d3d11 = extensions.khr_d3d11_enable;
    #[cfg(windows)]
    let d3d12 = extensions.khr_d3d12_enable;
    #[cfg(not(windows))]
    let d3d11 = false;
    #[cfg(not(windows))]
    let d3d12 = false;
    let _instance = entry
        .create_instance(
            &ApplicationInfo {
                application_name: "libovr-openxr",
                application_version: 1,
                engine_name: "libovr-openxr",
                engine_version: 1,
                api_version: openxr::Version::new(1, 0, 0),
            },
            &requested,
            &[],
        )
        .map_err(|error| error.to_string())?;

    Ok(OpenXrCapabilities {
        d3d11,
        d3d12,
        monado_headless: extensions.mnd_headless,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires an installed OpenXR runtime"]
    fn probes_the_active_runtime() {
        super::probe().expect("OpenXR runtime probe");
    }

    #[test]
    #[ignore = "requires an installed OpenXR runtime"]
    fn probes_vulkan_extensions_used_by_wineopenxr() {
        let entry = unsafe { openxr::Entry::load() }.expect("OpenXR loader");
        let available = entry.enumerate_extensions().expect("extension enumeration");
        eprintln!("available OpenXR extensions: {available:?}");

        for (name, enable_vulkan_1) in [("vulkan1", true), ("vulkan2", false)] {
            let mut requested = openxr::ExtensionSet::default();
            requested.khr_vulkan_enable = enable_vulkan_1 && available.khr_vulkan_enable;
            requested.khr_vulkan_enable2 = !enable_vulkan_1 && available.khr_vulkan_enable2;
            requested.khr_convert_timespec_time = available.khr_convert_timespec_time;
            entry
                .create_instance(
                    &openxr::ApplicationInfo {
                        application_name: "libovr-openxr-wine-probe",
                        application_version: 1,
                        engine_name: "test",
                        engine_version: 1,
                        api_version: openxr::Version::new(1, 0, 0),
                    },
                    &requested,
                    &[],
                )
                .unwrap_or_else(|error| panic!("{name} instance creation: {error}"));
        }
    }
}
