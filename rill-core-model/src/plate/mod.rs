//! 2D plate/membrane model — waveguide mesh with FDTD.
//!
//! Solves the 2D wave equation ∂²z/∂t² = c²(∂²z/∂x² + ∂²z/∂y²)
//! using finite-difference time-domain (FDTD) on a rectangular grid.
//! Single-sample excitation at a configurable position; output is
//! read at the same position.

mod params;

pub use params::PlateParams;

use rill_core::traits::algorithm::{
    ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
};
use rill_core::traits::ParamValue;
use rill_core::Transcendental;

/// 2D plate model using FDTD waveguide mesh.
///
/// Pre-allocates two full grids (`grid_prev`, `grid_curr`) plus a scratch
/// buffer. All grid updates happen inside `process()` — RT-safe, no allocation.
#[derive(Debug, Clone)]
pub struct PlateModel<T: Transcendental> {
    params: PlateParams<T>,
    grid_prev: Vec<T>,
    grid_curr: Vec<T>,
    grid_next: Vec<T>,
    exc_x: usize,
    exc_y: usize,
    input_energy: T,
}

impl<T: Transcendental> PlateModel<T> {
    /// Create a plate model with the given grid dimensions.
    ///
    /// The grid is pre-allocated. `wave_speed` must be ≤ 0.25 for stability.
    pub fn new(params: PlateParams<T>) -> Self {
        let size = params.grid_width * params.grid_height;
        let exc_x = ((params.excitation_x.to_f64() * (params.grid_width - 1) as f64).round()
            as usize)
            .min(params.grid_width - 1);
        let exc_y = ((params.excitation_y.to_f64() * (params.grid_height - 1) as f64).round()
            as usize)
            .min(params.grid_height - 1);
        Self {
            params,
            grid_prev: vec![T::ZERO; size],
            grid_curr: vec![T::ZERO; size],
            grid_next: vec![T::ZERO; size],
            exc_x,
            exc_y,
            input_energy: T::ZERO,
        }
    }

    /// Excite the plate at the configured excitation position.
    pub fn strike(&mut self, strength: T) {
        let idx = self.exc_y * self.params.grid_width + self.exc_x;
        self.grid_curr[idx] = strength;
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.params.grid_width + x
    }

    fn process_sample(&mut self, input: T) -> T {
        let w = self.params.grid_width;
        let h = self.params.grid_height;
        let c2 = self.params.wave_speed * self.params.wave_speed;
        let two = T::from_f32(2.0);

        // FDTD update for interior points
        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                let i = self.idx(x, y);
                let lap = self.grid_curr[self.idx(x, y - 1)]
                    + self.grid_curr[self.idx(x, y + 1)]
                    + self.grid_curr[self.idx(x - 1, y)]
                    + self.grid_curr[self.idx(x + 1, y)]
                    - self.grid_curr[i] * T::from_f32(4.0);
                self.grid_next[i] = c2 * lap + two * self.grid_curr[i] - self.grid_prev[i];
                self.grid_next[i] = self.grid_next[i] * self.params.decay;
            }
        }

        // Boundary damping (simplified clamped edge)
        let bound = self.params.boundary;
        for x in 0..w {
            let top = self.idx(x, 0);
            let bot = self.idx(x, h - 1);
            self.grid_next[top] = self.grid_next[top] * bound;
            self.grid_next[bot] = self.grid_next[bot] * bound;
        }
        for y in 1..(h - 1) {
            let left = self.idx(0, y);
            let right = self.idx(w - 1, y);
            self.grid_next[left] = self.grid_next[left] * bound;
            self.grid_next[right] = self.grid_next[right] * bound;
        }

        // Excitation injection
        let exc_idx = self.idx(self.exc_x, self.exc_y);
        self.grid_next[exc_idx] = self.grid_next[exc_idx] + input;

        let output = self.grid_next[exc_idx];

        // Rotate grids
        std::mem::swap(&mut self.grid_prev, &mut self.grid_curr);
        std::mem::swap(&mut self.grid_curr, &mut self.grid_next);

        output
    }
}

