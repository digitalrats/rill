//! Execution engine for CompiledGraph with a FixedBuffer pool.
//!
//! Runs a flat vector of NodeClosures in topological order. Zero heap
//! allocation on the real-time signal path.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::buffer::Buffer;
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ParamValue, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_compiler::CompiledGraph;

#[cfg(feature = "debug")]
use crate::debug::{CmdStr, CommandFrame, DebugControl, ProbeSlot};
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;

/// Map from parameter name to its index in the node's parameter list.
pub type ParamMap = HashMap<String, usize>;

struct PendingParam {
    node_idx: usize,
    param_idx: usize,
    value: ParamValue,
    sample_pos: Option<u64>,
}

/// Graph execution engine running a [`CompiledGraph`].
pub struct CompiledGraphEngine<T: Transcendental, const BUF_SIZE: usize> {
    graph: CompiledGraph<T, BUF_SIZE>,
    pending: Vec<PendingParam>,
    param_maps: Vec<HashMap<String, usize>>,
    anchor_map: HashMap<String, usize>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
}

impl<T: Transcendental, const BUF_SIZE: usize> CompiledGraphEngine<T, BUF_SIZE> {
    /// Create a new graph engine from a compiled graph and a shared mailbox.
    pub fn new(graph: CompiledGraph<T, BUF_SIZE>, mailbox: Arc<Mailbox<CommandEnum>>) -> Self {
        let actor_ref = mailbox.actor_ref();
        let anchor_map: HashMap<String, usize> = graph
            .node_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();
        let param_maps: Vec<HashMap<String, usize>> = graph
            .node_param_names
            .iter()
            .map(|names| {
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (n.clone(), i))
                    .collect()
            })
            .collect();

        Self {
            graph,
            pending: Vec::new(),
            param_maps,
            anchor_map,
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

    /// Returns the merged parameter map for all nodes in the engine.
    ///
    /// The map is the first node's mapping, which covers all parameters
    /// for single-node engines. For multi-node graphs, this returns
    /// the first node's map only.
    pub fn param_map(&self) -> HashMap<String, usize> {
        self.param_maps.first().cloned().unwrap_or_default()
    }

    #[cfg(feature = "debug")]
    /// Allocate `count` probe slots for the engine.
    pub fn allocate_probe_slots(&mut self, count: usize) {
        self.probe_slots = (0..count)
            .map(|_| std::sync::Arc::new(ProbeSlot::default()))
            .collect();
    }

    #[cfg(feature = "debug")]
    /// Return debug state handles for external collector/debugger threads.
    pub fn debug_state(
        &self,
    ) -> (
        &[std::sync::Arc<ProbeSlot>],
        DebugControl,
        std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    ) {
        (
            &self.probe_slots,
            self.debug_control.clone(),
            self.command_queue.clone(),
        )
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
                if !sp.anchor.is_empty() {
                    if let Some(&node_idx) = self.anchor_map.get(&sp.anchor) {
                        if let Some(&idx) = self.param_maps[node_idx].get(param_name) {
                            self.pending.push(PendingParam {
                                node_idx,
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
                } else {
                    for (node_idx, map) in self.param_maps.iter().enumerate() {
                        if let Some(&idx) = map.get(param_name) {
                            self.pending.push(PendingParam {
                                node_idx,
                                param_idx: idx,
                                value: sp.value.clone(),
                                sample_pos: sp.sample_pos,
                            });
                            #[cfg(feature = "debug")]
                            {
                                applied = true;
                            }
                            break;
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
    /// Preserves sample-accurate scheduling: params with `sample_pos >= chunk_end`
    /// are deferred to the next tick.
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
            if p.node_idx < self.graph.nodes.len() {
                self.graph.nodes[p.node_idx].set_param(p.param_idx, &p.value);
            }
        }
    }

    /// Main processing tick — applies pending params, copies inputs, runs nodes, copies outputs.
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

        for (i, input) in inputs.iter().enumerate() {
            if i < self.graph.inputs && i < self.graph.buffers.len() {
                let buf = self.graph.buffers[i].as_mut_slice();
                let n = input.len().min(buf.len());
                buf[..n].copy_from_slice(&input[..n]);
            }
        }

        for node in &mut self.graph.nodes {
            node.execute(&mut self.graph.buffers)?;
        }

        for (i, output) in outputs.iter_mut().enumerate() {
            if i < self.graph.output_mapping.len() {
                let src = self.graph.output_mapping[i];
                if src < self.graph.buffers.len() {
                    let buf = self.graph.buffers[src].as_slice();
                    let n = output.len().min(buf.len());
                    output[..n].copy_from_slice(&buf[..n]);
                }
            }
        }

        Ok(())
    }

    /// Reset all buffers and nodes to initial state.
    pub fn reset(&mut self) {
        for buf in &mut self.graph.buffers {
            buf.fill(T::ZERO);
        }
        for node in &mut self.graph.nodes {
            node.reset();
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for CompiledGraphEngine<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let bufs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let out_bufs: &mut [&mut [T]] = &mut [output];
        MultichannelAlgorithm::process(self, bufs, out_bufs)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> MultichannelAlgorithm<T>
    for CompiledGraphEngine<T, BUF_SIZE>
{
    fn num_inputs(&self) -> usize {
        self.graph.inputs
    }

    fn num_outputs(&self) -> usize {
        self.graph.outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.process_tick(inputs, outputs, u64::MAX)
    }

    fn reset(&mut self) {
        Self::reset(self);
    }
}
