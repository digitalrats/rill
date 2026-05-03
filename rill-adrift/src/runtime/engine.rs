//! Audio engine — owns the graph and drives processing via the backend
//! callback. The callback is set by AudioInput after construction.

use rill_core::time::{ClockTick, SystemClock};
use rill_core::traits::processable::NodeVariant;
use rill_graph::GraphBuilder;

pub const BUF_SIZE: usize = 256;

/// Handle — keeps the graph alive.
pub struct AudioHandle;

impl AudioHandle {
    pub fn start(
        builder: GraphBuilder<f32, BUF_SIZE>,
        sample_rate: f32,
    ) -> Result<Self, String> {
        let clock = SystemClock::with_sample_rate(sample_rate);
        let graph = builder
            .build(Box::new(clock))
            .map_err(|e| format!("graph build failed: {e:?}"))?;
        let (nodes, _topo, _) = graph.into_parts();
        let _nodes_ptr: *mut [NodeVariant<f32, BUF_SIZE>] = Box::leak(nodes.into_boxed_slice());

        // TODO: pass nodes_ptr to AudioInput::start()
        // AudioInput will:
        //   1. Drain param_queue from its own field
        //   2. Read backend input
        //   3. Call port.propagate() on each output

        Ok(AudioHandle)
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        if let Some(b) = crate::registration::get_audio_backend().as_ref() {
            let _ = b.stop();
        }
    }
}
