//! Thin wrapper over RillProgram with anchor-based parameter routing via mailbox.
//!
//! During `process()`, the engine drains its mailbox for `GraphSetParameter`
//! commands, maps anchor+param_name to program parameter indices, and then
//! executes the flat RillProgram.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::program::RillProgram;

/// Anchor name → (param_name → param_index) mapping.
pub type AnchorMap = HashMap<String, HashMap<String, usize>>;

/// Runtime for a flat RillProgram with anchor-based parameter control.
pub struct RillGraphEngine<T: Transcendental> {
    program: RillProgram<T>,
    anchor_map: AnchorMap,
    /// Pending param updates: (param_index, value). Populated by drain_mailbox,
    /// applied one per process() call to prevent overwriting intermediate states.
    pending_params: VecDeque<(usize, rill_core::traits::ParamValue)>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
}

impl<T: Transcendental> RillGraphEngine<T> {
    /// Create a new engine wrapping a compiled program.
    pub fn new(program: RillProgram<T>, anchor_map: AnchorMap) -> Self {
        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();
        Self {
            program,
            anchor_map,
            pending_params: VecDeque::new(),
            mailbox,
            actor_ref,
        }
    }

    /// Handle for sending `GraphSetParameter` commands from control threads.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// The underlying compiled program.
    pub fn program(&self) -> &RillProgram<T> {
        &self.program
    }

    /// Anchor → param → index map.
    pub fn anchor_map(&self) -> &AnchorMap {
        &self.anchor_map
    }

    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::GraphSetParameter {
                anchor,
                param,
                value,
            } = cmd
            {
                if let Some(inner) = self.anchor_map.get(&anchor) {
                    if let Some(&param_idx) = inner.get(&param) {
                        self.pending_params.push_back((param_idx, value));
                    }
                }
            }
        }
    }
}

impl<T: Transcendental> Algorithm<T> for RillGraphEngine<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.drain_mailbox();
        if let Some((param_idx, value)) = self.pending_params.pop_front() {
            self.program.set_param(param_idx, value);
        }
        self.program.process(input, output)
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.program);
    }
}
