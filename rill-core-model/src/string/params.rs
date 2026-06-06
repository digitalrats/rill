use rill_core::Transcendental;

/// Parameters for a physical string model.
#[derive(Debug, Clone)]
pub struct StringParams<T: Transcendental> {
    /// Fundamental frequency in Hz (e.g., 440.0 for A4).
    pub frequency: T,
    /// Decay factor per sample (0.0 = instant decay, 0.99999 = near-infinite sustain).
    pub decay: T,
    /// Stiffness coefficient (0.0 = ideal flexible string, > 0 = inharmonic dispersion).
    pub stiffness: T,
    /// Brightness — loop filter cutoff ratio (0.0 = fully dark, 1.0 = fully bright).
    pub brightness: T,
}

impl<T: Transcendental> Default for StringParams<T> {
    fn default() -> Self {
        Self {
            frequency: T::from_f32(440.0),
            decay: T::from_f32(0.9995),
            stiffness: T::from_f32(0.0),
            brightness: T::from_f32(0.95),
        }
    }
}
