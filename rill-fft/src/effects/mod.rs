//! Frequency-domain effects built on real FFT.
//!
//! These effects transform the signal into the frequency domain via
//! overlap-add processing, manipulate the spectrum, then transform back.

pub mod spectral_delay;
pub mod spectral_gate;
