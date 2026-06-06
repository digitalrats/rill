//! # Butterworth Filters

use super::{FilterParams, FilterType};
use crate::algorithm::ParameterizedAlgorithm;
use crate::vector::{ScalarVector1, Vector};
use num_complex::Complex64;
use rill_core::traits::algorithm::{
    ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata,
};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;
use std::f64::consts::PI as PI64;

// -----------------------------------------------------------------------------
// Helper functions
// -----------------------------------------------------------------------------

fn butterworth_analog_poles(n: usize) -> Vec<Complex64> {
    let mut poles = Vec::with_capacity(n);
    let n_f64 = n as f64;

    for k in 1..=n {
        let k_f64 = k as f64;
        let theta = PI64 * (2.0 * k_f64 - 1.0) / (2.0 * n_f64);

        let real = -theta.sin();
        let imag = theta.cos();

        poles.push(Complex64::new(real, imag));
    }

    poles
}

// -----------------------------------------------------------------------------
// Biquad section
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct BiquadSection<T: Transcendental> {
    coeffs: [ScalarVector1<T>; 5],
    state: [ScalarVector1<T>; 4],
}

impl<T: Transcendental> BiquadSection<T> {
    fn new() -> Self {
        Self {
            coeffs: [
                ScalarVector1::splat(T::from_f32(1.0)),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
            ],
            state: [
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
            ],
        }
    }

    #[inline(always)]
    fn process(&mut self, input: T) -> T {
        let inp = ScalarVector1::splat(input);
        let b0 = self.coeffs[0];
        let b1 = self.coeffs[1];
        let b2 = self.coeffs[2];
        let a1 = self.coeffs[3];
        let a2 = self.coeffs[4];

        let x1 = self.state[0];
        let x2 = self.state[1];
        let y1 = self.state[2];
        let y2 = self.state[3];

        let output = b0 * inp + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;

        self.state[0] = inp;
        self.state[1] = x1;
        self.state[2] = output;
        self.state[3] = y1;

        output.extract(0)
    }

    fn set_coeffs(&mut self, b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) {
        self.coeffs[0] = ScalarVector1::splat(T::from_f32(b0 as f32));
        self.coeffs[1] = ScalarVector1::splat(T::from_f32(b1 as f32));
        self.coeffs[2] = ScalarVector1::splat(T::from_f32(b2 as f32));
        self.coeffs[3] = ScalarVector1::splat(T::from_f32(a1 as f32));
        self.coeffs[4] = ScalarVector1::splat(T::from_f32(a2 as f32));
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            *s = ScalarVector1::splat(T::ZERO);
        }
    }
}

// -----------------------------------------------------------------------------
// Butterworth filter
// -----------------------------------------------------------------------------

/// Butterworth filter (cascade implementation)
pub struct Butterworth<T: Transcendental, const MAX_SECTIONS: usize> {
    /// Filter parameters (uses shared FilterParams)
    params: FilterParams,
    /// Filter order
    order: usize,
    /// Biquad sections
    sections: [BiquadSection<T>; MAX_SECTIONS],
    /// Number of active sections
    num_sections: usize,
    /// Normalization gain
    gain: ScalarVector1<T>,
    /// Sample rate
    sample_rate: f32,
}

impl<T: Transcendental, const MAX_SECTIONS: usize> Butterworth<T, MAX_SECTIONS> {
    /// Create a new Butterworth filter
    pub fn new(params: FilterParams, order: usize) -> Self {
        let mut filter = Self {
            params,
            order,
            sections: [(); MAX_SECTIONS].map(|_| BiquadSection::new()),
            num_sections: 0,
            gain: ScalarVector1::splat(T::from_f32(1.0)),
            sample_rate: 44100.0,
        };
        filter.design();
        filter
    }

    /// Create a low-pass filter
    pub fn lowpass(cutoff: f32, order: usize) -> Self {
        Self::new(
            FilterParams {
                filter_type: FilterType::LowPass,
                cutoff,
                q: 0.0,
                gain_db: 0.0,
            },
            order,
        )
    }

    /// Create a high-pass filter
    pub fn highpass(cutoff: f32, order: usize) -> Self {
        Self::new(
            FilterParams {
                filter_type: FilterType::HighPass,
                cutoff,
                q: 0.0,
                gain_db: 0.0,
            },
            order,
        )
    }

