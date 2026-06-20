//! ParameterWrite — polymorphic control interface for DSP engines.
//!
//! Decouples parameter dispatch from the concrete engine type,
//! enabling generic controllers (sequencers, MIDI, OSC) to write
//! parameters to oscillators, chip emulators, or any `Algorithm`
//! implementor through a uniform `write_parameter(name, value)` call.

use crate::traits::{ParamValue, ProcessResult};

/// Polymorphic parameter write interface for DSP engines.
///
/// Implementors accept named parameter writes and apply them
/// immediately to internal state.  The set of supported names
/// is engine-specific.
///
/// # Relationship to `Algorithm<T>`
///
/// `ParameterWrite` handles the *control* path (parameter changes
/// from UI, MIDI, OSC, sequencers).  `Algorithm<T>` handles the
/// *signal* path (per-block audio generation via `process()`).
/// Engines typically implement both.
///
/// # Example
///
/// ```ignore
/// fn send_cc(target: &mut dyn ParameterWrite, cc: u8, value: u8) {
///     let _ = target.write_parameter(
///         "amplitude",
///         ParamValue::Float(value as f32 / 127.0),
///     );
/// }
/// ```
pub trait ParameterWrite {
    /// Write a named parameter value.
    ///
    /// Returns `Ok(())` if the parameter was applied, or
    /// `Err(ProcessError)` if the name is unknown or the value
    /// type is invalid.
    fn write_parameter(&mut self, name: &str, value: ParamValue) -> ProcessResult<()>;

    /// Read a named parameter value.
    ///
    /// Returns `None` by default — engines that support reading
    /// override to return current values.
    fn read_parameter(&self, _name: &str) -> Option<ParamValue> {
        None
    }
}
