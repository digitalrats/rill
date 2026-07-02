//! ChipEmulator trait — control interface for audio chip emulators.
//!
//! Complements `Algorithm<f32>` for chips that receive register writes
//! from external player modules (e.g. STC player, NSF player).

/// Control interface for audio chip emulators (AY-3-8910, NES APU, etc.).
///
/// Implementors maintain internal chip state and produce audio via
/// [`Algorithm::process`](rill_core::traits::Algorithm::process).
/// This trait adds the register-write control path.
pub trait ChipEmulator {
    /// Write a batch of chip registers.
    ///
    /// The slice length and encoding are chip-specific.  For AY-3-8910
    /// it is exactly 14 bytes (registers 0–13).  The implementor parses
    /// the bytes and updates internal state.
    fn write_registers(&mut self, regs: &[u8]);
}
