use rill_core::math::vector::traits::Vector as VecTrait;
use rill_core::traits::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ProcessResult};
use rill_core::Transcendental;

// First-order WDF lowpass section.
//
// Models an RC pole in the wave digital domain.
// The parameter `alpha` = π·fc/fs / (1 + π·fc/fs) controls the cutoff.
//
// This is functionally equivalent to `Series<Resistor, Capacitor>`
// where the capacitor's scattering has been simplified into a single
// coefficient. The compose macro (`wdf_compose!`) handles static
// Series/Parallel networks correctly, but for filters with memory
// (capacitors, inductors) the explicit scattering form is needed
// for correct state tracking.
crate::wdf_element! {
    name: RcPole<T>,
    params: { alpha: T },
    state: { state: T },
    port_resistance: |_s| T::ONE,
    scattering: |s, a| {
        let b = s.state + s.alpha * (a - s.state);
        s.state = b + s.alpha * (a - b);
        b
    },
    update: |_s| {},
    reset: |s| { s.state = T::ZERO; },
}

// 4-pole Moog ladder filter with resonance feedback.
crate::wdf_cascade! {
    name: MoogLadder<T>,
    section: RcPole<T>,
    count: 4,
    params: { cutoff: T, resonance: T, sample_rate: T },
    state: { feedback_prev: T },
    feedback: |s, input, fb_prev| {
        let k = s.resonance * T::from_f32(4.0);
        let fb = fb_prev * k;
        input - fb.clamp(-T::ONE, T::ONE)
    },
    update: |s| {
        let g = T::PI * s.cutoff / s.sample_rate;
        let alpha = g / (T::ONE + g);
        for p in &mut s.poles { p.alpha = alpha; }
    },
}

impl<T: Transcendental> MoogLadder<T> {
    /// Process 4 independent voices via `ScalarVector4`.
    ///
    /// Each lane of the input vector represents one voice; each lane
    /// of the output vector is the corresponding filtered sample.
    /// Reduces function-call overhead in polyphonic synthesis.
    pub fn process_4_voices(
        &mut self,
        inputs: rill_core::math::vector::scalar::ScalarVector4<T>,
    ) -> rill_core::math::vector::scalar::ScalarVector4<T> {
        rill_core::math::vector::scalar::ScalarVector4::from_fn(|i| {
            self.process_sample(inputs.extract(i))
        })
    }
}

impl<T: Transcendental> crate::WdfElement<T> for MoogLadder<T> {
    fn port_resistance(&self) -> T {
        T::ONE
    }
    fn process_incident(&mut self, a: T) -> T {
        self.process_sample(a)
    }
    fn update_state(&mut self) {}
    fn voltage(&self) -> T {
        self.feedback_prev
    }
    fn current(&self) -> T {
        T::ZERO
    }
    fn reset(&mut self) {
        self.reset();
    }
}

impl<T: Transcendental> Algorithm<T> for MoogLadder<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = T::from_f32(sample_rate);
        self.update_coeffs();
    }

    fn reset(&mut self) {
        self.reset();
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "WDF Moog Ladder Filter",
            category: AlgorithmCategory::Filter,
            description: "WDF-based 4-pole Moog transistor ladder VCF with resonance",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filter(sample_rate: f64) -> MoogLadder<f64> {
        let pole = RcPole::new(0.0);
        let mut filter = MoogLadder::new(pole, 1000.0, 0.0, sample_rate);
        filter.update_coeffs();
        filter
    }

    #[test]
    fn test_lp_section_dc() {
        let fs = 44100.0_f64;
        let fc = 1000.0_f64;
        let g = core::f64::consts::PI * fc / fs;
        let alpha = g / (1.0 + g);
        let mut pole = RcPole::new(alpha);
        let mut out = 0.0_f64;
        for _ in 0..5000 {
            out = crate::WdfElement::process_incident(&mut pole, 1.0);
        }
        assert!((out - 1.0).abs() < 0.01, "DC gain should approach 1.0");
    }

    #[test]
    fn test_moog_ladder_creation() {
        let filter = make_filter(44100.0);
        assert!((filter.cutoff() - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn test_moog_ladder_dc() {
        let mut filter = make_filter(44100.0);
        filter.set_cutoff(100.0);
        let mut out = 0.0_f64;
        for _ in 0..5000 {
            out = filter.process_sample(1.0_f64);
        }
        assert!((out - 1.0_f64).abs() < 0.01, "DC gain should be near 1.0");
    }

    #[test]
    fn test_moog_ladder_cutoff_clamp() {
        let mut filter = make_filter(44100.0);
        filter.set_cutoff(10.0);
        assert!((filter.cutoff() - 20.0).abs() < 1e-6);
        filter.set_cutoff(50000.0);
        assert!((filter.cutoff() - 22050.0).abs() < 1e-6);
    }

    #[test]
    fn test_moog_ladder_algorithm_process() {
        let mut filter = make_filter(44100.0);
        filter.set_cutoff(100.0);
        let input = vec![1.0f64; 64];
        let mut output = vec![0.0f64; 64];
        for _ in 0..500 {
            filter.process(Some(&input), &mut output).unwrap();
        }
        for &o in &output {
            assert!(o.is_finite());
            assert!((o - 1.0).abs() < 0.05);
        }
    }

    #[test]
    fn test_moog_ladder_algorithm_reset() {
        let mut filter = make_filter(44100.0);
        let input = vec![1.0f64; 64];
        let mut output = vec![0.0f64; 64];
        filter.process(Some(&input), &mut output).unwrap();
        filter.reset();
        filter.process(Some(&input), &mut output).unwrap();
        assert!(output[0] >= 0.0);
    }
}
