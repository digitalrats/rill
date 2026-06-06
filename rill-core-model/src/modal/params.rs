use rill_core::Transcendental;

/// Definition of a single resonant mode.
#[derive(Debug, Clone, Copy)]
pub struct ModeParams<T: Transcendental> {
    /// Frequency ratio relative to fundamental (1.0 = fundamental).
    pub freq_ratio: T,
    /// Amplitude of this mode (0.0–1.0).
    pub amplitude: T,
    /// Decay time in seconds (time to -60 dB).
    pub decay_time: T,
}

/// Parameters for a modal resonator model.
#[derive(Debug, Clone)]
pub struct ModalParams<T: Transcendental, const MAX_MODES: usize> {
    /// Number of active modes (1–MAX_MODES).
    pub num_modes: usize,
    /// Mode definitions (frequency ratio, amplitude, decay time).
    pub modes: [ModeParams<T>; MAX_MODES],
    /// Fundamental frequency in Hz.
    pub fundamental: T,
    /// Global damping multiplier (1.0 = natural, > 1 = heavier damping).
    pub damping: T,
}

impl<T: Transcendental, const MAX_MODES: usize> Default for ModalParams<T, MAX_MODES> {
    fn default() -> Self {
        let default_mode = ModeParams {
            freq_ratio: T::ONE,
            amplitude: T::ONE,
            decay_time: T::from_f32(1.0),
        };
        Self {
            num_modes: 1,
            modes: [default_mode; MAX_MODES],
            fundamental: T::from_f32(440.0),
            damping: T::ONE,
        }
    }
}

/// Pre-computed bell modal parameters (5 modes).
///
/// Ratios approximate a tuned bell: 1.0, 2.76, 5.40, 8.93, 13.34
/// with amplitudes decaying as 1/n².
pub fn bell_modes<T: Transcendental, const MAX_MODES: usize>() -> ModalParams<T, MAX_MODES> {
    let ratios = [1.0, 2.76, 5.40, 8.93, 13.34];
    let amplitudes = [1.0, 0.67, 0.34, 0.12, 0.06];
    let decays = [2.0, 1.5, 0.8, 0.3, 0.1];
    let mut modes = [ModeParams {
        freq_ratio: T::ONE,
        amplitude: T::ZERO,
        decay_time: T::from_f32(0.01),
    }; MAX_MODES];
    let limit = 5.min(MAX_MODES);
    for i in 0..limit {
        modes[i] = ModeParams {
            freq_ratio: T::from_f64(ratios[i]),
            amplitude: T::from_f64(amplitudes[i]),
            decay_time: T::from_f64(decays[i]),
        };
    }
    ModalParams {
        num_modes: limit,
        modes,
        fundamental: T::from_f32(440.0),
        damping: T::ONE,
    }
}

/// Pre-computed marimba modal parameters (3 modes).
///
/// Ratios approximate a tuned marimba bar: 1.0, 4.0, 9.0
/// with amplitudes decaying as 1/n.
pub fn marimba_modes<T: Transcendental, const MAX_MODES: usize>() -> ModalParams<T, MAX_MODES> {
    let ratios = [1.0, 4.0, 9.0];
    let amplitudes = [1.0, 0.5, 0.2];
    let decays = [3.0, 1.5, 0.5];
    let mut modes = [ModeParams {
        freq_ratio: T::ONE,
        amplitude: T::ZERO,
        decay_time: T::from_f32(0.01),
    }; MAX_MODES];
    let limit = 3.min(MAX_MODES);
    for i in 0..limit {
        modes[i] = ModeParams {
            freq_ratio: T::from_f64(ratios[i]),
            amplitude: T::from_f64(amplitudes[i]),
            decay_time: T::from_f64(decays[i]),
        };
    }
    ModalParams {
        num_modes: limit,
        modes,
        fundamental: T::from_f32(440.0),
        damping: T::ONE,
    }
}
