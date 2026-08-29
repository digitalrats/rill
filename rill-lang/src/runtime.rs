//! Runtime — launches backends wired to a [`ProgramRunner`].
//!
//! Thin glue that registers a process callback on a driver, connecting
//! an [`IoCapture`] (optional) and [`IoPlayback`] (optional) to a
//! [`ProgramRunner::apply`](crate::program_runner::ProgramRunner::apply)
//! call on every tick.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_core::io::{IoCapture, IoDriver, IoPlayback};

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
        mut program: ProgramRunner,
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
}
