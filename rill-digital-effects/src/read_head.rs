use rill_core::{
    buffer::TapeReader,
    math::Transcendental,
    traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata},
    traits::ProcessResult,
};

pub struct ReadHead<T: Transcendental, const BUF_SIZE: usize> {
    tape: Option<TapeReader<T>>,
    resource_name: String,
    delay: f32,
    sample_rate: f32,
    current_delay_samples: f64,
    delay_smoothing: f64,
}

const DELAY_SMOOTH_SECONDS: f64 = 0.008;

fn delay_smoothing_coeff(sample_rate: f64) -> f64 {
    1.0 - (-1.0 / (DELAY_SMOOTH_SECONDS * sample_rate)).exp()
}

#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Send for ReadHead<T, B> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Sync for ReadHead<T, B> {}

impl<T: Transcendental, const BUF_SIZE: usize> ReadHead<T, BUF_SIZE> {
    pub fn new() -> Self {
        Self::with_resource("tape_0")
    }

    pub fn with_resource(resource_name: &str) -> Self {
        Self {
            tape: None,
            resource_name: resource_name.to_string(),
            delay: 0.5,
            sample_rate: 44100.0,
            current_delay_samples: 0.5 * 44100.0,
            delay_smoothing: delay_smoothing_coeff(44100.0),
        }
    }

    pub fn set_delay(&mut self, delay: f32) {
        self.delay = delay.clamp(0.01, 2.0);
    }

    pub fn set_reader(&mut self, reader: TapeReader<T>) {
        self.tape = Some(reader);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for ReadHead<T, BUF_SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.delay_smoothing = delay_smoothing_coeff(sample_rate as f64);
        self.current_delay_samples = (self.delay as f64) * (sample_rate as f64);
    }

    fn reset(&mut self) {
        self.current_delay_samples = (self.delay as f64) * (self.sample_rate as f64);
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let Some(tape) = self.tape.as_ref() else {
            output.fill(T::ZERO);
            return Ok(());
        };
        let target = (self.delay as f64) * (self.sample_rate as f64);
        let glide = self.delay_smoothing;
        let mut current = self.current_delay_samples;
        let n = output.len();
        for i in 0..n {
            let d = current + (n - 1 - i) as f64;
            output[i] = tape.read_interpolated(d.max(0.0));
            current += (target - current) * glide;
        }
        self.current_delay_samples = current;
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "ReadHead",
            category: AlgorithmCategory::Generator,
            description: "Tape read head with gliding delay",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::buffer::{tape_handles, TapeLoop};

    fn ramp_tape(n: usize) -> TapeLoop<f32> {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        for i in 0..n {
            tape.write(i as f32);
        }
        tape
    }

    #[test]
    fn test_read_head_creation() {
        let rh = ReadHead::<f32, 64>::new();
        assert!((rh.delay - 0.5).abs() < 1e-6);
    }

    #[test]
    fn read_head_integer_delay_reads_exact_samples() {
        let tape = ramp_tape(40);
        let mut rh = ReadHead::<f32, 4>::new();
        rh.set_delay(0.1);
        rh.init(100.0);
        let (_writer, reader) = tape_handles(tape);
        rh.set_reader(reader);

        let mut out = [0.0f32; 4];
        rh.process(None, &mut out).unwrap();
        assert_eq!(out[0], 26.0);
        assert_eq!(out[3], 29.0);
    }

    #[test]
    fn read_head_fractional_delay_interpolates() {
        let tape = ramp_tape(40);
        let mut rh = ReadHead::<f32, 4>::new();
        rh.set_delay(0.105);
        rh.init(100.0);
        let (_writer, reader) = tape_handles(tape);
        rh.set_reader(reader);

        let mut out = [0.0f32; 4];
        rh.process(None, &mut out).unwrap();
        assert!(out[0] > 25.0 && out[0] < 26.0);
        assert!((out[0] - 25.5).abs() < 0.01, "got {}", out[0]);
    }
}
