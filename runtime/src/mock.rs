//! Deterministic runtime used by unit, property, and replay tests.

use crate::runtime::{FrameId, HeadsetState, Runtime, RuntimeError};

#[derive(Debug, Default)]
pub struct MockRuntime {
    initialized: bool,
    next_frame: u64,
    pub state: HeadsetState,
    pub fail_next_frame: bool,
}

impl MockRuntime {
    pub fn with_state(state: HeadsetState) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }
}

impl Runtime for MockRuntime {
    fn initialize(&mut self) -> Result<(), RuntimeError> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) {
        self.initialized = false;
    }

    fn wait_begin_frame(&mut self) -> Result<FrameId, RuntimeError> {
        if !self.initialized {
            return Err(RuntimeError::NotInitialized);
        }
        if self.fail_next_frame {
            self.fail_next_frame = false;
            return Err(RuntimeError::SessionLost);
        }
        let frame = FrameId(self.next_frame);
        self.next_frame += 1;
        Ok(frame)
    }

    fn headset_state(&self, _predicted_display_time_s: f64) -> Result<HeadsetState, RuntimeError> {
        self.initialized
            .then_some(self.state)
            .ok_or(RuntimeError::NotInitialized)
    }
}
