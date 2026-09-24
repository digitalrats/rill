//! ProgramEngine — thin control wrapper around a single [`RillProgram`].
//!
//! Owns the actor mailbox and parameter routing; execution delegates to the
//! program's flat instruction sequence. This is the single execution mechanism
//! for both DSL sources and graph definitions.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ParamValue, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::program::RillProgram;

#[cfg(feature = "debug")]
use crate::debug::{CmdStr, CommandFrame, DebugControl, ProbeSlot};
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;

/// Map from parameter name to its index in the program's parameter list.
pub type ParamMap = HashMap<String, usize>;

struct PendingParam {
    param_idx: usize,
    value: ParamValue,
    sample_pos: Option<u64>,
}

/// A runnable signal program with SetParameter routing and a control mailbox.
pub struct ProgramEngine<T: Transcendental, const BUF: usize> {
    program: RillProgram<T, BUF>,
    pending: Vec<PendingParam>,
    param_map: ParamMap,
    anchor: String,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
}

impl<T: Transcendental, const BUF: usize> ProgramEngine<T, BUF> {
    /// Create a new engine from a compiled program and a shared mailbox.
    pub fn new(program: RillProgram<T, BUF>, mailbox: Arc<Mailbox<CommandEnum>>) -> Self {
        let actor_ref = mailbox.actor_ref();
        let param_map = program
            .params_meta()
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), i))
            .collect();
        Self {
            program,
            pending: Vec::new(),
            param_map,
            anchor: "main".to_string(),
            mailbox,
            actor_ref,
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
        }
    }

    /// Returns the actor handle for sending control commands to the engine.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// Returns the parameter name → index mapping.
    pub fn param_map(&self) -> HashMap<String, usize> {
        self.param_map.clone()
    }

    /// Reference to the underlying program.
    pub fn program(&self) -> &RillProgram<T, BUF> {
        &self.program
    }

    /// Mutable reference to the underlying program.
    pub fn program_mut(&mut self) -> &mut RillProgram<T, BUF> {
        &mut self.program
    }

    #[cfg(feature = "debug")]
    /// Allocate `count` probe slots for the engine.
    pub fn allocate_probe_slots(&mut self, count: usize) {
        self.probe_slots = (0..count)
            .map(|_| std::sync::Arc::new(ProbeSlot::default()))
            .collect();
    }

    #[cfg(feature = "debug")]
    /// Clone probe slots, debug control, and command queue for sharing with a
    /// collector or debugger thread.
    pub fn clone_debug_state(
        &self,
    ) -> (
        Vec<std::sync::Arc<ProbeSlot>>,
        DebugControl,
        std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    ) {
        (
            self.probe_slots.clone(),
            self.debug_control.clone(),
            self.command_queue.clone(),
        )
    }

    fn drain_mailbox(&mut self) {
        #[cfg(feature = "debug")]
        let block_idx = self.debug_control.block_index.load(Ordering::Relaxed);

        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::SetParameter(ref sp) = cmd {
                let param_name = sp.parameter.as_str();
                #[cfg(feature = "debug")]
                let mut applied = false;
                let matches_anchor = sp.anchor.is_empty() || sp.anchor == self.anchor;
                if matches_anchor {
                    if let Some(&idx) = self.param_map.get(param_name) {
                        self.pending.push(PendingParam {
                            param_idx: idx,
                            value: sp.value.clone(),
                            sample_pos: sp.sample_pos,
                        });
                        #[cfg(feature = "debug")]
                        {
                            applied = true;
                        }
                    }
                }
                #[cfg(feature = "debug")]
                if applied {
                    let _ = self.command_queue.push(CommandFrame {
                        block_index: block_idx,
                        command_kind: CmdStr::new("SetParameter"),
                        node_name: CmdStr::new(&sp.anchor),
                        param_name: CmdStr::new(&format!("{}", sp.parameter)),
                        value_repr: CmdStr::new(&format!("{:?}", sp.value)),
                    });
                }
            }
        }
    }

    /// Apply pending parameter updates that are due by `chunk_end`.
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

    /// Main processing tick — applies pending params and runs the program.
    pub fn process_tick(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
        chunk_end: u64,
    ) -> ProcessResult<()> {
        #[cfg(feature = "debug")]
        {
            self.debug_control
                .block_index
                .fetch_add(1, Ordering::Relaxed);
        }
        self.drain_mailbox();

        #[cfg(feature = "debug")]
        {
            while self.debug_control.global_pause.load(Ordering::Acquire)
                && !self.debug_control.global_resume.load(Ordering::Acquire)
            {
                std::hint::spin_loop();
            }
            self.debug_control
                .global_resume
                .store(false, Ordering::Release);
        }

        self.apply_due_params(chunk_end);

        MultichannelAlgorithm::process(&mut self.program, inputs, outputs)
    }

    /// Reset the underlying program to its initial state.
    pub fn reset(&mut self) {
        Algorithm::reset(&mut self.program);
    }
}

impl<T: Transcendental, const BUF: usize> Algorithm<T> for ProgramEngine<T, BUF> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let inputs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let mut outs: [&mut [T]; 1] = [output];
        self.process_tick(inputs, &mut outs, u64::MAX)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl<T: Transcendental, const BUF: usize> MultichannelAlgorithm<T> for ProgramEngine<T, BUF> {
    fn num_inputs(&self) -> usize {
        self.program.ir.num_inputs
    }

    fn num_outputs(&self) -> usize {
        self.program.ir.num_outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.process_tick(inputs, outputs, u64::MAX)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}
