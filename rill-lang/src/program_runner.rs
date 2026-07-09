//! ProgramRunner — signal-thread executor for flat RillProgram graphs.
//!
//! Replaces `ProcessingState` for DSL-compiled programs. Instead of a DAG
//! graph with recursive `Port::propagate()`, it runs a flat `RillProgram`
//! per output channel inside the I/O backend callback.
//!
//! # Safety
//!
//! `!Send + !Sync` (via `PhantomData<*const ()>`) — must stay on the I/O
//! callback thread.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::queues::CommandEnum;
use rill_core::time::ClockTick;
use rill_core::traits::Algorithm;
use rill_core_actor::ActorRef;

use crate::graph_engine::RillGraphEngine;

/// Thin wrapper that runs a flat rill-lang program inside an I/O callback.
pub struct ProgramRunner {
    engine: RillGraphEngine<f32>,
    parent_ref: Option<ActorRef<CommandEnum>>,
    capture: Option<Arc<dyn IoCapture>>,
    playback: Option<Arc<dyn IoPlayback>>,
    input_buf: Vec<f32>,
    output_buf: Vec<f32>,
    max_block_size: usize,
    _not_send_sync: PhantomData<*const ()>,
}

impl ProgramRunner {
    /// Create a new runner wrapping a compiled graph engine.
    #[allow(missing_docs)]
    pub fn new(
        engine: RillGraphEngine<f32>,
        parent_ref: Option<ActorRef<CommandEnum>>,
        max_block_size: usize,
    ) -> Self {
        Self {
            engine,
            parent_ref,
            capture: None,
            playback: None,
            input_buf: vec![0.0f32; max_block_size],
            output_buf: vec![0.0f32; max_block_size],
            max_block_size,
            _not_send_sync: PhantomData,
        }
    }

    /// Wire I/O capture and playback backends into the runner.
    ///
    /// Must be called before `run_with_driver()`.
    pub fn wire_backends(
        &mut self,
        capture: Option<Arc<dyn IoCapture>>,
        playback: Option<Arc<dyn IoPlayback>>,
    ) {
        self.capture = capture;
        self.playback = playback;
    }

    /// Handle for sending `SetParameter` commands from control threads.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.engine.handle()
    }

    /// Reference to the underlying compiled engine.
    pub fn engine(&self) -> &RillGraphEngine<f32> {
        &self.engine
    }

    fn process_tick(&mut self, tick: &ClockTick) {
        let block_size = tick.samples_since_last as usize;
        assert!(
            block_size <= self.max_block_size,
            "block size {block_size} exceeds max {}",
            self.max_block_size
        );

        let num_outputs = self
            .playback
            .as_ref()
            .map(|p| p.num_output_channels())
            .unwrap_or(1);

        let chunk_end = tick.sample_pos + tick.samples_since_last as u64;
        self.engine.apply_due_params(chunk_end);

        for ch in 0..num_outputs {
            let in_slice = &mut self.input_buf[..block_size];
            let out_slice = &mut self.output_buf[..block_size];

            if let Some(ref cap) = self.capture {
                cap.read_input(ch, in_slice);
            }

            let _ = self.engine.process(
                if self.capture.is_some() {
                    Some(in_slice as &[f32])
                } else {
                    None
                },
                out_slice,
            );
            if let Some(ref pb) = self.playback {
                pb.write_output(ch, out_slice);
            }
        }

        if tick.is_final {
            if let Some(ref parent) = self.parent_ref {
                parent.send(CommandEnum::ClockTick(tick.clone()));
            }
        }
    }

    /// Enter the I/O lifecycle.
    ///
    /// Registers a process callback on the driver and runs the driver loop.
    pub fn run_with_driver(
        mut self,
        driver: Arc<dyn IoDriver>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        driver.set_process_callback(Box::new(move |tick: &ClockTick| {
            self.process_tick(tick);
        }));
        driver.run(running.clone())?;
        while running.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = driver.stop();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_runner_ticks_without_crash() {
        use crate::builtin::Registry;
        use crate::compile_graph;

        let engine = compile_graph::<f32>("main = _", &Registry::new(), 44100.0).unwrap();
        let mut runner = ProgramRunner::new(engine, None, 128);

        let tick = ClockTick {
            sample_pos: 0,
            samples_since_last: 64,
            sample_rate: 44100.0,
            speed_ratio: 1.0,
            io_quantum: 64,
            source: "test".into(),
            is_new_block: true,
            is_final: false,
            tempo: None,
        };

        runner.process_tick(&tick);
    }

    #[test]
    fn process_tick_gain_produces_finite_output() {
        use crate::builtin::Registry;
        use crate::compile_graph;

        let engine = compile_graph::<f32>("main = _ * 0.5", &Registry::new(), 44100.0).unwrap();
        let mut runner = ProgramRunner::new(engine, None, 64);

        let tick = ClockTick {
            sample_pos: 0,
            samples_since_last: 4,
            sample_rate: 44100.0,
            speed_ratio: 1.0,
            io_quantum: 4,
            source: "test".into(),
            is_new_block: true,
            is_final: true,
            tempo: None,
        };

        runner.process_tick(&tick);

        for v in &runner.output_buf[..4] {
            assert!(v.is_finite());
        }
    }
}
