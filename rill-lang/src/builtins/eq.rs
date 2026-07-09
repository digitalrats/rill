use rill_core::math::Transcendental;

pub enum BandType {
    Peak = 0,
    LowShelf = 1,
    HighShelf = 2,
    LowPass = 3,
    HighPass = 4,
    BandPass = 5,
    Notch = 6,
}

pub struct EqBandConfig {
    pub freq: f64,
    pub q: f64,
    pub gain_db: f64,
    pub band_type: BandType,
}

pub struct EqConfig {
    pub bands: Vec<EqBandConfig>,
}

struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

pub struct EqState<T: Transcendental> {
    config: EqConfig,
    coeffs: Vec<BiquadCoeffs>,
    x1: Vec<T>,
    x2: Vec<T>,
    y1: Vec<T>,
    y2: Vec<T>,
    sample_rate: f32,
}

impl<T: Transcendental> EqState<T> {
    pub fn new(config: EqConfig, sample_rate: f32) -> Self {
        let n = config.bands.len();
        let mut state = Self {
            coeffs: Vec::with_capacity(n),
            x1: vec![T::ZERO; n],
            x2: vec![T::ZERO; n],
            y1: vec![T::ZERO; n],
            y2: vec![T::ZERO; n],
            config,
            sample_rate,
        };
        state.recompute_coeffs();
        state
    }

    pub fn process_sample(&mut self, input: T) -> T {
        let mut x = input;
        for i in 0..self.config.bands.len() {
            let c = &self.coeffs[i];
            let b0 = T::from_f64(c.b0);
            let b1 = T::from_f64(c.b1);
            let b2 = T::from_f64(c.b2);
            let a1 = T::from_f64(c.a1);
            let a2 = T::from_f64(c.a2);
            let y = b0 * x + b1 * self.x1[i] + b2 * self.x2[i] - a1 * self.y1[i] - a2 * self.y2[i];
            self.x2[i] = self.x1[i];
            self.x1[i] = x;
            self.y2[i] = self.y1[i];
            self.y1[i] = y;
            x = y;
        }
        x
    }

    fn recompute_coeffs(&mut self) {
        self.coeffs.clear();
        for band in &self.config.bands {
            self.coeffs
                .push(compute_biquad(band, self.sample_rate as f64));
        }
    }

    pub fn process_slice(&mut self, input: &[T], output: &mut [T]) {
        for (i, sample) in input.iter().enumerate() {
            output[i] = self.process_sample(*sample);
        }
    }
}

fn compute_biquad(band: &EqBandConfig, sr: f64) -> BiquadCoeffs {
    use std::f64::consts::PI;
    let freq = band.freq.max(20.0).min(sr * 0.49);
    let omega = 2.0 * PI * freq / sr;
    let sn = omega.sin();
    let cs = omega.cos();
    let alpha = sn / (2.0 * band.q.max(0.1));

    let (b0, b1, b2, a0, a1, a2) = match band.band_type {
        BandType::LowPass => {
            let b1 = 1.0 - cs;
            (b1 / 2.0, b1, b1 / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
        BandType::HighPass => {
            let b1 = 1.0 + cs;
            (b1 / 2.0, -b1, b1 / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
        BandType::BandPass => (
            sn / 2.0,
            0.0,
            -sn / 2.0,
            1.0 + alpha,
            -2.0 * cs,
            1.0 - alpha,
        ),
        BandType::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
        BandType::Peak => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            )
        }
        BandType::LowShelf => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            let sa = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cs + sa),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                a * ((a + 1.0) - (a - 1.0) * cs - sa),
                (a + 1.0) + (a - 1.0) * cs + sa,
                -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                (a + 1.0) + (a - 1.0) * cs - sa,
            )
        }
        BandType::HighShelf => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            let sa = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cs + sa),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                a * ((a + 1.0) + (a - 1.0) * cs - sa),
                (a + 1.0) - (a - 1.0) * cs + sa,
                2.0 * ((a - 1.0) - (a + 1.0) * cs),
                (a + 1.0) - (a - 1.0) * cs - sa,
            )
        }
    };

    let a0_inv = 1.0 / a0;
    BiquadCoeffs {
        b0: b0 * a0_inv,
        b1: b1 * a0_inv,
        b2: b2 * a0_inv,
        a1: a1 * a0_inv,
        a2: a2 * a0_inv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_passthrough_no_bands() {
        let config = EqConfig { bands: vec![] };
        let mut eq = EqState::<f32>::new(config, 44100.0);
        let input = [1.0f32, 2.0, 3.0, 4.0];
        let mut output = [0.0f32; 4];
        eq.process_slice(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn eq_does_not_panic() {
        let config = EqConfig {
            bands: vec![
                EqBandConfig {
                    freq: 1000.0,
                    q: 1.0,
                    gain_db: 3.0,
                    band_type: BandType::Peak,
                },
                EqBandConfig {
                    freq: 200.0,
                    q: 0.71,
                    gain_db: -2.0,
                    band_type: BandType::LowShelf,
                },
            ],
        };
        let mut eq = EqState::<f32>::new(config, 44100.0);
        let input = [0.5f32; 64];
        let mut output = [0.0f32; 64];
        eq.process_slice(&input, &mut output);
        for v in &output {
            assert!(v.is_finite(), "EQ output should be finite");
        }
    }
}
