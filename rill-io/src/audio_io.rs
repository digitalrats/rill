//! Backward‑compat re‑exports.
//!
//! The old `AudioIo` trait has been replaced by `rill_core::io::IoBackend`.

pub use rill_core::io::IoResult;

/// Backward‑compat alias for `dyn IoBackend`.
pub type AudioIo = dyn rill_core::io::IoBackend;

/// Backward‑compat alias for `Arc<dyn IoBackend>`.
pub type AudioIoPtr = std::sync::Arc<dyn rill_core::io::IoBackend>;
