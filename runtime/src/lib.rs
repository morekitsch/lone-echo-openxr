//! A LibOVR CAPI 1.94 compatibility layer implemented over OpenXR.
//!
//! The project is deliberately split into a small, ABI-facing CAPI layer and
//! a testable runtime core. The core must be usable with [`mock::MockRuntime`]
//! so most development does not require an HMD, Wine, or a GPU.

pub mod abi;
pub mod capi;
pub mod config;
pub mod hmd_cache;
pub mod mock;
pub mod openxr_backend;
pub mod platform_exports;
pub mod runtime;
mod render_size;
mod perf_stats;
mod log_buffer;
#[cfg(any(windows, test))]
mod tracking_origin;

pub use mock::MockRuntime;
pub use runtime::{FrameId, HeadsetState, Pose, Runtime, RuntimeError, ShimCore, Vec3};

#[cfg(windows)]
mod direct3d;

#[cfg(windows)]
mod d3d11_views;

#[cfg(windows)]
mod d3d12_views;

#[cfg(windows)]
mod d3d12_states;
