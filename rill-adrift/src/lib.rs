#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

pub use rill_core;
pub use rill_core_actor;
pub use rill_core_dsp;
pub use rill_digital_effects;
pub use rill_digital_filters;
pub use rill_graph;
pub use rill_patchbay;
pub use rill_router;

#[cfg(feature = "io")]
pub use rill_io as io;

#[cfg(feature = "lofi")]
pub use rill_lofi as lofi;

#[cfg(feature = "telemetry")]
pub use rill_telemetry as telemetry;

#[cfg(feature = "osc")]
pub use rill_osc as osc;

#[cfg(feature = "analog")]
pub use rill_core_model as core_model;

#[cfg(feature = "analog")]
pub use rill_analog_filters as analog_filters;

#[cfg(feature = "analog")]
pub use rill_analog_effects as analog_effects;

#[cfg(feature = "sampler")]
pub use rill_sampler as sampler;

pub use rill_lang as lang;

#[cfg(feature = "fft")]
pub use rill_fft as fft;

/// rill-lang DSP/model built-in bindings.
pub mod lang_builtins;

/// Centralised node type registration for the Rill ecosystem.
pub mod registration;

pub mod modular;

/// Common re-exports for typical Rill application development.
pub mod prelude {
    pub use rill_core::prelude::*;
}

/// Shared memory IPC initialization for debugging.
/// Activated by the `debug` feature.
#[cfg(feature = "debug")]
pub mod debug_init {
    use rill_telemetry::debug::ipc::ShmemRegion;

    /// Create the shared memory region for IPC debugging.
    /// Called at host application startup. Returns None if creation fails.
    pub fn init_shmem() -> Option<ShmemRegion> {
        match ShmemRegion::create() {
            Ok(shmem) => {
                log::info!(
                    "rill-debug: shmem created /dev/shm/rill-debug-{}",
                    std::process::id()
                );
                Some(shmem)
            }
            Err(e) => {
                log::warn!("rill-debug: failed to create shmem: {}", e);
                None
            }
        }
    }

    /// Open shmem from environment variable (for child processes launched by rill-analyzer).
    pub fn init_shmem_from_env() -> Option<ShmemRegion> {
        match ShmemRegion::open_from_env("RILL_DEBUG_SHMEM") {
            Ok(shmem) => {
                shmem.set_flag(rill_telemetry::debug::ipc::FLAG_ATTACHED);
                log::info!(
                    "rill-debug: shmem opened from env, pid={}",
                    shmem.process_pid()
                );
                Some(shmem)
            }
            Err(_) => None,
        }
    }
}
