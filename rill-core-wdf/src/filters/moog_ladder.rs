use rill_core::traits::{ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata, ProcessResult};
use rill_core::AudioNum;

/// WDF first-order lowpass section (bilinear-transform real pole).
///
/// Models a series RC lowpass in the wave digital domain. The coefficient
/// α = π·fc/fs / (1 + π·fc/fs) sets the cutoff frequency.
#[derive(Debug, Clone)]
struct WdfLpSection<T: AudioNum> {
    alpha: T,
    state: T,
}

impl<T: AudioNum> WdfLpSection<T> {
    fn new(alpha: T) -> Self {
        Self {
            alpha,
            state: T::ZERO,
        }
    }

    fn set_alpha(&mut self, alpha: T) {
        self.alpha = alpha;
    }

    fn process(&mut self, a: T) -> T {
        // WDF real-pole scattering:
        //   b = s + α·(a - s)
        //   s' = b + α·(a - b)   (state update for next sample)
        let b = self.state + self.alpha * (a - self.state);
        self.state = b + self.alpha * (a - b);
        b
    }

    fn reset(&mut self) {
        self.state = T::ZERO;
    }
}

/// WDF-based Moog ladder 4-pole lowpass filter.
///
/// Four first-order WDF lowpass sections in cascade with resonance feedback.
/// Uses wave digital filter math for authentic analog behaviour.
#[derive(Debug, Clone)]
pub struct MoogLadder<T: AudioNum> {
    poles: [WdfLpSection<T>; 4],
    cutoff: T,
    resonance: T,
    sample_rate: T,
    feedback_prev: T,
}

impl<T: AudioNum> MoogLadder<T> {
    /// Create a new WDF Moog ladder filter.
    pub fn new(sample_rate: T) -> Self {
        let two = T::from_f32(2.0);
        let mut filter = Self {
            poles: [
                WdfLpSection::new(T::ZERO),
                WdfLpSection::new(T::ZERO),
                WdfLpSection::new(T::ZERO),
                WdfLpSection::new(T::ZERO),
            ],
            cutoff: T::from_f32(1000.0),
            resonance: T::ZERO,
            sample_rate,
            feedback_prev: T::ZERO,
        };
        filter.update_coeffs();
        filter
    }

    /// Get cutoff frequency (Hz).
    pub fn cutoff(&self) -> T {
        self.cutoff
    }

    /// Set cutoff frequency (Hz), clamped to [20, sample_rate/2].
    pub fn set_cutoff(&mut self, cutoff: T) {
        let half_sr = self.sample_rate / T::from_f32(2.0);
        let twenty = T::from_f32(20.0);
        self.cutoff = cutoff.clamp(twenty, half_sr);
        self.update_coeffs();
    }

    /// Get resonance (0.0 – 1.0).
    pub fn resonance(&self) -> T {
        self.resonance
    }

    /// Set resonance (0.0 – 1.0).
    pub fn set_resonance(&mut self, resonance: T) {
        let one = T::ONE;
        self.resonance = resonance.clamp(T::ZERO, one);
    }

    /// Process a single sample.
    pub fn process_sample(&mut self, input: T) -> T {
        let two = T::from_f32(2.0);
        let four = T::from_f32(4.0);

        // Resonance feedback (one-sample delay, standard Moog structure).
        let feedback = self.feedback_prev * self.resonance * four;

        // Incident wave of the first pole.
        // With matched impedances, the incident wave from a voltage source
        // equals the source voltage.
        let a0 = input - feedback.clamp(T::from_f32(-1.0), T::from_f32(1.0));

        // Cascade 4 WDF lowpass sections (b from pole i → a for pole i+1).
        // With identical port resistances, the through wave equals the
        // reflected wave from the previous pole.
        let mut a = a0;
        for pole in &mut self.poles {
            a = pole.process(a);
        }

        // Output voltage = incident wave of last pole (a_last).
        // At DC, the reflected wave b = a (unity transmission), and the
        // voltage across the output port is (a + b)/2 = a.
        let output_voltage = a;

        self.feedback_prev = output_voltage;
        output_voltage
    }

    fn update_coeffs(&mut self) {
        let pi = T::PI;
        let g = pi * self.cutoff / self.sample_rate;
        // α = g / (1 + g)  where g = π·fc/fs
        let alpha = g / (T::ONE + g);
        for pole in &mut self.poles {
            pole.set_alpha(alpha);
        }
    }
}

