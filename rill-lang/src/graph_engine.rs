//! Thin wrapper over RillProgram with anchor-based parameter routing via mailbox.
//!
//! During `process()`, the engine drains its mailbox for `GraphSetParameter`
//! commands and applies them with sample-accurate timing via `apply_due_params`,
//! matching rill-graph's deferred parameter mechanism.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::program::RillProgram;

pub type AnchorMap = HashMap<String, HashMap<String, usize>>;

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

pub struct RillGraphEngine<T: Transcendental> {
    program: RillProgram<T>,
    anchor_map: AnchorMap,
    /// Deferred parameter updates, sorted by sample_pos.
    pending: Vec<PendingParam>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
}

impl<T: Transcendental> RillGraphEngine<T> {
    pub fn new(program: RillProgram<T>, anchor_map: AnchorMap) -> Self {
        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();
        Self {
            program,
            anchor_map,
            pending: Vec::new(),
            mailbox,
            actor_ref,
        }
    }

    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    pub fn program(&self) -> &RillProgram<T> {
        &self.program
    }

    pub fn anchor_map(&self) -> &AnchorMap {
        &self.anchor_map
    }

    /// Drain mailbox: push all `GraphSetParameter` to pending queue.
    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::GraphSetParameter {
                anchor,
                param,
                value,
                sample_pos,
            } = cmd
            {
                if let Some(inner) = self.anchor_map.get(&anchor) {
                    if let Some(&param_idx) = inner.get(&param) {
                        self.pending.push(PendingParam {
                            param_idx,
                            value,
                            sample_pos,
                        });
                    }
                }
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
        self.drain_mailbox();
        self.program.process(input, output)
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.program);
    }
}
