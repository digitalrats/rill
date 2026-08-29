//! The tape as a passive backend: an in-memory ring buffer written by a write
//! head and read by read heads. Declaratively described by [`TapeBackendSpec`].

use crate::tape::read_head::ReadHead;
use crate::tape::write_head::WriteHead;
use rill_core::buffer::{shared_handles, DelayBuffer, TapeLoop};
use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;

/// Configuration for the write head of a tape backend.
#[derive(Debug, Clone)]
pub struct WriteHeadConfig {
    /// Feedback amount (0.0–0.99).
    pub feedback: f64,
}

impl Default for WriteHeadConfig {
    fn default() -> Self {
        Self { feedback: 0.3 }
    }
}

/// Configuration for one read head (tap) of a tape backend.
#[derive(Debug, Clone)]
pub struct ReadHeadConfig {
    /// Read delay in seconds.
    pub delay: f64,
}

impl Default for ReadHeadConfig {
    fn default() -> Self {
        Self { delay: 0.1 }
    }
}

/// Declarative description of a tape backend.
#[derive(Debug, Clone)]
pub struct TapeBackendSpec {
    /// Backend name.
    pub name: String,
    /// Tape capacity in samples.
    pub capacity: usize,
    /// Write head configuration.
    pub write: WriteHeadConfig,
    /// Per-tap read head configurations.
    pub reads: Vec<ReadHeadConfig>,
}

/// Instantiated passive tape backend.
///
/// The tape lives in a shared cell handed out as one unique [`SharedWriter`]
/// (inside the write head) and a cloned [`SharedReader`] per read head, so every
/// head operates on the same single-threaded `TapeLoop`.
pub struct TapeBackend<T: Transcendental> {
    write: WriteHead<T, 256>,
    reads: Vec<ReadHead<T, 256>>,
}

impl<T: Transcendental> TapeBackend<T> {
    /// Create from a spec at the given sample rate.
    pub fn new(spec: &TapeBackendSpec, sample_rate: f32) -> Self {
        let tape = TapeLoop::<T>::new(spec.capacity).expect("non-zero capacity");
        let (writer, reader) = shared_handles(Box::new(tape) as Box<dyn DelayBuffer<T>>);
        let mut write = WriteHead::<T, 256>::new(sample_rate);
        write.set_feedback(spec.write.feedback as f32);
        write.set_writer(writer);
        let reads = spec
            .reads
            .iter()
            .map(|r| {
                let mut h = ReadHead::<T, 256>::new();
                h.set_delay(r.delay as f32);
                h.init(sample_rate);
                h.set_reader(reader.clone());
                h
            })
            .collect();
        Self { write, reads }
    }

    /// Write one block (the recording pass's record signal) as
    /// `dry + write.feedback·fb` straight to the tape.
    ///
    /// RT-safe: no per-call allocation — the mix is written directly to the
    /// tape through the write head.
    pub fn write_block(&mut self, dry: &[T], fb: &[T]) {
        self.write.write_block(dry, fb);
    }

    /// Read one block per read head (the playback pass's tap inputs).
    pub fn read_blocks(&mut self, outs: &mut [&mut [T]]) {
        for (head, out) in self.reads.iter_mut().zip(outs.iter_mut()) {
            let _ = Algorithm::process(head, None, out);
        }
    }
}
