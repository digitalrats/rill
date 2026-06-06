use rill_core::Transcendental;

/// Parameters for a 2D plate/membrane model.
#[derive(Debug, Clone)]
pub struct PlateParams<T: Transcendental> {
    /// Grid width (4–64, power of two recommended).
    pub grid_width: usize,
    /// Grid height (4–64, power of two recommended).
    pub grid_height: usize,
    /// Wave speed coefficient (0.0–0.5, stability requires ≤ 0.25 for 2D).
    pub wave_speed: T,
    /// Decay per sample (0.0–1.0, 0.999 typical).
    pub decay: T,
    /// Boundary condition: 0.0 = clamped, 1.0 = free edge.
    pub boundary: T,
    /// Excitation position as fraction of width (0.0–1.0).
    pub excitation_x: T,
    /// Excitation position as fraction of height (0.0–1.0).
    pub excitation_y: T,
}

impl<T: Transcendental> Default for PlateParams<T> {
    fn default() -> Self {
        Self {
            grid_width: 16,
            grid_height: 16,
            wave_speed: T::from_f64(0.25),
            decay: T::from_f64(0.999),
            boundary: T::from_f64(0.5),
            excitation_x: T::from_f64(0.5),
            excitation_y: T::from_f64(0.5),
        }
    }
}
