//! Comb filter
//!
//! Used in:
//! - Reverb (series of comb filters)
//! - "Metallic" sound effects
//! - Physical string modeling
use super::FilterParams;
use crate::algorithm::ParameterizedAlgorithm;
use crate::vector::{ScalarVector1, Vector};
use rill_core::buffer::DelayLine;
use rill_core::math::vector::scalar::ScalarVector4;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;

/// Comb filter
pub struct CombFilter<T: Transcendental, const MAX_DELAY: usize> {
    params: FilterParams,
    delay: DelayLine<T, MAX_DELAY>,
    feedback: ScalarVector1<T>,
    delay_samples: usize,
    sample_rate: f32,
}

impl<T: Transcendental, const MAX_DELAY: usize> CombFilter<T, MAX_DELAY> {
    /// Create a new comb filter
    pub fn new(params: FilterParams, feedback: f32) -> Self {
        Self {
            params,
            delay: DelayLine::new(44100.0),
            feedback: ScalarVector1::splat(T::from_f32(feedback)),
            delay_samples: 0,
            sample_rate: 44100.0,
        }
    }

    /// Update delay based on cutoff frequency
    fn update_delay(&mut self) {
        // For comb filter, delay = sample_rate / cutoff
        self.delay_samples = (self.sample_rate / self.params.cutoff) as usize;
        self.delay.set_delay_samples(self.delay_samples);
    }

    /// Set feedback
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = ScalarVector1::splat(T::from_f32(feedback));
    }
}

impl<T: Transcendental, const MAX_DELAY: usize> Algorithm<T> for CombFilter<T, MAX_DELAY> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_delay();
        self.delay.clear();
    }

    fn reset(&mut self) {
        self.delay.clear();
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        let fb = self.feedback.extract(0);

        // SIMD path: when delay >= 4 samples, reads and writes don't overlap
        // within a block, so we can batch 4 at a time
        if self.delay_samples >= 4 {
            let chunks = len / 4;

            for chunk in 0..chunks {
                let offset = chunk * 4;

                // Batch read 4 delayed samples
                let mut delayed_buf = [T::ZERO; 4];
                for item in delayed_buf.iter_mut() {
                    *item = self.delay.read_delayed(self.delay_samples);
                }
                let delayed_v = ScalarVector4::load(&delayed_buf);

                // SIMD math
                let input_v = ScalarVector4::load(&input[offset..offset + 4]);
                let write_v = input_v + delayed_v * ScalarVector4::splat(fb);

                // Store output
                delayed_v.store(&mut output[offset..offset + 4]);

                // Batch write 4 feedback samples
                let vals = [
                    write_v.extract(0),
                    write_v.extract(1),
                    write_v.extract(2),
                    write_v.extract(3),
                ];
                for &v in &vals {
                    let _ = self.delay.write(v);
                }
            }

            // Scalar remainder
            for (inp, out) in input[chunks * 4..len]
                .iter()
                .zip(output[chunks * 4..len].iter_mut())
            {
                let delayed = self.delay.read_delayed(self.delay_samples);
                *out = delayed;
                let write_signal = *inp + delayed * fb;
                let _ = self.delay.write(write_signal);
            }
        } else {
            // Short delay: scalar path (reads overlap with writes)
            for i in 0..len {
                let delayed = self.delay.read_delayed(self.delay_samples);
                output[i] = delayed;
                let write_signal = input[i] + delayed * fb;
                let _ = self.delay.write(write_signal);
            }
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Comb Filter",
            category: AlgorithmCategory::Filter,
            description: "Comb filter for reverb and physical modeling",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental, const MAX_DELAY: usize> ParameterizedAlgorithm<T>
    for CombFilter<T, MAX_DELAY>
{
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_delay();
    }
}

// Blanket implementation in mod.rs handles Filter
