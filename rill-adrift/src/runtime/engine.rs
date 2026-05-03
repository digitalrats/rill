//! Audio engine — driven by the backend's hardware callback.

use rill_core::time::{ClockTick, SystemClock};
use rill_graph::engine::SignalEngine as RillEngine;
use rill_graph::GraphBuilder;

pub const BUF_SIZE: usize = 256;

/// Handle — keeps the backend alive.
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
        let (nodes, topo_order, _) = graph.into_parts();

        let engine = Box::new(RillEngine::<f32, BUF_SIZE>::new(
            nodes, topo_order, None, None,
        ));
        let engine_ptr: *mut RillEngine<f32, BUF_SIZE> = Box::leak(engine);

        let backend = crate::registration::get_audio_backend();
        if let Some(b) = backend.as_ref() {
            let sample_pos = std::cell::Cell::new(0u64);
            b.set_process_callback(Box::new(move || {
                let tick = ClockTick::new(
                    sample_pos.get(), BUF_SIZE as u32, sample_rate,
                );
                unsafe {
                    let _ = (*engine_ptr).process_block(&tick);
                }
                sample_pos.set(sample_pos.get() + BUF_SIZE as u64);
            }));
            let _ = b.start();
        }

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
