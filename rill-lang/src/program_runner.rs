//! ProgramRunner — pure signal transform for DSL-compiled programs.
//!
//! Thin wrapper around a compiled graph engine. It has no I/O knowledge —
//! the caller feeds input buffers and receives output buffers.
//!
//! # Safety
//!
//! `!Send + !Sync` (via `PhantomData<*const ()>`) — must stay on the I/O
//! callback thread.

use std::marker::PhantomData;

use rill_core::queues::CommandEnum;
use rill_core::time::ClockTick;
use rill_core_actor::ActorRef;

use crate::program_engine::ProgramEngine;

/// Pure signal transform: `&[&[f32]]` → `&mut [&mut [f32]]`.
///
/// The caller is responsible for wiring I/O backends and managing the
/// driver lifecycle. Use [`apply`](ProgramRunner::apply) inside the
/// process callback set on the driver.
pub struct ProgramRunner {
    engine: ProgramEngine<f32>,
    parent_ref: Option<ActorRef<CommandEnum>>,
    _not_send_sync: PhantomData<*const ()>,
}

impl ProgramRunner {
    /// Create a new runner wrapping a compiled program engine.
    pub fn new(engine: ProgramEngine<f32>, parent_ref: Option<ActorRef<CommandEnum>>) -> Self {
        Self {
            engine,
            parent_ref,
            _not_send_sync: PhantomData,
        }
    }

    /// Handle for sending `SetParameter` commands from control threads.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.engine.handle()
    }

    /// Reference to the underlying program engine.
    pub fn engine(&self) -> &ProgramEngine<f32> {
        &self.engine
    }

    /// Process one tick: transform `inputs` into `outputs`.
    ///
    /// Called from the driver's process callback. The caller provides
    /// the input data (from `IoCapture`) and receives the output
    /// (destined for `IoPlayback`). Clock events are forwarded to the
    /// engine's mailbox and the optional parent actor.
    pub fn apply(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], tick: &ClockTick) {
        let chunk_end = tick.sample_pos + tick.samples_since_last as u64;
        let _ = self.engine.process_tick(inputs, outputs, chunk_end);
        if tick.is_final {
            self.engine
                .handle()
                .send(CommandEnum::ClockTick(tick.clone()));
            if let Some(ref parent) = self.parent_ref {
                parent.send(CommandEnum::ClockTick(tick.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::time::ClockTick;

    #[test]
    fn apply_produces_finite_output() {
        use crate::builtin::Registry;
        use crate::compile_graph;

        let engine = compile_graph::<f32>("main = _ * 0.5", &Registry::new(), 44100.0).unwrap();
        let mut runner = ProgramRunner::new(engine, None);

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

        let input = [0.5f32; 4];
        let mut output = [0.0f32; 4];
        runner.apply(&[&input[..]], &mut [&mut output[..]], &tick);

        for v in &output {
            assert!(v.is_finite());
        }
    }
}
