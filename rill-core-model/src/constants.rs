//! Physical constants and numerical tolerances used throughout the crate.

/// Boltzmann constant (J/K).
pub const BOLTZMANN: f64 = 1.380649e-23;

/// Elementary charge (C).
pub const ELECTRON_CHARGE: f64 = 1.60217662e-19;

/// Convergence tolerance for Newton-Raphson iteration (dimensionless).
pub const NEWTON_TOLERANCE: f64 = 1e-9;
