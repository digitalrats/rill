//! Mono-to-stereo panning with configurable pan law and exponential smoothing.
//!
//! Provides [`MonoToStereo`] — a [`MultichannelAlgorithm`](rill_core::traits::MultichannelAlgorithm)
//! that converts a mono signal to stereo with per-sample gain control.

use rill_core::math::Transcendental;
use rill_core::traits::{MultichannelAlgorithm, ProcessResult};

/// Pan law — determines gain distribution between left and right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanLaw {
    /// sqrt(2)/2 * (cos(theta) + sin(theta)) per channel, constant perceived loudness.
    ConstantPower,
    /// Linear fade: left = 1-pan, right = pan. -3 dB dip at center.
    Linear,
}

impl PanLaw {
    /// Compute left and right gain factors for a given pan position.
    ///
    /// `pan` is clamped to `[-1.0, 1.0]`, where -1.0 = hard left, 0.0 = center, 1.0 = hard right.
    pub fn gains(self, pan: f32) -> (f32, f32) {
        let pan = pan.clamp(-1.0, 1.0);
        match self {
            PanLaw::ConstantPower => {
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                (angle.cos(), angle.sin())
            }
            PanLaw::Linear => {
                let right = (pan + 1.0) * 0.5;
                (1.0 - right, right)
            }
        }
    }
}

/// Converts a mono signal to stereo with pan and exponential smoothing.
pub struct MonoToStereo<T: Transcendental> {
    pan_law: PanLaw,
    pan: f32,
    smoothing: f32,
    left_gain: f32,
    right_gain: f32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental> MonoToStereo<T> {
    /// Create a new `MonoToStereo` with the given pan law, initial pan position, and smoothing factor.
    pub fn new(pan_law: PanLaw, pan: f32, smoothing: f32) -> Self {
        let (lg, rg) = pan_law.gains(pan);
        Self {
            pan_law,
            pan,
            smoothing,
            left_gain: lg,
            right_gain: rg,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set the target pan position (clamped to `[-1.0, 1.0]`).
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan.clamp(-1.0, 1.0);
    }

    /// Set the smoothing factor (clamped to `[0.0, 1.0]`), controlling how fast gains converge toward targets.
    pub fn set_smoothing(&mut self, s: f32) {
        self.smoothing = s.clamp(0.0, 1.0);
    }
}

impl<T: Transcendental> Default for MonoToStereo<T> {
    fn default() -> Self {
        Self::new(PanLaw::ConstantPower, 0.0, 0.1)
    }
}

impl<T: Transcendental> MultichannelAlgorithm<T> for MonoToStereo<T> {
    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        2
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let mono = inputs[0];

        let (tg_l, tg_r) = self.pan_law.gains(self.pan);
        self.left_gain += self.smoothing * (tg_l - self.left_gain);
        self.right_gain += self.smoothing * (tg_r - self.right_gain);

        let lg = T::from_f32(self.left_gain);
        let rg = T::from_f32(self.right_gain);

        for i in 0..mono.len() {
            let s = mono[i];
            outputs[0][i] = s * lg;
            outputs[1][i] = s * rg;
        }
        Ok(())
    }

    fn reset(&mut self) {
        let (lg, rg) = self.pan_law.gains(self.pan);
        self.left_gain = lg;
        self.right_gain = rg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_power_center_spreads_equally() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, 0.0, 0.0);
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]])
            .unwrap();
        for i in 0..64 {
            assert!((left[i] - right[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn pan_hard_left_silences_right() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, -1.0, 0.0);
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]])
            .unwrap();
        assert!(left.iter().any(|&v| v > 0.0));
        assert!(right.iter().all(|&v| v < 1e-6));
    }

    #[test]
    fn smoothing_ramps_toward_target() {
        let mut ms = MonoToStereo::<f32>::new(PanLaw::ConstantPower, 1.0, 0.0);
        let input = [1.0f32; 64];
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]])
            .unwrap();
        ms.set_pan(-1.0);
        ms.set_smoothing(0.5);
        ms.process(&[&input[..]], &mut [&mut left[..], &mut right[..]])
            .unwrap();
        assert!(
            left[0] > 0.0,
            "left should have non-zero signal after pan change"
        );
        assert!(right[0] < 1.0, "right should have decreased");
    }
}
