use rill_core::Transcendental;

/// Parameters for a single Helmholtz cavity resonator.
#[derive(Debug, Clone)]
pub struct HelmholtzParams<T: Transcendental> {
    /// Cavity volume in m³ (e.g., 0.001 for 1 liter).
    pub volume: T,
    /// Neck cross-sectional area in m².
    pub neck_area: T,
    /// Neck length in m.
    pub neck_length: T,
    /// Speed of sound in m/s (343.0 default).
    pub sound_speed: T,
    /// Radiation loss coefficient (0.0–1.0).
    pub radiation_loss: T,
    /// Viscous loss coefficient (0.0–1.0).
    pub viscous_loss: T,
    /// Excitation type: 0 = filter (no self-oscillation), 1 = reed.
    pub excitation: u8,
    /// Mouth pressure for reed excitation (0.0–1.0).
    pub pressure: T,
}

impl<T: Transcendental> Default for HelmholtzParams<T> {
    fn default() -> Self {
        Self {
            volume: T::from_f64(0.001),
            neck_area: T::from_f64(0.0001),
            neck_length: T::from_f64(0.02),
            sound_speed: T::from_f64(343.0),
            radiation_loss: T::from_f64(0.01),
            viscous_loss: T::from_f64(0.005),
            excitation: 0,
            pressure: T::ZERO,
        }
    }
}

/// Parameters for a 1D array of coupled Helmholtz cavities.
#[derive(Debug, Clone)]
pub struct CavityArrayParams<T: Transcendental> {
    /// Number of active cavities (1–MAX_CAVITIES).
    pub num_cavities: usize,
    /// Per-cavity Helmholtz parameters.
    pub cavity_params: HelmholtzParams<T>,
    /// Nearest-neighbor coupling strength (0.0–1.0).
    pub coupling: T,
    /// Input position index (0 to N-1).
    pub input_index: usize,
    /// Output position index (0 to N-1).
    pub output_index: usize,
}

impl<T: Transcendental> Default for CavityArrayParams<T> {
    fn default() -> Self {
        Self {
            num_cavities: 4,
            cavity_params: HelmholtzParams::default(),
            coupling: T::from_f64(0.1),
            input_index: 0,
            output_index: 3,
        }
    }
}
