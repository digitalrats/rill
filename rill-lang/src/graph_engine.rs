//! Execution engine for `ScheduledGraph` with a buffer pool.
//!
//! The engine runs a linear schedule of steps (`InlineProgram`, `BufferCopy`,
//! `ReadDelay`, `WriteDelay`) over a pool of pre-allocated signal buffers.
//! Parameters are routed from actor mailbox commands to the correct program
//! within the graph.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::bridge::BridgeAlgorithm;
#[cfg(feature = "router")]
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_lower::{DuplexSchedule, ScheduledGraph, Step};
use crate::program::RillProgram;

#[cfg(feature = "debug")]
use crate::debug::{CmdStr, CommandFrame, DebugControl, ProbeFrame, ProbeSlot};
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;

/// Map from parameter name to its index in the program's parameter list.
pub type ParamMap = HashMap<String, usize>;

/// A deferred parameter update with optional sample-accurate timing.
struct PendingParam {
    /// Target param index in the program.
    param_idx: usize,
    /// Which program in `programs` vec this param belongs to.
    program_idx: usize,
    /// New value.
    value: rill_core::traits::ParamValue,
    /// Sample position when this update should take effect.
    sample_pos: Option<u64>,
}

/// Self-contained engine state for one side of a duplex graph.
struct SubEngine<T: Transcendental> {
    programs: Vec<RillProgram<T>>,
    buffers: Vec<Vec<T>>,
    param_maps: Vec<HashMap<String, usize>>,
}

impl<T: Transcendental> SubEngine<T> {
    fn new(programs: Vec<RillProgram<T>>, n_bufs: usize, buf_size: usize) -> Self {
        let param_maps: Vec<HashMap<String, usize>> = programs
            .iter()
            .map(|p| {
                p.params_meta()
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (m.name.clone(), i))
                    .collect()
            })
            .collect();
        Self {
            programs,
            buffers: vec![vec![T::ZERO; buf_size]; n_bufs],
            param_maps,
        }
    }
}

/// Bridge-backed duplex execution state.
struct DuplexData<T: Transcendental> {
    duplex_schedule: DuplexSchedule,
    left: SubEngine<T>,
    right: SubEngine<T>,
    bridge: Box<dyn BridgeAlgorithm<T>>,
    feedback_read: Vec<Vec<T>>,
    feedback_write: Vec<Vec<T>>,
    name_to_idx: HashMap<String, usize>,
    delay_buffers: Vec<Vec<T>>,
}

/// Graph execution engine that runs a `ScheduledGraph` over a buffer pool.
pub struct RillGraphEngine<T: Transcendental> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,
    buffers: Vec<Vec<T>>,
    delay_buffers: Vec<Vec<T>>,
    #[allow(dead_code)]
    param_values: Vec<Vec<f64>>,
    pending: Vec<PendingParam>,
    param_maps: Vec<HashMap<String, usize>>,
    anchor_map: HashMap<String, usize>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    duplex: Option<DuplexData<T>>,
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
}

impl<T: Transcendental> RillGraphEngine<T> {
    /// Create a new graph engine from a schedule, programs, and a shared mailbox.
    pub fn new(
        schedule: ScheduledGraph,
        programs: Vec<RillProgram<T>>,
        mailbox: Arc<Mailbox<CommandEnum>>,
        buf_size: usize,
    ) -> Self {
        let n_bufs = schedule.buffers;
        let n_delays = schedule
            .steps
            .iter()
            .filter(|s| matches!(s, Step::ReadDelay { .. }))
            .count();

        let buffers = vec![vec![T::ZERO; buf_size]; n_bufs];
        let delay_buffers = vec![vec![T::ZERO; buf_size]; n_delays.max(1)];
        let param_values: Vec<Vec<f64>> = programs
            .iter()
            .map(|p| p.params_meta().iter().map(|m| m.default).collect())
            .collect();
        let param_maps: Vec<HashMap<String, usize>> = programs
            .iter()
            .map(|p| {
                p.params_meta()
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (m.name.clone(), i))
                    .collect()
            })
            .collect();
        let actor_ref = mailbox.actor_ref();

