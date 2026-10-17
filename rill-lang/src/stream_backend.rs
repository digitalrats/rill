//! # Stream backends — the runtime contract behind `mkStream`
//!
//! A stream backend is the program-facing I/O of the new execution model.
//! It is **single-threaded by design**: warm setup creates it (the program's
//! `main` thread, via `mkStream`), and the `runStream` hot loop — which is the
//! only code that touches it — runs on the master backend's tick (for real
//! backends, the audio thread). It is never accessed concurrently, so no
//! `Send + Sync` machinery is required: the interior uses [`RefCell`], no
//! locks, no `unsafe` (the crate denies it). Backends are shared by
//! [`Rc`](std::rc::Rc) (not `Arc`) — the registry and the test/executor hold
//! clones, all on one thread; `Arc` would mislead (it implies the thread
//! safety this contract deliberately does not provide).
//!
//! The method shape mirrors the existing `rill_core::io` channel contract
//! (`read_input(channel, dst)`, `write_output(channel, src)`, per-direction
//! channel counts — the same signatures as [`IoCapture`](rill_core::io::IoCapture)
//! /[`IoPlayback`](rill_core::io::IoPlayback)). The `rill-io` backends
//! (ALSA/PipeWire/JACK/PortAudio) are adapted in the follow-on plan by a
//! wrapper that calls their capture/playback from the hot-loop thread; the
//! in-memory backend below implements the same channel shape directly. An
//! [`ActiveSide`] records which side is the active clock (the `runStream`
//! master).
//!
//! `mkStream` (Task 3) resolves a [`BackendSpec`] against the program's
//! [`BackendRegistry`], which creates the stream pair: `(master, slave)` —
//! the first element is the backend's active side (the `runStream` master),
//! the second the passive side (the slave, serviced within the master's tick).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Which side of a stream backend is the active clock (the `runStream`
/// master — the side whose backend drives the tick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveSide {
    /// The input (capture) side drives the tick.
    Input,
    /// The output (playback) side drives the tick.
    Output,
}

/// Direction of a stream side, used to validate `asInput`/`asOutput`
/// projections against the stream's actual direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDirection {
    /// An input (capture) side — `asInput` is valid, `asOutput` is not.
    Input,
    /// An output (playback) side — `asOutput` is valid, `asInput` is not.
    Output,
}

/// A backend spec: identifies a backend in the runtime [`BackendRegistry`]
/// by name. Programs construct the rill-lang `BackendSpec` value
/// (`{ name: "test" }`); the executor maps it to this runtime spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSpec {
    /// The registered backend name.
    pub name: String,
}

impl BackendSpec {
    /// A spec naming `name`.
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// A program-facing stream value: a handle to one side of a backend in the
/// [`BackendRegistry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamValue {
    /// Index of the backend entry in the [`BackendRegistry`].
    pub backend: usize,
    /// The direction of this side.
    pub direction: StreamDirection,
}

/// In-memory stream backend for tests: a fillable input (capture) side and a
/// capturable output (playback) side, plus the active side (master).
///
/// Single-threaded (see the module docs): the interior is [`RefCell`]-backed
/// — no locks, no `unsafe`. The channel methods mirror the `rill_core::io`
/// contract (`read_input`/`write_output` per channel), so a real backend
/// adapter (follow-on plan) is one-to-one. Tests fill the input side with
/// [`Self::push_input`] and read the captured output with
/// [`Self::captured_output`].
pub struct InMemoryBackend {
    name: String,
    input_channels: usize,
    output_channels: usize,
    master_side: ActiveSide,
    /// Pending input samples, one `VecDeque` per input channel (pop = read).
    input: RefCell<Vec<VecDeque<f32>>>,
    /// Captured output samples, one `Vec` per output channel (append = write).
    output: RefCell<Vec<Vec<f32>>>,
}

