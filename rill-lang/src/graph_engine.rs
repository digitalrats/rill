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
#[cfg(feature = "router")]
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_lower::{ScheduledGraph, Step};
use crate::program::RillProgram;

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

/// Graph execution engine that runs a `ScheduledGraph` over a buffer pool.
pub struct RillGraphEngine<T: Transcendental> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,
    buffers: Vec<Vec<T>>,
    delay_buffers: Vec<Vec<T>>,
    param_values: Vec<Vec<f64>>,
    pending: Vec<PendingParam>,
    param_maps: Vec<HashMap<String, usize>>,
    anchor_map: HashMap<String, usize>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
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

    /// Drain mailbox: push all `SetParameter` to pending queue.
    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::SetParameter(ref sp) = cmd {
                let param_name = sp.parameter.as_str();
                if !sp.anchor.is_empty() {
                    if let Some(&prog_idx) = self.anchor_map.get(&sp.anchor) {
                        if let Some(&idx) = self.param_maps[prog_idx].get(param_name) {
                            self.pending.push(PendingParam {
                                param_idx: idx,
                                program_idx: prog_idx,
                                value: sp.value.clone(),
                                sample_pos: sp.sample_pos,
                            });
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
                            break;
                        }
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
            self.programs[p.program_idx].set_param(p.param_idx, p.value);
        }
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
        self.drain_mailbox();

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
        for prog in &mut self.programs {
            prog.init(sr);
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
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for RillGraphEngine<T> {
    fn num_inputs(&self) -> usize {
        self.schedule.inputs
    }

    fn num_outputs(&self) -> usize {
        self.schedule.outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.drain_mailbox();

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