impl<T: Transcendental> Algorithm<T> for PlateModel<T> {
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> rill_core::traits::ProcessResult<()> {
        for (i, out) in output.iter_mut().enumerate() {
            let inp = input
                .map(|s| s.get(i).copied().unwrap_or(T::ZERO))
                .unwrap_or(T::ZERO);
            *out = self.process_sample(inp);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.grid_prev.fill(T::ZERO);
        self.grid_curr.fill(T::ZERO);
        self.grid_next.fill(T::ZERO);
        self.input_energy = T::ZERO;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Plate Model",
            category: AlgorithmCategory::Generator,
            description: "2D FDTD waveguide mesh for plate and membrane simulation",
            author: "Rill",
            version: "0.5",
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for PlateModel<T> {
    type Params = PlateParams<T>;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        let size_changed = params.grid_width != self.params.grid_width
            || params.grid_height != self.params.grid_height;
        self.params = params;
        if size_changed {
            let size = self.params.grid_width * self.params.grid_height;
            self.grid_prev = vec![T::ZERO; size];
            self.grid_curr = vec![T::ZERO; size];
            self.grid_next = vec![T::ZERO; size];
        }
        self.exc_x = ((self.params.excitation_x.to_f64() * (self.params.grid_width - 1) as f64)
            .round() as usize)
            .min(self.params.grid_width - 1);
        self.exc_y = ((self.params.excitation_y.to_f64() * (self.params.grid_height - 1) as f64)
            .round() as usize)
            .min(self.params.grid_height - 1);
    }

    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        match name {
            "wave_speed" => {
                let mut p = self.params.clone();
                p.wave_speed = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.25));
                self.set_params(p);
                Ok(())
            }
            "decay" => {
                let mut p = self.params.clone();
                p.decay = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.999));
                self.set_params(p);
                Ok(())
            }
            "boundary" => {
                let mut p = self.params.clone();
                p.boundary = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.5));
                self.set_params(p);
                Ok(())
            }
            _ => Err("Unknown parameter"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plate_creation() {
        let params = PlateParams::<f64>::default();
        let model = PlateModel::<f64>::new(params);
        assert_eq!(model.grid_prev.len(), 16 * 16);
    }

    #[test]
    fn test_plate_algorithm_process() {
        let params = PlateParams::<f64>::default();
        let mut model = PlateModel::<f64>::new(params);
        model.strike(1.0.into());
        let mut output = [0.0f64; 64];
        let tick = rill_core::time::ClockTick::default();
        let ctx = ActionContext::new(&tick);
        model.process(None, &mut output, &ctx).unwrap();
        let max_abs = output.iter().map(|x| x.abs()).fold(0.0, f64::max);
        assert!(max_abs > 0.0);
    }

    #[test]
    fn test_plate_boundary() {
        let mut params = PlateParams::<f64>::default();
        params.boundary = 0.0.into();
        params.decay = 0.95.into();
        params.grid_width = 8;
        params.grid_height = 8;
        params.excitation_x = 0.5.into();
        params.excitation_y = 0.5.into();
        let mut model = PlateModel::<f64>::new(params);
        model.strike(1.0.into());
        let tick = rill_core::time::ClockTick::default();
        let ctx = ActionContext::new(&tick);
        for _ in 0..200 {
            let mut out = [0.0f64; 1];
            model.process(None, &mut out, &ctx).unwrap();
        }
        let mut out = [0.0f64; 1];
        model.process(None, &mut out, &ctx).unwrap();
        assert!(out[0].abs() < 0.01, "expected near-zero, got {}", out[0]);
    }

    #[test]
    fn test_plate_params() {
        let params = PlateParams::<f64>::default();
        let mut model = PlateModel::<f64>::new(params);
        let new_params = PlateParams {
            wave_speed: 0.125.into(),
            ..PlateParams::default()
        };
        model.set_params(new_params);
        let ws = model.params.wave_speed.to_f64();
        assert!((ws - 0.125).abs() < 1e-10, "expected 0.125, got {}", ws);
    }

    #[test]
    fn test_plate_reset() {
        let params = PlateParams::<f64>::default();
        let mut model = PlateModel::<f64>::new(params);
        model.strike(1.0.into());
        model.reset();
        let tick = rill_core::time::ClockTick::default();
        let ctx = ActionContext::new(&tick);
        let mut out = [0.0f64; 1];
        model.process(None, &mut out, &ctx).unwrap();
        assert!((out[0].abs() - 0.0).abs() < 1e-15);
    }
}
