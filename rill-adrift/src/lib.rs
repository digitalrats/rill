#![doc = include_str!("../README.md")]

pub use rill_core;
pub use rill_core_dsp;
pub use rill_digital_effects;
pub use rill_digital_filters;
pub use rill_graph;
pub use rill_oscillators;
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
pub use rill_core_wdf as core_wdf;

#[cfg(feature = "analog")]
pub use rill_analog_filters as analog_filters;

#[cfg(feature = "analog")]
pub use rill_analog_effects as analog_effects;

#[cfg(feature = "sampler")]
pub use rill_sampler as sampler;

/// Centralised node type registration for the Rill ecosystem.
pub mod registration;

pub mod runtime;

pub mod prelude {
    pub use rill_core::prelude::*;
}