        let anchor_map = schedule
            .program_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();

        Self {
            schedule,
            programs,
            buffers,
            delay_buffers,
            param_values,
            pending: Vec::new(),
            param_maps,
            anchor_map,
            mailbox,
            actor_ref,
            duplex: None,
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
        }
    }

    /// Create a duplex graph engine from a [`DuplexSchedule`], programs,
    /// a bridge backend, and a shared mailbox.
    pub fn new_duplex(
        duplex_schedule: DuplexSchedule,
        left_programs: Vec<RillProgram<T>>,
        right_programs: Vec<RillProgram<T>>,
        bridge: Box<dyn BridgeAlgorithm<T>>,
        mailbox: Arc<Mailbox<CommandEnum>>,
        buf_size: usize,
    ) -> Self {
        let actor_ref = mailbox.actor_ref();
        let n_left_bufs = duplex_schedule.left.buffers;
        let n_right_bufs = duplex_schedule.right.buffers;

        let n_left_delays = duplex_schedule
            .left
            .steps
            .iter()
            .filter(|s| matches!(s, Step::ReadDelay { .. }))
            .count();
        let n_right_delays = duplex_schedule
            .right
            .steps
            .iter()
            .filter(|s| matches!(s, Step::ReadDelay { .. }))
            .count();

        let left = SubEngine::new(left_programs, n_left_bufs, buf_size);
        let right = SubEngine::new(right_programs, n_right_bufs, buf_size);

        let n_feedback = duplex_schedule.feedback_names.len();

        let mut name_to_idx = HashMap::new();
        for (i, name) in duplex_schedule.feedback_names.iter().enumerate() {
            name_to_idx.insert(name.clone(), i);
        }

        let n_delays = n_left_delays + n_right_delays;

        let anchor_map = {
            let mut map = HashMap::new();
            for (i, name) in duplex_schedule.left.program_names.iter().enumerate() {
                map.insert(name.clone(), i);
            }
            for (i, name) in duplex_schedule.right.program_names.iter().enumerate() {
                map.insert(name.clone(), i + left.programs.len());
            }
            map
        };

        let param_maps: Vec<HashMap<String, usize>> = {
            let mut maps = Vec::new();
            maps.extend(left.param_maps.clone());
            maps.extend(right.param_maps.clone());
            maps
        };

        let duplex = DuplexData {
            duplex_schedule,
            left,
            right,
            bridge,
            feedback_read: vec![vec![T::ZERO; buf_size]; n_feedback],
            feedback_write: vec![vec![T::ZERO; buf_size]; n_feedback],
            name_to_idx,
            delay_buffers: vec![vec![T::ZERO; buf_size]; n_delays.max(1)],
        };

        let param_values: Vec<Vec<f64>> = vec![vec![]; param_maps.len()];

        Self {
            schedule: ScheduledGraph {
                inputs: 0,
                outputs: 0,
                steps: Vec::new(),
                buffers: 0,
                output_mapping: Vec::new(),
                program_names: Vec::new(),
            },
            programs: Vec::new(),
            buffers: Vec::new(),
            delay_buffers: Vec::new(),
            param_values,
            pending: Vec::new(),
            param_maps,
            anchor_map,
            mailbox,
            actor_ref,
            duplex: Some(duplex),
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

    /// Returns the merged parameter map for all programs in the engine.
    ///
    /// The map is the first program's mapping, which covers all parameters
    /// for single-program engines. For multi-program graphs, this returns
    /// the first program's map only.
    pub fn param_map(&self) -> &HashMap<String, usize> {
        self.param_maps.first().expect("engine has no programs")
    }

    /// Returns the active programs in this engine (borrowed).
    pub fn programs(&self) -> &[RillProgram<T>] {
        &self.programs
    }

    /// Returns the first program (for single-program engines).
    pub fn program(&self) -> &RillProgram<T> {
        self.programs.first().expect("engine has no programs")
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

    /// Drain mailbox: push all `SetParameter` to pending queue.
    fn drain_mailbox(&mut self) {
        #[cfg(feature = "debug")]
        let block_idx = self.debug_control.block_index.load(Ordering::Relaxed);

        while let Some(cmd) = self.mailbox.pop() {
            match cmd {
                CommandEnum::SetParameter(ref sp) => {
                    let param_name = sp.parameter.as_str();
                    #[cfg(feature = "debug")]
                    let mut applied = false;
                    if !sp.anchor.is_empty() {
                        if let Some(&prog_idx) = self.anchor_map.get(&sp.anchor) {
                            if let Some(&idx) = self.param_maps[prog_idx].get(param_name) {
                                self.pending.push(PendingParam {
                                    param_idx: idx,
                                    program_idx: prog_idx,
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
                        for (prog_idx, map) in self.param_maps.iter().enumerate() {
                            if let Some(&idx) = map.get(param_name) {
                                self.pending.push(PendingParam {
                                    param_idx: idx,
                                    program_idx: prog_idx,
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
                _ => {
                    #[cfg(feature = "debug")]
                    {
                        let _ = self.command_queue.push(CommandFrame {
                            block_index: block_idx,
                            command_kind: CmdStr::new(&format!("{:?}", cmd)),
                            node_name: CmdStr::default(),
                            param_name: CmdStr::default(),
                            value_repr: CmdStr::default(),
                        });
                    }
                }
            }
        }
    }

    /// Apply all pending parameter updates that are due by `chunk_end`.
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
            if let Some(ref mut duplex) = self.duplex {
                if p.program_idx < duplex.left.programs.len() {
                    duplex.left.programs[p.program_idx].set_param(p.param_idx, p.value);
                } else {
                    let right_idx = p.program_idx - duplex.left.programs.len();
                    if right_idx < duplex.right.programs.len() {
                        duplex.right.programs[right_idx].set_param(p.param_idx, p.value);
                    }
                }
            } else {
                self.programs[p.program_idx].set_param(p.param_idx, p.value);
            }
        }
    }

    /// 5-phase tick for duplex graphs.
    ///
    /// Phases:
    /// 1. ReadFeedback — copy named feedback buffers into sub-engine buffers
    /// 2. process_left — execute left sub-graph, then bridge.process_left
    /// 3. process_right — bridge.process_right, then execute right sub-graph
    /// 4. WriteFeedback — copy sub-engine outputs into named feedback buffers
    /// 5. Shadow copy — swap read/write feedback buffers
    pub fn process_tick(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
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

        if let Some(ref mut duplex) = self.duplex {
            if !inputs.is_empty() || !outputs.is_empty() {
                let buf_size = if !outputs.is_empty() {
                    outputs[0].len()
                } else {
                    inputs[0].len()
                };

                // Phase 1: ReadFeedback
                Self::apply_read_feedback(duplex, buf_size);

                // Phase 2: process_left
                Self::execute_sub_schedule(
                    &duplex.duplex_schedule.left,
                    &mut duplex.left,
                    &mut duplex.delay_buffers,
                    inputs,
                    buf_size,
                    #[cfg(feature = "debug")]
                    &self.debug_control,
                    #[cfg(feature = "debug")]
                    &self.probe_slots,
                )?;
                duplex.bridge.process_left(inputs)?;

                // Phase 3: process_right
                duplex.bridge.process_right(outputs)?;
                Self::execute_sub_schedule(
                    &duplex.duplex_schedule.right,
                    &mut duplex.right,
                    &mut duplex.delay_buffers,
                    &[],
                    buf_size,
                    #[cfg(feature = "debug")]
                    &self.debug_control,
                    #[cfg(feature = "debug")]
                    &self.probe_slots,
                )?;

                // Phase 4: WriteFeedback
                Self::capture_write_feedback(duplex, buf_size);

                // Phase 5: Shadow copy
                for &idx in duplex.name_to_idx.values() {
                    duplex.feedback_read[idx][..buf_size]
                        .copy_from_slice(&duplex.feedback_write[idx][..buf_size]);
                }
            }
        } else {
            self.execute_siso(inputs, outputs)?;
        }

        Ok(())
    }

    fn apply_read_feedback(duplex: &mut DuplexData<T>, buf_size: usize) {
        for step in &duplex.duplex_schedule.left.steps {
            if let Step::ReadFeedback { name, target_buf } = step {
                if let Some(&idx) = duplex.name_to_idx.get(name) {
                    let src = &duplex.feedback_read[idx];
                    if *target_buf < duplex.left.buffers.len() {
                        duplex.left.buffers[*target_buf][..buf_size]
                            .copy_from_slice(&src[..buf_size]);
                    }
                }
            }
        }
        for step in &duplex.duplex_schedule.right.steps {
            if let Step::ReadFeedback { name, target_buf } = step {
                if let Some(&idx) = duplex.name_to_idx.get(name) {
                    let src = &duplex.feedback_read[idx];
                    if *target_buf < duplex.right.buffers.len() {
                        duplex.right.buffers[*target_buf][..buf_size]
                            .copy_from_slice(&src[..buf_size]);
                    }
                }
            }
        }
    }

    fn capture_write_feedback(duplex: &mut DuplexData<T>, buf_size: usize) {
        for step in &duplex.duplex_schedule.left.steps {
            if let Step::WriteFeedback { name, source_buf } = step {
                if let Some(&idx) = duplex.name_to_idx.get(name) {
                    let dst = &mut duplex.feedback_write[idx];
                    if *source_buf < duplex.left.buffers.len() {
                        dst[..buf_size]
                            .copy_from_slice(&duplex.left.buffers[*source_buf][..buf_size]);
                    }
                }
            }
        }
        for step in &duplex.duplex_schedule.right.steps {
            if let Step::WriteFeedback { name, source_buf } = step {
                if let Some(&idx) = duplex.name_to_idx.get(name) {
                    let dst = &mut duplex.feedback_write[idx];
                    if *source_buf < duplex.right.buffers.len() {
                        dst[..buf_size]
                            .copy_from_slice(&duplex.right.buffers[*source_buf][..buf_size]);
                    }
                }
            }
        }
    }

    fn execute_sub_schedule(
        schedule: &ScheduledGraph,
        engine: &mut SubEngine<T>,
        delay_bufs: &mut [Vec<T>],
        inputs: &[&[T]],
        buf_size: usize,
        #[cfg(feature = "debug")] debug_control: &DebugControl,
        #[cfg(feature = "debug")] probe_slots: &[std::sync::Arc<ProbeSlot>],
    ) -> ProcessResult<()> {
        for (i, input) in inputs.iter().enumerate() {
            if i < engine.buffers.len() {
                engine.buffers[i][..buf_size].copy_from_slice(input);
            }
        }

        for step in &schedule.steps {
            match step {
                Step::ReadFeedback { .. } | Step::WriteFeedback { .. } => {
                    // Handled in phases 1 and 4
                }
                Step::ReadDelay { slot, target } => {
                    if *target < engine.buffers.len() && *slot < delay_bufs.len() {
                        engine.buffers[*target][..buf_size]
                            .copy_from_slice(&delay_bufs[*slot][..buf_size]);
                    }
                }
                Step::WriteDelay { source, slot } => {
                    if *source < engine.buffers.len() && *slot < delay_bufs.len() {
                        delay_bufs[*slot][..buf_size]
                            .copy_from_slice(&engine.buffers[*source][..buf_size]);
                    }
                }
                Step::InlineProgram {
                    node_idx,
                    input_bufs,
                    output_bufs,
                    param_indices: _,
                } => {
                    let prog = &mut engine.programs[*node_idx];
                    for i in 0..prog.params_meta().len() {
                        let val = prog.param(i);
                        prog.set_param(i, val);
                    }
                    let n_in = input_bufs.len();
                    let n_out = output_bufs.len();

                    let (inp_slices, mut out_slices) =
                        buf_slices(&mut engine.buffers, input_bufs, output_bufs, buf_size);

                    if n_in <= 1 && n_out == 1 {
                        let input = if n_in == 0 { None } else { Some(inp_slices[0]) };
                        Algorithm::process(prog, input, out_slices[0])?;
                    } else {
                        #[cfg(feature = "router")]
                        {
                            MultichannelAlgorithm::process(prog, &inp_slices, &mut out_slices)?;
                        }
                        #[cfg(not(feature = "router"))]
                        {
                            return Err(rill_core::traits::ProcessError::processing(
                                "multi-IO graph requires 'router' feature",
                            ));
                        }
                    }
                    #[cfg(feature = "debug")]
                    {
                        let block_idx = debug_control.block_index.load(Ordering::Relaxed);
                        let ir = &engine.programs[*node_idx].ir;
                        for instr in &ir.instrs {
                            if let crate::ir::Instr::ProbePoint { id, .. } = instr {
                                let slot_idx = *id as usize;
                                if slot_idx < probe_slots.len() {
                                    let slot = &probe_slots[slot_idx];
                                    if slot.is_active()
                                        && !out_slices.is_empty()
                                        && !out_slices[0].is_empty()
                                    {
                                        let val = out_slices[0][0];
                                        let bits = val.to_f64().to_bits();
                                        slot.last_value.store(bits, Ordering::Release);
                                        let _ = slot.queue.push(ProbeFrame {
                                            value_bits: bits,
                                            block_index: block_idx,
                                        });
                                        if slot.is_breakpoint() {
                                            slot.paused_flag.store(true, Ordering::Release);
                                            debug_control.pause();
                                            while !debug_control
                                                .global_resume
                                                .load(Ordering::Acquire)
                                            {
                                                std::hint::spin_loop();
                                            }
                                            slot.paused_flag.store(false, Ordering::Release);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Step::BufferCopy {
                    from,
                    to,
                    gain,
                    add,
                } => {
                    if *from < engine.buffers.len() && *to < engine.buffers.len() {
                        if *from != *to {
                            let (src, dst) = if *from < *to {
                                let (left, right) = engine.buffers.split_at_mut(*to);
                                (&left[*from][..buf_size], &mut right[0][..buf_size])
                            } else {
                                let (left, right) = engine.buffers.split_at_mut(*from);
                                (&right[0][..buf_size], &mut left[*to][..buf_size])
                            };
                            if *add {
                                for (d, s) in dst.iter_mut().zip(src.iter()) {
                                    *d += *s * T::from_f64(*gain as f64);
                                }
                            } else {
                                dst.copy_from_slice(src);
                                if (*gain - 1.0f32).abs() > f32::EPSILON {
                                    let gain_t = T::from_f64(*gain as f64);
                                    for v in dst {
                                        *v *= gain_t;
                                    }
                                }
                            }
                        } else if *add {
                            let gain_t = T::from_f64(*gain as f64);
                            let buf = &mut engine.buffers[*to][..buf_size];
                            for v in buf {
                                *v = *v + *v * gain_t;
                            }
                        } else if (*gain - 1.0f32).abs() > f32::EPSILON {
                            let gain_t = T::from_f64(*gain as f64);
                            for v in &mut engine.buffers[*to][..buf_size] {
                                *v *= gain_t;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn execute_siso(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let buf_size = if !outputs.is_empty() {
            outputs[0].len()
        } else {
            return Ok(());
        };

        for (i, input) in inputs.iter().enumerate() {
            if i < self.buffers.len() {
                self.buffers[i][..buf_size].copy_from_slice(input);
            }
        }

        let steps = self.schedule.steps.clone();
        for step in &steps {
            self.execute_step(step, buf_size)?;
        }

        for (i, output) in outputs.iter_mut().enumerate() {
            if i < self.schedule.output_mapping.len() {
                let src = self.schedule.output_mapping[i];
                if src < self.buffers.len() {
                    output.copy_from_slice(&self.buffers[src][..buf_size]);
                }
            }
        }

        Ok(())
    }

    fn execute_step(&mut self, step: &Step, buf_size: usize) -> ProcessResult<()> {
        match step {
            Step::ReadDelay { slot, target } => {
                self.buffers[*target][..buf_size]
                    .copy_from_slice(&self.delay_buffers[*slot][..buf_size]);
            }
            Step::InlineProgram {
                node_idx,
                input_bufs,
                output_bufs,
                param_indices: _,
            } => {
                let prog = &mut self.programs[*node_idx];
                for i in 0..prog.params_meta().len() {
                    let val = prog.param(i);
                    prog.set_param(i, val);
                }
                let n_in = input_bufs.len();
                let n_out = output_bufs.len();

                let (inputs, mut outputs) =
                    buf_slices(&mut self.buffers, input_bufs, output_bufs, buf_size);

                if n_in <= 1 && n_out == 1 {
                    let input = if n_in == 0 { None } else { Some(inputs[0]) };
                    Algorithm::process(prog, input, outputs[0])?;
                } else {
                    #[cfg(feature = "router")]
                    {
                        MultichannelAlgorithm::process(prog, &inputs, &mut outputs)?;
                    }
                    #[cfg(not(feature = "router"))]
                    {
                        return Err(rill_core::traits::ProcessError::processing(
                            "multi-IO graph requires 'router' feature",
                        ));
                    }
                }
                #[cfg(feature = "debug")]
                {
                    let block_idx = self.debug_control.block_index.load(Ordering::Relaxed);
                    let ir = &self.programs[*node_idx].ir;
                    for instr in &ir.instrs {
                        if let crate::ir::Instr::ProbePoint { id, .. } = instr {
                            let slot_idx = *id as usize;
                            if slot_idx < self.probe_slots.len() {
                                let slot = &self.probe_slots[slot_idx];
                                if slot.is_active()
                                    && !output_bufs.is_empty()
                                    && *output_bufs.first().unwrap() < self.buffers.len()
                                {
                                    let buf = &self.buffers[*output_bufs.first().unwrap()];
                                    if !buf.is_empty() {
                                        let val = buf[0];
                                        let bits = val.to_f64().to_bits();
                                        slot.last_value.store(bits, Ordering::Release);
                                        let _ = slot.queue.push(ProbeFrame {
                                            value_bits: bits,
                                            block_index: block_idx,
                                        });
                                        if slot.is_breakpoint() {
                                            slot.paused_flag.store(true, Ordering::Release);
                                            self.debug_control.pause();
                                            while !self
                                                .debug_control
                                                .global_resume
                                                .load(Ordering::Acquire)
                                            {
                                                std::hint::spin_loop();
                                            }
                                            slot.paused_flag.store(false, Ordering::Release);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Step::BufferCopy {
                from,
                to,
                gain,
                add,
            } => {
                if *from != *to {
                    let (src, dst) = if *from < *to {
                        let (left, right) = self.buffers.split_at_mut(*to);
                        (&left[*from][..buf_size], &mut right[0][..buf_size])
                    } else {
                        let (left, right) = self.buffers.split_at_mut(*from);
                        (&right[0][..buf_size], &mut left[*to][..buf_size])
                    };
                    if *add {
                        for (d, s) in dst.iter_mut().zip(src.iter()) {
                            *d += *s * T::from_f64(*gain as f64);
                        }
                    } else {
                        dst.copy_from_slice(src);
                        if (*gain - 1.0f32).abs() > f32::EPSILON {
                            let gain_t = T::from_f64(*gain as f64);
                            for v in dst {
                                *v *= gain_t;
                            }
                        }
                    }
                } else if *add {
                    let gain_t = T::from_f64(*gain as f64);
                    let buf = &mut self.buffers[*to][..buf_size];
                    for v in buf {
                        *v = *v + *v * gain_t;
                    }
                } else if (*gain - 1.0f32).abs() > f32::EPSILON {
                    let gain_t = T::from_f64(*gain as f64);
                    for v in &mut self.buffers[*to][..buf_size] {
                        *v *= gain_t;
                    }
                }
            }
            Step::WriteDelay { source, slot } => {
                self.delay_buffers[*slot][..buf_size]
                    .copy_from_slice(&self.buffers[*source][..buf_size]);
            }
            Step::ReadFeedback { .. } | Step::WriteFeedback { .. } => {
                // Handled externally — these only appear in duplex sub-schedules.
            }
        }
        Ok(())
    }
}

impl<T: Transcendental> Algorithm<T> for RillGraphEngine<T> {
    #[cfg(feature = "router")]
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let bufs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let out_bufs: &mut [&mut [T]] = &mut [output];
        MultichannelAlgorithm::process(self, bufs, out_bufs)
    }

    #[cfg(not(feature = "router"))]
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let buf_size = output.len();
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

        if let Some(inp) = input {
            if self.schedule.inputs > 0 {
                self.buffers[0][..buf_size].copy_from_slice(inp);
            }
        }

        let steps = self.schedule.steps.clone();
        for step in &steps {
            self.execute_step(step, buf_size)?;
        }

        if let Some(&src) = self.schedule.output_mapping.first() {
            output.copy_from_slice(&self.buffers[src][..buf_size]);
        } else {
            output.fill(T::ZERO);
        }

        Ok(())
    }

    fn init(&mut self, sr: f32) {
        if let Some(ref mut duplex) = self.duplex {
            for prog in &mut duplex.left.programs {
                prog.init(sr);
            }
            for prog in &mut duplex.right.programs {
                prog.init(sr);
            }
        } else {
            for prog in &mut self.programs {
                prog.init(sr);
            }
        }
    }

    fn reset(&mut self) {
        for buf in &mut self.buffers {
            buf.fill(T::ZERO);
        }
        for buf in &mut self.delay_buffers {
            buf.fill(T::ZERO);
        }
        for prog in &mut self.programs {
            Algorithm::reset(prog);
        }
        if let Some(ref mut duplex) = self.duplex {
            for buf in &mut duplex.left.buffers {
                buf.fill(T::ZERO);
            }
            for buf in &mut duplex.right.buffers {
                buf.fill(T::ZERO);
            }
            for buf in &mut duplex.feedback_read {
                buf.fill(T::ZERO);
            }
            for buf in &mut duplex.feedback_write {
                buf.fill(T::ZERO);
            }
            for buf in &mut duplex.delay_buffers {
                buf.fill(T::ZERO);
            }
            duplex.bridge.reset();
        }
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for RillGraphEngine<T> {
    fn num_inputs(&self) -> usize {
        if let Some(ref duplex) = self.duplex {
            duplex.duplex_schedule.left.inputs
        } else {
            self.schedule.inputs
        }
    }

    fn num_outputs(&self) -> usize {
        if let Some(ref duplex) = self.duplex {
            duplex.duplex_schedule.right.outputs
        } else {
            self.schedule.outputs
        }
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.process_tick(inputs, outputs)
    }

    fn reset(&mut self) {
        Algorithm::reset(self);
    }
}

// ---------------------------------------------------------------------------
// Buffer-pool slice helpers
//
// Each buffer in `buffers` is a separate heap allocation (Vec<T>), so they
// do not alias. The DAG guarantees that input/output buffer index sets are
// disjoint — no buffer is read and written in the same step. This combined
// function uses raw pointers to extract both &[T] and &mut [T] slices from
// the same buffer pool without tripping the borrow checker.
// ---------------------------------------------------------------------------

#[allow(unsafe_code)]
fn buf_slices<'a, T>(
    bufs: &'a mut [Vec<T>],
    in_indices: &[usize],
    out_indices: &[usize],
    len: usize,
) -> (Vec<&'a [T]>, Vec<&'a mut [T]>) {
    let bufs_ptr = bufs.as_mut_ptr();
    let inputs: Vec<&[T]> = in_indices
        .iter()
        .map(|&i| unsafe { std::slice::from_raw_parts((*bufs_ptr.add(i)).as_ptr(), len) })
        .collect();
    let outputs: Vec<&mut [T]> = out_indices
        .iter()
        .map(|&i| unsafe { std::slice::from_raw_parts_mut((*bufs_ptr.add(i)).as_mut_ptr(), len) })
        .collect();
    (inputs, outputs)
}
