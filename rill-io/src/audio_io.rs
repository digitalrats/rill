//! Backward‑compat re‑exports.
//!
//! The old `AudioIo` trait has been replaced by `rill_core::io::IoBackend<f32>`.

pub use rill_core::io::IoResult;

/// Backward‑compat alias for `dyn IoBackend<f32>`.
pub type AudioIo = dyn rill_core::io::IoBackend<f32>;

/// Backward‑compat alias for `IoBackendPtr<f32>`.
pub type AudioIoPtr = crate::signal_io::IoBackendPtr<f32>;
