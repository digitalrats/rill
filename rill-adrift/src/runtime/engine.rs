//! Audio engine — builds the graph and delegates the reactive stream
//! to AudioInput. No processing loop in the runtime.

use std::sync::Arc;

use rill_core::queues::MpscQueue;
use rill_core::time::SystemClock;
use rill_core::traits::{NodeTypeId, ParameterId, ParamValue, SignalNode};
use rill_core::traits::processable::NodeVariant;
use rill_graph::GraphBuilder;
use rill_patchbay::control::ParameterCommand;

use crate::io::input::AudioInput;
use crate::registration;

pub const BUF_SIZE: usize = 256;

pub struct AudioHandle;

impl AudioHandle {
    pub fn start(
        builder: GraphBuilder<f32, BUF_SIZE>,
        sample_rate: f32,
        param_queue: Arc<MpscQueue<ParameterCommand>>,
    ) -> Result<Self, String> {
        let clock = SystemClock::with_sample_rate(sample_rate);
        let graph = builder
            .build(Box::new(clock))
            .map_err(|e| format!("graph build failed: {e:?}"))?;
        let (nodes, topo, _) = graph.into_parts();
        let nodes_ptr: *mut [NodeVariant<f32, BUF_SIZE>] =
            Box::leak(nodes.into_boxed_slice());

        // Find AudioInput in topo_order and call start.
        let input_tid = NodeTypeId::of::<AudioInput<f32, BUF_SIZE>>();
        if let Some(&source_idx) = topo.first() {
            if unsafe { (*nodes_ptr)[source_idx].node_type_id() } == input_tid {
                let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF_SIZE>]) + Send> = {
                    let q = param_queue.clone();
                    Box::new(move |nodes: &mut [NodeVariant<f32, BUF_SIZE>]| {
                        while let Some(cmd) = q.pop() {
                            let idx = cmd.node_id.inner() as usize;
                            if idx < nodes.len() {
                                if let Ok(pid) = ParameterId::new(&cmd.param) {
                                    let _ = nodes[idx].set_parameter(
                                        &pid, ParamValue::Float(cmd.value),
                                    );
                                }
                            }
                        }
                    })
                };

                let audio_input: &mut AudioInput<f32, BUF_SIZE> = {
                    let src = &mut unsafe { &mut *nodes_ptr }[source_idx];
                    if let NodeVariant::Source(ref mut s) = src {
                        // SAFETY: we verified node_type_id matches AudioInput
                        unsafe { &mut *(s.as_mut() as *mut dyn rill_core::traits::Source<f32, BUF_SIZE>
                            as *mut AudioInput<f32, BUF_SIZE>) }
                    } else {
                        return Err("first node is not AudioInput".into());
                    }
                };

                audio_input.start(nodes_ptr, source_idx, drain_fn, sample_rate);
            }
        }

        Ok(AudioHandle)
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        if let Some(b) = registration::get_audio_backend().as_ref() {
            let _ = b.stop();
        }
    }
}
