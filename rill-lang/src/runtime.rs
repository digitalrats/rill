//! Runtime — launches backends wired to a [`ProgramRunner`].
//!
//! Thin glue that registers a process callback on a driver, connecting
//! an [`IoCapture`] (optional) and [`IoPlayback`] (optional) to a program or a
//! compiled stream.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::traits::MultichannelAlgorithm;

use crate::graph::CompiledStream;
use crate::program_runner::ProgramRunner;

/// Stateless launcher — wires backends to a program and starts the driver.
pub struct Runtime;

impl Runtime {
    /// Wire capture → program → playback inside a process callback on the
    /// driver, then call [`IoDriver::run`]. Blocks until the driver stops.
    ///
    /// `capture` and `playback` are optional — use [`NullBackend`](rill_core::io::NullBackend)
    /// to fill the unused direction.
    pub fn launch<const BUF: usize>(
        driver: Arc<dyn IoDriver>,
        capture: Option<Arc<dyn IoCapture>>,
        playback: Option<Arc<dyn IoPlayback>>,
        mut program: ProgramRunner<BUF>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let (cap, pb) = (capture.clone(), playback.clone());
        let mut in_buf = [0.0f32; BUF];
        let mut out_buf = [0.0f32; BUF];
        driver.set_callback(Box::new(move |tick| {
            let n = tick.samples_since_last as usize;
            let input: &[&[f32]] = if let Some(ref c) = cap {
                c.read_input(0, &mut in_buf[..n]);
                &[&in_buf[..n]]
            } else {
                &[]
            };
            program.apply(input, &mut [&mut out_buf[..n]], tick);
            if let Some(ref p) = pb {
                p.write_output(0, &out_buf[..n]);
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