impl InMemoryBackend {
    /// Create an in-memory backend with `input_channels` capture and
    /// `output_channels` playback channels. `master_side` records which side
    /// is the active clock (the `runStream` master).
    pub fn new(
        name: impl Into<String>,
        input_channels: usize,
        output_channels: usize,
        master_side: ActiveSide,
    ) -> Self {
        Self {
            name: name.into(),
            input_channels,
            output_channels,
            master_side,
            input: RefCell::new(vec![VecDeque::new(); input_channels]),
            output: RefCell::new(vec![Vec::new(); output_channels]),
        }
    }

    /// The backend's registered name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Which side is the active clock (the `runStream` master).
    pub fn master_side(&self) -> ActiveSide {
        self.master_side
    }

    /// Number of capture channels.
    pub fn num_input_channels(&self) -> usize {
        self.input_channels
    }

    /// Number of playback channels.
    pub fn num_output_channels(&self) -> usize {
        self.output_channels
    }

    /// Read captured input samples for one channel into `dst`.
    ///
    /// Consumes up to `dst.len()` pending samples for `channel`; returns the
    /// number read (may be less when the queue is short). Mirrors
    /// [`IoCapture::read_input`](rill_core::io::IoCapture::read_input).
    pub fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        let mut input = self.input.borrow_mut();
        let Some(queue) = input.get_mut(channel) else {
            return 0;
        };
        let n = dst.len().min(queue.len());
        for (dst_sample, queue_sample) in dst.iter_mut().take(n).zip(queue.drain(..n)) {
            *dst_sample = queue_sample;
        }
        n
    }

    /// Write output samples for one channel from `src`.
    ///
    /// Appends `src` to the captured output for `channel`; returns the number
    /// written. Mirrors [`IoPlayback::write_output`](rill_core::io::IoPlayback::write_output).
    pub fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        let mut output = self.output.borrow_mut();
        let Some(buf) = output.get_mut(channel) else {
            return 0;
        };
        buf.extend_from_slice(src);
        src.len()
    }

    /// Fill the input side of `channel` with `samples` (appended to the
    /// pending input; the test pushes before driving ticks).
    pub fn push_input(&self, channel: usize, samples: &[f32]) {
        self.input.borrow_mut()[channel].extend(samples.iter().copied());
    }

    /// The captured output of `channel` so far (cloned; the test reads after
    /// driving ticks).
    pub fn captured_output(&self, channel: usize) -> Vec<f32> {
        self.output.borrow()[channel].clone()
    }

    /// Reset both sides: drop pending input and captured output.
    pub fn clear(&self) {
        for queue in self.input.borrow_mut().iter_mut() {
            queue.clear();
        }
        for buf in self.output.borrow_mut().iter_mut() {
            buf.clear();
        }
    }
}

/// A registered backend entry.
struct BackendEntry {
    name: String,
    backend: Rc<InMemoryBackend>,
}

/// The runtime backend registry: named stream backends that `mkStream`
/// resolves against. Lives in the program's runtime `State` (Task 3). Tests
/// seed it with in-memory backends and read created streams back by name.
#[derive(Default)]
pub struct BackendRegistry {
    entries: Vec<BackendEntry>,
}

