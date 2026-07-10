//! Thin wrapper over RillProgram with flat parameter routing via mailbox.
//!
//! During `process()`, the engine drains its mailbox for `SetParameter`
//! commands and applies them with sample-accurate timing via `apply_due_params`,
//! matching rill-graph's deferred parameter mechanism.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::program::RillProgram;

/// Map from parameter name to its index in the program's parameter list.
pub type ParamMap = HashMap<String, usize>;

/// A deferred parameter update with optional sample-accurate timing.
struct PendingParam {
    /// Target param index in the program.
    param_idx: usize,
    /// New value.
    value: rill_core::traits::ParamValue,
    /// Sample position when this update should take effect.
    /// `None` means immediate (apply on next process call).
    sample_pos: Option<u64>,
}

/// A compiled rill-lang program backed by its own actor mailbox for control messages.
pub struct RillGraphEngine<T: Transcendental> {
    program: RillProgram<T>,
    param_map: ParamMap,
    /// Deferred parameter updates, sorted by sample_pos.
    pending: Vec<PendingParam>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
}

impl<T: Transcendental> RillGraphEngine<T> {
    /// Create a new graph engine from a compiled program and a parameter name-to-index map.
    pub fn new(program: RillProgram<T>, param_map: ParamMap) -> Self {
        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();
        Self {
            program,
            param_map,
            pending: Vec::new(),
            mailbox,
            actor_ref,
        }
    }

    /// Returns the actor handle for sending control commands to the engine.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// Returns a reference to the compiled rill-lang program.
    pub fn program(&self) -> &RillProgram<T> {
        &self.program
    }

    /// Returns the parameter name-to-index map used by this engine.
    pub fn param_map(&self) -> &ParamMap {
        &self.param_map
    }

    /// Drain mailbox: push all `SetParameter` to pending queue.
    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            match cmd {
                CommandEnum::SetParameter(ref sp) => {
                    let name = sp.parameter.as_str();
                    if let Some(&idx) = self.param_map.get(name) {
                        self.pending.push(PendingParam {
                            param_idx: idx,
                            value: sp.value.clone(),
                            sample_pos: sp.sample_pos,
                        });
                    }
                }
                CommandEnum::ClockTick(_) => {
                    // ClockTick commands are also routed through the engine
                    // but the engine doesn't process them — they're forwarded
                    // by the ProgramRunner.
                }
                _ => {}
            }
        }
    }

    /// Apply all pending parameter updates that are due by `chunk_end`
    /// (the absolute sample position just past the current block).
    /// Matches rill-graph's `apply_due_params`.
    pub fn apply_due_params(&mut self, chunk_end: u64) {
        if self.pending.is_empty() {
            return;
        }
        self.pending.sort_by_key(|p| p.sample_pos.unwrap_or(0));
        let split = self
            .pending
            .partition_point(|p| p.sample_pos.is_none_or(|sp| sp < chunk_end));
        if split == 0 {
            return;
        }
        for p in self.pending.drain(0..split) {
            self.program.set_param(p.param_idx, p.value);
        }
    }
}

impl<T: Transcendental> Algorithm<T> for RillGraphEngine<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let inputs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let outputs: &mut [&mut [T]] = &mut [output];
        MultichannelAlgorithm::process(self, inputs, outputs)
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.program);
    }
}

impl<T: Transcendental> MultichannelAlgorithm<T> for RillGraphEngine<T> {
    fn num_inputs(&self) -> usize {
        self.program.num_inputs()
    }

    fn num_outputs(&self) -> usize {
        self.program.num_outputs()
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.drain_mailbox();
        MultichannelAlgorithm::process(&mut self.program, inputs, outputs)
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.program);
    }
}
