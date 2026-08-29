use rill_core::{
    buffer::TapeWriter,
    math::Transcendental,
    traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata},
    traits::{MultichannelAlgorithm, ProcessResult},
};

#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Send for WriteHead<T, B> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Sync for WriteHead<T, B> {}

/// Tape write head for delay-based tape effects with feedback control.
pub struct WriteHead<T: Transcendental, const BUF_SIZE: usize> {
    tape: Option<TapeWriter<T>>,
    delay_time: f32,
    feedback: f32,
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> WriteHead<T, BUF_SIZE> {
    /// Creates a new write head with default settings.
    pub fn new(sample_rate: f32) -> Self {
        Self::with_resource(sample_rate, "tape_0")
    }

    /// Creates a new write head with a named tape resource.
    pub fn with_resource(sample_rate: f32, _resource_name: &str) -> Self {
        Self {
            tape: None,
            delay_time: 0.5,
            feedback: 0.3,
            sample_rate,
        }
    }

    /// Sets the write delay time in seconds (clamped to 0.01–2.0).
    pub fn set_delay_time(&mut self, time: f32) {
        self.delay_time = time.clamp(0.01, 2.0);
    }

    /// Sets feedback amount (clamped to 0.0–0.99).
    pub fn set_feedback(&mut self, fb: f32) {
        self.feedback = fb.clamp(0.0, 0.99);
    }

    /// Sets the tape writer to write to.
    pub fn set_writer(&mut self, writer: TapeWriter<T>) {
        self.tape = Some(writer);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for WriteHead<T, BUF_SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {}

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len());
                output[..n].copy_from_slice(&inp[..n]);
                output[n..].fill(T::ZERO);
                if let Some(tape) = self.tape.as_mut() {
                    for &sample in inp.iter() {
                        tape.write(sample);
                    }
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "WriteHead",
            category: AlgorithmCategory::Effect,
            description: "Tape write head for tape loop effects",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> rill_core::builtin::BlockBuiltin<T>
    for WriteHead<T, BUF_SIZE>
{
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = value.as_f32().unwrap_or(0.0);
        match index {
            0 => self.set_delay_time(v),
            1 => self.set_feedback(v),
            _ => {}
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> MultichannelAlgorithm<T> for WriteHead<T, BUF_SIZE> {
    fn num_inputs(&self) -> usize {
        2
    }

    fn num_outputs(&self) -> usize {
        1
    }

    /// Inputs are `[dry, feedback]`; the mixed signal `dry + feedback * feedback`
    /// is written to the tape and passed through to the output.
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let dry = inputs.first().copied().unwrap_or(&[]);
        let fb = inputs.get(1).copied().unwrap_or(&[]);
        let g = T::from_f32(self.feedback);
        if let Some(out) = outputs.first_mut() {
            let n = out.len();
            for i in 0..n {
                let d = dry.get(i).copied().unwrap_or(T::ZERO);
                let f = fb.get(i).copied().unwrap_or(T::ZERO);
                let mixed = d + f * g;
                out[i] = mixed;
                if let Some(tape) = self.tape.as_mut() {
                    tape.write(mixed);
                }
            }
        }
        Ok(())
    }

    fn reset(&mut self) {}
}

impl<T: Transcendental, const BUF_SIZE: usize> rill_core::builtin::MultichannelBlockBuiltin<T>
    for WriteHead<T, BUF_SIZE>
{
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = value.as_f32().unwrap_or(0.0);
        match index {
            0 => self.set_delay_time(v),
            1 => self.set_feedback(v),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_head_creation() {
        let wh = WriteHead::<f32, 64>::new(44100.0);
        assert!((wh.delay_time - 0.5).abs() < 1e-6);
        assert!((wh.feedback - 0.3).abs() < 1e-6);
    }
}
