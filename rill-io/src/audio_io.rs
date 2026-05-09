//! Backward‑compat re‑exports.
//!
//! The old `AudioIo` trait has been replaced by `rill_core::io::IoBackend<f32>`.

pub use rill_core::io::IoResult;

/// Backward‑compat alias for `dyn IoBackend<f32>`.
pub type AudioIo = dyn rill_core::io::IoBackend<f32>;

/// Backward‑compat alias for `Arc<dyn IoBackend<f32>>`.
pub type AudioIoPtr = std::sync::Arc<dyn rill_core::io::IoBackend<f32>>;