impl BackendRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `backend` under `name`. Fails if the name is already taken.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        backend: Rc<InMemoryBackend>,
    ) -> Result<(), String> {
        let name = name.into();
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("backend '{name}' already registered"));
        }
        self.entries.push(BackendEntry { name, backend });
        Ok(())
    }

    /// The registered backend for `name`, if any (test access).
    pub fn get(&self, name: &str) -> Option<&Rc<InMemoryBackend>> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.backend)
    }

    /// The registered backend for a [`StreamValue`], if the handle is valid.
    pub fn backend(&self, value: StreamValue) -> Option<&Rc<InMemoryBackend>> {
        self.entries.get(value.backend).map(|e| &e.backend)
    }

    /// Resolve `spec` against the registry and create the backend's stream
    /// pair. The pair is ordered `(master, slave)`: the first element is the
    /// backend's active side (the `runStream` master), the second the passive
    /// side. This is the runtime side of `mkStream`.
    pub fn open(&self, spec: &BackendSpec) -> Result<(StreamValue, StreamValue), String> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.name == spec.name)
            .ok_or_else(|| format!("backend '{}' not found in the registry", spec.name))?;
        let master = match self.entries[idx].backend.master_side {
            ActiveSide::Input => StreamValue {
                backend: idx,
                direction: StreamDirection::Input,
            },
            ActiveSide::Output => StreamValue {
                backend: idx,
                direction: StreamDirection::Output,
            },
        };
        let slave = StreamValue {
            backend: idx,
            direction: match master.direction {
                StreamDirection::Input => StreamDirection::Output,
                StreamDirection::Output => StreamDirection::Input,
            },
        };
        Ok((master, slave))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_input_then_read_input_consumes() {
        let backend = InMemoryBackend::new("test", 2, 2, ActiveSide::Input);
        backend.push_input(0, &[1.0, 2.0, 3.0, 4.0]);
        let mut dst = [0.0f32; 2];
        let n = backend.read_input(0, &mut dst);
        assert_eq!(n, 2);
        assert_eq!(dst, [1.0, 2.0]);
        // Remaining samples stay queued for the next tick.
        let n2 = backend.read_input(0, &mut dst);
        assert_eq!(n2, 2);
        assert_eq!(dst, [3.0, 4.0]);
    }

    #[test]
    fn read_input_beyond_available_is_bounded() {
        let backend = InMemoryBackend::new("test", 1, 1, ActiveSide::Input);
        backend.push_input(0, &[7.0]);
        let mut dst = [0.0f32; 4];
        let n = backend.read_input(0, &mut dst);
        assert_eq!(n, 1);
        assert_eq!(dst[0], 7.0);
    }

    #[test]
    fn write_output_captures() {
        let backend = InMemoryBackend::new("test", 1, 2, ActiveSide::Input);
        let n = backend.write_output(1, &[0.5, 0.25]);
        assert_eq!(n, 2);
        assert_eq!(backend.captured_output(1), [0.5, 0.25]);
        backend.write_output(1, &[1.0]);
        assert_eq!(backend.captured_output(1), [0.5, 0.25, 1.0]);
    }

    #[test]
    fn registry_open_orders_master_slave_by_active_side() {
        let mut registry = BackendRegistry::new();
        registry
            .register(
                "in",
                Rc::new(InMemoryBackend::new("in", 2, 2, ActiveSide::Input)),
            )
            .unwrap();
        registry
            .register(
                "out",
                Rc::new(InMemoryBackend::new("out", 2, 2, ActiveSide::Output)),
            )
            .unwrap();

        let (m, s) = registry.open(&BackendSpec::named("in")).unwrap();
        assert_eq!(m.direction, StreamDirection::Input);
        assert_eq!(s.direction, StreamDirection::Output);

        let (m, s) = registry.open(&BackendSpec::named("out")).unwrap();
        assert_eq!(m.direction, StreamDirection::Output);
        assert_eq!(s.direction, StreamDirection::Input);
    }

    #[test]
    fn registry_open_unknown_spec_is_error() {
        let registry = BackendRegistry::new();
        assert!(registry.open(&BackendSpec::named("missing")).is_err());
    }

    #[test]
    fn registry_rejects_duplicate_names() {
        let mut registry = BackendRegistry::new();
        registry
            .register(
                "dup",
                Rc::new(InMemoryBackend::new("dup", 1, 1, ActiveSide::Input)),
            )
            .unwrap();
        assert!(registry
            .register(
                "dup",
                Rc::new(InMemoryBackend::new("dup", 1, 1, ActiveSide::Input))
            )
            .is_err());
    }
}
