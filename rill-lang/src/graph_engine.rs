//! Thin wrapper over RillProgram with anchor-based parameter routing via mailbox.
//!
//! During `process()`, the engine drains its mailbox for `GraphSetParameter`
//! commands, maps anchor+param_name to program parameter indices, and then
//! executes the flat RillProgram.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::program::RillProgram;

/// Anchor name → (param_name → param_index) mapping.
pub type AnchorMap = HashMap<String, HashMap<String, usize>>;

/// Runtime for a flat RillProgram with anchor-based parameter control.
pub struct RillGraphEngine<T: Transcendental> {
    program: RillProgram<T>,
    anchor_map: AnchorMap,
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
                        let v = match value {
                            ParamValue::Float(f) => f as f64,
                            ParamValue::Int(i) => i as f64,
                            ParamValue::Bool(_) => return,
                            _ => return,
                        };
                        self.program.set_param(param_idx, v);
                    }
                }
            }
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
