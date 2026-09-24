//! Runtime — launches backends wired to a [`ProgramRunner`].
//!
//! Thin glue that registers a process callback on a driver, connecting
//! an [`IoCapture`] (optional) and [`IoPlayback`] (optional) to a program or a
//! compiled stream.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_core::buffer::FixedBuffer;
use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::traits::MultichannelAlgorithm;

use crate::graph::CompiledStream;
use crate::program_runner::ProgramRunner;

/// Maximum number of channels `launch` drives per direction.
///
/// Pre-allocated channel buffers are bounded by this constant; a backend
/// exposing more channels is rejected at launch.
pub const MAX_CHANNELS: usize = 8;

/// Stateless launcher — wires backends to a program and starts the driver.
pub struct Runtime;

impl Runtime {
    /// Wire capture → program → playback inside a process callback on the
    /// driver, then call [`IoDriver::run`]. Blocks until the driver stops.
    ///
    /// Every backend channel is driven: each tick reads `num_input_channels()`
    /// from `capture` and writes `num_output_channels()` from `playback`. The
    /// backend channel counts must match the program's arity
    /// ([`ProgramRunner::num_inputs`]/[`num_outputs`]) — a mismatch is
    /// rejected before the driver starts.
    ///
    /// `capture` and `playback` are optional — use [`NullBackend`](rill_core::io::NullBackend)
    /// to fill the unused direction. Channel buffers are pre-allocated before
    /// [`IoDriver::run`]; the callback performs no heap allocation.
    pub fn launch<const BUF: usize>(
        driver: Arc<dyn IoDriver>,
        capture: Option<Arc<dyn IoCapture>>,
        playback: Option<Arc<dyn IoPlayback>>,
        mut program: ProgramRunner<BUF>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let (cap, pb) = (capture.clone(), playback.clone());
        let n_in = cap.as_ref().map(|c| c.num_input_channels()).unwrap_or(0);
        let n_out = pb.as_ref().map(|p| p.num_output_channels()).unwrap_or(0);
        if n_in != program.num_inputs() || n_out != program.num_outputs() {
            return Err(format!(
                "channel count mismatch: capture exposes {n_in} input(s), program expects {}; \
                 playback exposes {n_out} output(s), program expects {}",
                program.num_inputs(),
                program.num_outputs(),
            ));
        }
        if n_in > MAX_CHANNELS || n_out > MAX_CHANNELS {
            return Err(format!(
                "channel count {n_in}->{n_out} exceeds MAX_CHANNELS ({MAX_CHANNELS})"
            ));
        }

        // Pre-allocated per-channel buffers — no heap allocation in the callback.
        let mut in_bufs: [FixedBuffer<f32, BUF>; MAX_CHANNELS] =
            std::array::from_fn(|_| FixedBuffer::new());
        let mut out_bufs: [FixedBuffer<f32, BUF>; MAX_CHANNELS] =
            std::array::from_fn(|_| FixedBuffer::new());

        driver.set_callback(Box::new(move |tick| {
            let n = tick.samples_since_last as usize;
            if let Some(ref cptr) = cap {
                for (c, buf) in in_bufs.iter_mut().take(n_in).enumerate() {
                    cptr.read_input(c, &mut buf[..n]);
                }
            }
            let mut in_iter = in_bufs.iter().map(|b| &b[..n]);
            let input_slices: [&[f32]; MAX_CHANNELS] =
                std::array::from_fn(|_| in_iter.next().unwrap());
            let mut out_iter = out_bufs.iter_mut().map(|b| &mut b[..n]);
            let mut output_slices: [&mut [f32]; MAX_CHANNELS] =
                std::array::from_fn(|_| out_iter.next().unwrap());
            program.apply(&input_slices[..n_in], &mut output_slices[..n_out], tick);
            if let Some(ref p) = pb {
                for (c, buf) in out_bufs.iter().take(n_out).enumerate() {
                    p.write_output(c, &buf[..n]);
                }
            }
        }));
        driver.run(running)?;
        let _ = driver.stop();
        Ok(())
    }

    /// Launch a compiled stream. A plain graph uses one callback
    /// (`capture → program → playback`); a duplex tape echo uses a two-pass
    /// callback: recording (`[capture, fb] → [dryL, dryR]`, the internal
    /// `write_head` builtin writes the shared tape) then playback
    /// (`[dryL, dryR] → [fb, out]`, the internal `read_head` builtins read the
    /// shared tape; `fb` is shadowed one tick).
    pub fn launch_stream<const BUF: usize>(
        driver: Arc<dyn IoDriver>,
        capture: Option<Arc<dyn IoCapture>>,
        playback: Option<Arc<dyn IoPlayback>>,
        stream: CompiledStream<f32, BUF>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let (cap, pb) = (capture, playback);
        match stream {
            CompiledStream::Single(engine) => {
                let runner = ProgramRunner::<BUF>::new(engine, None);
                Self::launch::<BUF>(driver, cap, pb, runner, running)
            }
            CompiledStream::Duplex {
                mut recording,
                mut playback,
                ..
            } => {
                let mut dry = [vec![0.0f32; BUF], vec![0.0f32; BUF]]; // dryL, dryR
                let mut fb = [0.0f32; BUF];
                driver.set_callback(Box::new(move |tick| {
                    let n = tick.samples_since_last as usize;
                    let mut cap_buf = [0.0f32; BUF];
                    if let Some(ref c) = cap {
                        c.read_input(0, &mut cap_buf[..n]);
                    }
                    // recording pass: [capture, fb] -> [record, dryL, dryR]
                    let mut rec_out = [vec![0.0f32; BUF], vec![0.0f32; BUF], vec![0.0f32; BUF]];
                    let mut ro: Vec<&mut [f32]> = rec_out.iter_mut().map(|v| &mut v[..n]).collect();
                    let _ = MultichannelAlgorithm::process(
                        &mut recording,
                        &[&cap_buf[..n], &fb[..n]],
                        &mut ro,
                    );
                    for (d, r) in dry.iter_mut().zip(rec_out.iter().skip(1)) {
                        d[..n].copy_from_slice(&r[..n]);
                    }
                    // playback pass: [dryL, dryR] -> [fb, outL, outR]
                    let dry_refs: Vec<&[f32]> = dry.iter().map(|d| &d[..n]).collect();
                    let mut pb_out = [vec![0.0f32; BUF], vec![0.0f32; BUF], vec![0.0f32; BUF]];
                    let mut po: Vec<&mut [f32]> = pb_out.iter_mut().map(|v| &mut v[..n]).collect();
                    let _ = MultichannelAlgorithm::process(&mut playback, &dry_refs, &mut po);
                    fb[..n].copy_from_slice(&po[0][..n]);
                    if let Some(ref p) = pb {
                        p.write_output(0, &po[1][..n]);
                        if p.num_output_channels() > 1 {
                            p.write_output(1, &po[2][..n]);
                        }
                    }
                }));
                driver.run(running)?;
                let _ = driver.stop();
                Ok(())
            }
        }
    }
}