    /// Design the filter (calculate coefficients)
    pub fn design(&mut self) {
        let n = self.order;
        let cutoff = self.params.cutoff as f64;
        let sample_rate_f64 = self.sample_rate as f64;

        // Frequency pre-warping
        let warp_cutoff = 2.0 * (PI64 * cutoff / sample_rate_f64).tan();

        // Get analog filter poles
        let analog_poles = butterworth_analog_poles(n);

        // Number of biquad sections
        self.num_sections = n.div_ceil(2);

        // Compute gain
        self.gain = self.compute_gain(&analog_poles, warp_cutoff);

        // Group poles into complex-conjugate pairs
        for i in 0..self.num_sections {
            let idx1 = i * 2;
            let idx2 = i * 2 + 1;

            if idx2 < n {
                let p1 = analog_poles[idx1];
                let p2 = analog_poles[idx2];

                let sp1 = p1 * warp_cutoff;
                let sp2 = p2 * warp_cutoff;

                let zp1 = (Complex64::new(2.0, 0.0) + sp1) / (Complex64::new(2.0, 0.0) - sp1);
                let zp2 = (Complex64::new(2.0, 0.0) + sp2) / (Complex64::new(2.0, 0.0) - sp2);

                let a1 = -(zp1 + zp2).re;
                let a2 = (zp1 * zp2).re;

                let (b0, b1, b2) = self.numerator_coeffs(i);

                self.sections[i].set_coeffs(b0, b1, b2, a1, a2);
            } else {
                let p = analog_poles[idx1];

                let sp = p * warp_cutoff;
                let zp = (Complex64::new(2.0, 0.0) + sp) / (Complex64::new(2.0, 0.0) - sp);

                let a1 = -zp.re;
                let a2 = 0.0;

                let (b0, b1, b2) = self.numerator_coeffs(i);

                self.sections[i].set_coeffs(b0, b1, b2, a1, a2);
            }
        }
    }

    fn compute_gain(&self, analog_poles: &[Complex64], _warp_cutoff: f64) -> ScalarVector1<T> {
        let _n = self.order;

        let mut analog_gain = 1.0;
        for pole in analog_poles {
            analog_gain *= (-pole).norm();
        }

        match self.params.filter_type {
            FilterType::LowPass => {
                let mut digital_response = Complex64::new(1.0, 0.0);
                for i in 0..self.num_sections {
                    let b0 = self.sections[i].coeffs[0].extract(0);
                    let b1 = self.sections[i].coeffs[1].extract(0);
                    let b2 = self.sections[i].coeffs[2].extract(0);
                    let a1 = self.sections[i].coeffs[3].extract(0);
                    let a2 = self.sections[i].coeffs[4].extract(0);

                    let b = Complex64::new((b0.to_f32() + b1.to_f32() + b2.to_f32()) as f64, 0.0);
                    let a = Complex64::new((1.0 + a1.to_f32() + a2.to_f32()) as f64, 0.0);

                    digital_response = digital_response * b / a;
                }

                ScalarVector1::splat(T::from_f32((analog_gain / digital_response.norm()) as f32))
            }

            FilterType::HighPass => {
                let mut digital_response = Complex64::new(1.0, 0.0);
                for i in 0..self.num_sections {
                    let b0 = self.sections[i].coeffs[0].extract(0);
                    let b1 = self.sections[i].coeffs[1].extract(0);
                    let b2 = self.sections[i].coeffs[2].extract(0);
                    let a1 = self.sections[i].coeffs[3].extract(0);
                    let a2 = self.sections[i].coeffs[4].extract(0);

                    let b = Complex64::new((b0.to_f32() - b1.to_f32() + b2.to_f32()) as f64, 0.0);
                    let a = Complex64::new((1.0 - a1.to_f32() + a2.to_f32()) as f64, 0.0);

                    digital_response = digital_response * b / a;
                }

                ScalarVector1::splat(T::from_f32((1.0 / digital_response.norm()) as f32))
            }

            _ => ScalarVector1::splat(T::from_f32(1.0)),
        }
    }

    fn numerator_coeffs(&self, _section_idx: usize) -> (f64, f64, f64) {
        match self.params.filter_type {
            FilterType::LowPass => (1.0, 2.0, 1.0),
            FilterType::HighPass => (1.0, -2.0, 1.0),
            FilterType::BandPass => (1.0, 0.0, -1.0),
            _ => (1.0, 0.0, 0.0),
        }
    }
}

impl<T: Transcendental, const MAX_SECTIONS: usize> Algorithm<T> for Butterworth<T, MAX_SECTIONS> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.design();
        self.reset();
    }

    fn reset(&mut self) {
        for section in &mut self.sections[..self.num_sections] {
            section.reset();
        }
    }

    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            let mut x = input[i].mul(self.gain.extract(0));

            for section in &mut self.sections[..self.num_sections] {
                x = section.process(x);
            }
            output[i] = x;
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Butterworth Filter",
            category: AlgorithmCategory::Filter,
            description: format!("Butterworth filter (order {})", self.order).leak(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental, const MAX_SECTIONS: usize> ParameterizedAlgorithm<T>
    for Butterworth<T, MAX_SECTIONS>
{
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.design();
    }
}
