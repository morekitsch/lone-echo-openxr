//! Runtime-independent state machine and data model.

use core::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    /// XYZW quaternion.
    pub orientation: [f32; 4],
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub position_valid: bool,
    pub orientation_valid: bool,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::default(),
            orientation: [0.0, 0.0, 0.0, 1.0],
            linear_velocity: Vec3::default(),
            angular_velocity: Vec3::default(),
            position_valid: false,
            orientation_valid: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadsetState {
    pub head: Pose,
    pub left_hand: Pose,
    pub right_hand: Pose,
    pub mounted: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    NotInitialized,
    SessionLost,
    Unsupported,
    BackendFailure,
}

/// Narrow interface used by the CAPI adapter. The future OpenXR backend and
/// deterministic mock implement the same contract.
pub trait Runtime: Send {
    fn initialize(&mut self) -> Result<(), RuntimeError>;
    fn shutdown(&mut self);
    fn wait_begin_frame(&mut self) -> Result<FrameId, RuntimeError>;
    fn headset_state(&self, predicted_display_time_s: f64) -> Result<HeadsetState, RuntimeError>;
}

/// Enforces lifecycle rules independently of the graphics backend.
#[derive(Debug)]
pub struct ShimCore<R> {
    runtime: R,
    initialized: bool,
}

impl<R: Runtime> ShimCore<R> {
    pub fn new(runtime: R) -> Self {
        Self {
            runtime,
            initialized: false,
        }
    }

    pub fn initialize(&mut self) -> Result<(), RuntimeError> {
        self.runtime.initialize()?;
        self.initialized = true;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if self.initialized {
            self.runtime.shutdown();
            self.initialized = false;
        }
    }

    pub fn wait_begin_frame(&mut self) -> Result<FrameId, RuntimeError> {
        self.require_initialized()?;
        self.runtime.wait_begin_frame()
    }

    pub fn headset_state(
        &self,
        predicted_display_time_s: f64,
    ) -> Result<HeadsetState, RuntimeError> {
        self.require_initialized()?;
        self.runtime.headset_state(predicted_display_time_s)
    }

    fn require_initialized(&self) -> Result<(), RuntimeError> {
        self.initialized
            .then_some(())
            .ok_or(RuntimeError::NotInitialized)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RuntimeError {}
