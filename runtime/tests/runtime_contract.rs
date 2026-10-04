use libovr_openxr::{HeadsetState, MockRuntime, RuntimeError, ShimCore};

#[test]
fn lifecycle_is_deterministic_without_an_hmd() {
    let mut shim = ShimCore::new(MockRuntime::with_state(HeadsetState {
        mounted: true,
        ..Default::default()
    }));

    assert_eq!(shim.wait_begin_frame(), Err(RuntimeError::NotInitialized));
    shim.initialize().expect("mock initialization");
    assert_eq!(shim.wait_begin_frame().expect("first frame").0, 0);
    assert_eq!(shim.wait_begin_frame().expect("second frame").0, 1);
    assert!(shim.headset_state(42.0).expect("state").mounted);

    shim.shutdown();
    assert_eq!(shim.headset_state(42.0), Err(RuntimeError::NotInitialized));
}

#[test]
fn session_loss_is_observable_and_recoverable() {
    let mut runtime = MockRuntime::default();
    runtime.fail_next_frame = true;
    let mut shim = ShimCore::new(runtime);
    shim.initialize().expect("mock initialization");

    assert_eq!(shim.wait_begin_frame(), Err(RuntimeError::SessionLost));
    assert_eq!(shim.wait_begin_frame().expect("next frame").0, 0);
}

#[test]
fn public_x64_hmd_and_legacy_eye_layouts_match_capi() {
    use libovr_openxr::abi::OvrHmdDesc;
    assert_eq!(std::mem::size_of::<OvrHmdDesc>(), 264);
    assert_eq!(std::mem::offset_of!(OvrHmdDesc, default_eye_fov), 184);
    assert_eq!(std::mem::offset_of!(OvrHmdDesc, resolution), 248);
    assert_eq!(std::mem::offset_of!(OvrHmdDesc, display_refresh_rate), 256);
    assert_eq!(std::mem::size_of::<libovr_openxr::capi::LegacyEyeRenderDesc>(), 56);
}

#[test]
fn tracker_pose_matches_public_capi_layout() {
    use libovr_openxr::abi::OvrTrackerPose;
    // Oculus OVR_CAPI.h: flags, Pose, LeveledPose, four bytes of padding.
    assert_eq!(std::mem::size_of::<OvrTrackerPose>(), 64);
    assert_eq!(std::mem::align_of::<OvrTrackerPose>(), 8);
    assert_eq!(std::mem::offset_of!(OvrTrackerPose, status_flags), 0);
    assert_eq!(std::mem::offset_of!(OvrTrackerPose, pose), 4);
    assert_eq!(std::mem::offset_of!(OvrTrackerPose, leveled_pose), 32);
    assert_eq!(std::mem::offset_of!(OvrTrackerPose, reserved), 60);
}

#[test]
fn active_controller_requests_resolve_to_touch() {
    use libovr_openxr::{abi::OvrInputState, capi::ovr_GetInputState};
    for (requested, expected) in [(0xff, 3), (u32::MAX, 3), (1, 1), (2, 2), (16, 0)] {
        let mut state = OvrInputState::default();
        assert_eq!(ovr_GetInputState(std::ptr::null_mut(), requested, &mut state), 0);
        assert_eq!(state.controller_type, expected);
    }
}
