//! Multichannel runtime test: `Runtime::launch` must drive **all** backend
//! channels, not just channel 0.
//!
//! A 2→2 program (`main = _ , _` — identity on both channels) is processed
//! through a fake capture/playback that each expose 2 channels. Each output
//! channel must receive the per-channel data read from the matching input
//! channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_core::io::{IoCapture, IoDriver, IoPlayback, IoResult};
use rill_core::time::ClockTick;
use rill_lang::builtin::Registry;
use rill_lang::compile_graph;
use rill_lang::program_runner::ProgramRunner;
use rill_lang::runtime::Runtime;

const BUF: usize = 256;
const ITERS: u64 = 4;

// ── mock I/O ────────────────────────────────────────────────────────────────

/// Capture fake: channel 0 yields constant `1.0`, channel 1 yields `2.0`.
struct FakeCapture {
    channels: usize,
}

impl IoCapture for FakeCapture {
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        dst.fill(if channel == 0 { 1.0 } else { 2.0 });
        dst.len()
    }

    fn num_input_channels(&self) -> usize {
        self.channels
    }
}

/// Playback fake: records everything written to each channel.
#[derive(Default)]
struct FakePlayback {
    channels: usize,
    recorded: Mutex<Vec<Vec<f32>>>,
}

impl FakePlayback {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            recorded: Mutex::new(vec![Vec::new(); channels]),
        }
    }
}

impl IoPlayback for FakePlayback {
    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        self.recorded.lock().unwrap()[channel].extend_from_slice(src);
        src.len()
    }

    fn num_output_channels(&self) -> usize {
        self.channels
    }
}

/// The process callback type the driver stores.
type ProcessCallback = Box<dyn FnMut(&ClockTick)>;

/// Driver fake: stores the process callback and fires it `iters` times.
///
/// The callback type is not `Send`, so the struct carries it behind a `Mutex`
/// and asserts `Send + Sync` (matching `IoDriver`'s supertraits) manually — the
/// same assertion the existing `duplex_runtime.rs` test makes.
struct FakeDriver {
    process: Mutex<Option<ProcessCallback>>,
    iters: u64,
}

// SAFETY: the callback is only ever invoked from `run` on a single thread
// (the test thread); the `Mutex` guards storage. This mirrors the unsafe
// `Send + Sync` assertion in `duplex_runtime.rs`.
unsafe impl Send for FakeDriver {}
// SAFETY: see `Send` impl above.
unsafe impl Sync for FakeDriver {}

impl FakeDriver {
    fn new(iters: u64) -> Self {
        Self {
            process: Mutex::new(None),
            iters,
        }
    }
}

impl IoDriver for FakeDriver {
    fn set_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        *self.process.lock().unwrap() = Some(cb);
    }

    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()> {
        for i in 0..self.iters {
            let tick = ClockTick::new(i * BUF as u64, BUF as u32, 44100.0, "mock".into());
            if let Some(cb) = self.process.lock().unwrap().as_mut() {
                cb(&tick);
            }
        }
        running.store(false, Ordering::Release);
        Ok(())
    }

    fn stop(&self) -> IoResult<()> {
        Ok(())
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[test]
fn launch_drives_all_channels() {
    let engine = compile_graph::<f32, BUF>("main = _ , _", &Registry::new(), 44100.0).unwrap();
    let runner = ProgramRunner::new(engine, None);

    let capture = Arc::new(FakeCapture { channels: 2 });
    let playback = Arc::new(FakePlayback::new(2));
    let driver = Arc::new(FakeDriver::new(ITERS));
    let running = Arc::new(AtomicBool::new(true));

    Runtime::launch::<BUF>(
        driver,
        Some(capture),
        Some(playback.clone()),
        runner,
        running,
    )
    .unwrap();

    let expected_len = (ITERS * BUF as u64) as usize;
    let recorded = playback.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 2, "both output channels must be written");
    assert_eq!(
        recorded[0].len(),
        expected_len,
        "channel 0 receives every tick"
    );
    assert_eq!(
        recorded[1].len(),
        expected_len,
        "channel 1 receives every tick"
    );
    assert!(
        recorded[0].iter().all(|&v| v == 1.0),
        "channel 0 must carry capture channel 0 data"
    );
    assert!(
        recorded[1].iter().all(|&v| v == 2.0),
        "channel 1 must carry capture channel 1 data"
    );
}

#[test]
fn launch_rejects_channel_count_mismatch() {
    let engine = compile_graph::<f32, BUF>("main = _ , _", &Registry::new(), 44100.0).unwrap();
    let runner = ProgramRunner::new(engine, None);

    // Mono capture behind a 2→2 program: must be rejected, not silently zero-filled.
    let capture = Arc::new(FakeCapture { channels: 1 });
    let playback = Arc::new(FakePlayback::new(2));
    let driver = Arc::new(FakeDriver::new(1));
    let running = Arc::new(AtomicBool::new(true));

    let err =
        Runtime::launch::<BUF>(driver, Some(capture), Some(playback), runner, running).unwrap_err();
    assert!(
        err.contains("capture") && err.contains("1"),
        "mismatch error should mention the offending channel count: {err}"
    );
}