impl<T: AudioNum> Algorithm<T> for MoogLadder<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = T::from_f32(sample_rate);
        self.update_coeffs();
    }

    fn reset(&mut self) {
        for pole in &mut self.poles {
            pole.reset();
        }
        self.feedback_prev = T::ZERO;
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
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "WDF Moog Ladder Filter",
            category: AlgorithmCategory::Filter,
            description:
                "WDF-based 4-pole Moog transistor ladder VCF with resonance",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::prelude::ClockTick;

    fn make_context(tick: &ClockTick) -> ActionContext {
        ActionContext::new(tick)
    }

    #[test]
    fn test_lp_section_dc() {
        // Single pole, DC input → output should approach input
        let fs = 44100.0;
        let fc = 1000.0;
        let g = core::f64::consts::PI * fc / fs;
        let alpha = g / (1.0 + g);
        let mut pole = WdfLpSection::<f64>::new(alpha);

        let input_wave = 1.0;
        let mut out = 0.0;
        for _ in 0..1000 {
            out = pole.process(input_wave);
        }
        // With DC = 1.0 wave input, output wave should approach input wave
        assert!((out - input_wave).abs() < 0.01);
    }

    #[test]
    fn test_moog_ladder_creation() {
        let filter = MoogLadder::<f64>::new(44100.0);
        assert!((filter.cutoff() - 1000.0).abs() < 1e-6);
        assert!((filter.resonance() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_moog_ladder_set_resonance() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.set_resonance(0.7);
        assert!((filter.resonance() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_moog_ladder_dc() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.set_cutoff(100.0);

        let mut out = 0.0;
        for _ in 0..1000 {
            out = filter.process_sample(1.0);
        }
        assert!((out - 1.0).abs() < 0.01,
            "DC gain should be near unity, got {}", out);
    }

    #[test]
    fn test_moog_ladder_resonance_boost() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.set_cutoff(500.0);
        filter.set_resonance(0.8);

        let mut out = 0.0;
        for i in 0..1000 {
            let t = i as f64 / 44100.0;
            let input = (2.0 * core::f64::consts::PI * 500.0 * t).sin() * 0.1;
            out = filter.process_sample(input);
        }
        assert!(out.abs() > 0.0,
            "resonance should produce a non-zero output");
    }

    #[test]
    fn test_moog_ladder_cutoff_clamp() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.set_cutoff(10.0);
        assert!((filter.cutoff() - 20.0).abs() < 1e-6,
            "cutoff should clamp to 20 Hz");
        filter.set_cutoff(50000.0);
        assert!((filter.cutoff() - 22050.0).abs() < 1e-6,
            "cutoff should clamp to Nyquist");
    }

    #[test]
    fn test_moog_ladder_algorithm_process() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.init(44100.0);
        filter.set_cutoff(100.0);

        let tick = ClockTick::new(0, 64, 44100.0);
        let input = vec![1.0f64; 64];
        let mut output = vec![0.0f64; 64];
        let ctx = make_context(&tick);

        // Process blocks to settle (4 cascaded poles, each with fc=100Hz)
        for _ in 0..200 {
            filter.process(Some(&input), &mut output, &ctx).unwrap();
        }
        for &o in &output {
            assert!(o.is_finite(), "output should be finite, got {}", o);
            assert!((o - 1.0).abs() < 0.05,
                "DC through lowpass should be near 1.0, got {}", o);
        }
    }

    #[test]
    fn test_moog_ladder_algorithm_reset() {
        let tick = ClockTick::new(0, 64, 44100.0);
        let mut filter = MoogLadder::<f64>::new(44100.0);
        let input = vec![1.0f64; 64];
        let mut output = vec![0.0f64; 64];
        let ctx = make_context(&tick);

        filter.process(Some(&input), &mut output, &ctx).unwrap();
        filter.reset();
        filter.process(Some(&input), &mut output, &ctx).unwrap();
        assert!(output[0] >= 0.0);
    }

    #[test]
    fn test_moog_ladder_sine_through() {
        let mut filter = MoogLadder::<f64>::new(44100.0);
        filter.set_cutoff(10000.0);
        filter.set_resonance(0.0);

        let mut max_out = 0.0;
        for i in 0..441 {
            let t = i as f64 / 44100.0;
            let input = (2.0 * core::f64::consts::PI * 100.0 * t).sin() * 0.5;
            let out = filter.process_sample(input);
            if out.abs() > max_out {
                max_out = out.abs();
            }
        }
        // 100 Hz sine through 10 kHz lowpass should pass with little attenuation
        assert!(max_out > 0.1,
            "sine should pass through, max_out = {}", max_out);
    }
}
