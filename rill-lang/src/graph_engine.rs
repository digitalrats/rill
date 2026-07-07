//! RillGraphEngine — runtime for compiled graphs.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::buffer::Buffer;
use rill_core::buffer::FixedBuffer;
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::Algorithm;
use rill_core::traits::ParamValue;
use rill_core::traits::ProcessResult;
use rill_core_actor::{ActorRef, Mailbox};

use crate::graph_schedule::{ScheduledGraph, Step};
use crate::program::RillProgram;

/// Runtime engine for a pre-compiled graph schedule.
pub struct RillGraphEngine<T: Transcendental, const BUF: usize> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,
    buffers: Vec<FixedBuffer<T, BUF>>,
    delay_buffers: Vec<FixedBuffer<T, BUF>>,
    mailbox: Arc<Mailbox<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    param_values: Vec<Vec<f64>>,
    param_map: HashMap<String, HashMap<String, (usize, usize)>>,
}

impl<T: Transcendental, const BUF: usize> RillGraphEngine<T, BUF> {
    /// Create a new runtime engine from a compiled schedule and programs.
    pub fn new(
        schedule: ScheduledGraph,
        programs: Vec<RillProgram<T>>,
        node_names: Vec<String>,
    ) -> Self {
        let num_programs = programs.len();
        let mut param_values: Vec<Vec<f64>> = Vec::with_capacity(num_programs);
        let mut param_map: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

        for prog in &programs {
            let values: Vec<f64> = prog.params_meta().iter().map(|p| p.default).collect();
            param_values.push(values);
        }

        for (pi, prog) in programs.iter().enumerate() {
            if let Some(node_name) = node_names.get(pi) {
                let mut inner = HashMap::new();
                for (param_idx, param_def) in prog.params_meta().iter().enumerate() {
                    inner.insert(param_def.name.clone(), (pi, param_idx));
                }
                param_map.insert(node_name.clone(), inner);
            }
        }

        let buffers = {
            let mut bufs = Vec::with_capacity(schedule.buffers);
            for _ in 0..schedule.buffers {
                bufs.push(FixedBuffer::<T, BUF>::default());
            }
            bufs
        };
        let delay_buffers = {
            let mut dbs = Vec::with_capacity(schedule.delay_slots);
            for _ in 0..schedule.delay_slots {
                dbs.push(FixedBuffer::<T, BUF>::default());
            }
            dbs
        };

        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();

        Self {
            schedule,
            programs,
            buffers,
            delay_buffers,
            mailbox,
            actor_ref,
            param_values,
            param_map,
        }
    }

    /// Return a handle for sending commands to this engine from control threads.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    fn drain_mailbox(&mut self) {
        while let Some(cmd) = self.mailbox.pop() {
            if let CommandEnum::GraphSetParameter {
                anchor,
                param,
                value,
            } = cmd
            {
                if let Some(inner) = self.param_map.get(&anchor) {
                    if let Some(&(prog_idx, param_idx)) = inner.get(&param) {
                        let v = match value {
                            ParamValue::Float(f) => f as f64,
                            ParamValue::Int(i) => i as f64,
                            ParamValue::Bool(_) => return,
                            _ => return,
                        };
                        self.param_values[prog_idx][param_idx] = v;
                    }
                }
            }
        }
    }
}

impl<T: Transcendental, const BUF: usize> Algorithm<T> for RillGraphEngine<T, BUF> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.drain_mailbox();

        if let Some(inp) = input {
            let n = inp.len().min(BUF);
            if self.schedule.inputs > 0 {
                self.buffers[0].as_mut_slice()[..n].copy_from_slice(&inp[..n]);
            }
        }

        for i in 0..self.schedule.steps.len() {
            let step = self.schedule.steps[i].clone();
            match step {
                Step::ReadDelay { slot, target } => {
                    let src: Vec<T> = self.delay_buffers[slot].as_slice().to_vec();
                    self.buffers[target].copy_from(&src);
                }
                Step::InlineProgram {
                    node_idx,
                    input_bufs,
                    output_bufs,
                    param_indices,
                } => {
                    let prog = &mut self.programs[node_idx];
                    for &pi in &param_indices {
                        prog.set_param(pi, self.param_values[node_idx][pi]);
                    }
                    let input_slice: Option<Vec<T>> = input_bufs
                        .first()
                        .map(|&bi| self.buffers[bi].as_slice().to_vec());
                    let mut out_slice = vec![T::default(); BUF];
                    prog.process(input_slice.as_deref(), &mut out_slice)?;
                    for &ob in &output_bufs {
                        self.buffers[ob].copy_from(&out_slice);
                    }
                }
                Step::BufferCopy {
                    from,
                    to,
                    gain,
                    add,
                } => {
                    let gain_t = T::from_f32(gain);
                    if add {
                        let src = self.buffers[from].as_slice().to_vec();
                        for (j, sample) in self.buffers[to].as_mut_slice().iter_mut().enumerate() {
                            *sample += src[j] * gain_t;
                        }
                    } else {
                        let src = self.buffers[from].as_slice().to_vec();
                        self.buffers[to].copy_from(&src);
                        if (gain - 1.0f32).abs() > f32::EPSILON {
                            for sample in self.buffers[to].as_mut_slice().iter_mut() {
                                *sample *= gain_t;
                            }
                        }
                    }
                }
                Step::WriteDelay { source, slot } => {
                    let src: Vec<T> = self.buffers[source].as_slice().to_vec();
                    self.delay_buffers[slot].copy_from(&src);
                }
            }
        }

        for (i, &buf_idx) in self.schedule.output_mapping.iter().enumerate() {
            if i < output.len() && buf_idx < self.buffers.len() {
                let src = self.buffers[buf_idx].as_slice();
                let len = output.len().min(src.len());
                output[..len].copy_from_slice(&src[..len]);
            }
        }

        Ok(())
    }

    fn reset(&mut self) {
        for prog in &mut self.programs {
            prog.reset();
        }
        for buf in &mut self.buffers {
            buf.clear();
        }
        for db in &mut self.delay_buffers {
            db.clear();
        }
    }
}
