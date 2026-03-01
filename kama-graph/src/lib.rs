//! # Kama Graph - Real-time Audio Graph
//!
//! This crate provides a flexible, high-performance audio graph for real-time
//! audio processing. It supports multiple operation modes (Producer, Consumer,
//! Bridge, Processor) and integrates seamlessly with the kama-patchbay
//! automation system.
//!
//! ## Core Concepts
//!
//! - **Nodes**: Processing units that implement the `Processor` trait
//! - **Connections**: Zero-copy links between nodes via `PipeBuffer`
//! - **Ports**: Typed connection points (`AudioIn`, `AudioOut`)
//! - **Flow Control**: Per-port policies for underflow/overflow handling
//! - **Graph Roles**: Producer, Consumer, Bridge, or Processor modes
//!
//! ## Example: Simple Playback Chain
//!
//! ```rust
//! use kama_graph::prelude::*;
//! use kama_core::traits::{NodeId, PortId};
//!
//! const BLOCK_SIZE: usize = 64;
//!
//! // Create graph with fixed block size
//! let mut graph = AudioGraph::<BLOCK_SIZE>::new(44100.0);
//!
//! // Add processors
//! let osc_id = graph.add_processor(Box::new(MyOscillator::new(440.0)));
//! let filter_id = graph.add_processor(Box::new(MyFilter::lowpass(1000.0)));
//!
//! // Connect them
//! graph.connect(
//!     PortId::audio_out(osc_id, 0),
//!     PortId::audio_in(filter_id, 0),
//! )?;
//!
//! // Configure as Producer (for playback)
//! let output_port = PortId::audio_out(filter_id, 0);
//! graph.configure_as_producer(vec![output_port])?;
//! graph.start()?;
//!
//! // In audio thread:
//! let output = graph.produce_next(output_port)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(unused))]

// Re-export core types
pub use kama_core::traits::{NodeId, ParameterId, ParamValue, PortId, PortType};
pub use kama_core::queues::{CommandEnum, CommandQueue, TelemetryQueue, MicroControlObserver};

// Graph implementation
mod graph;
pub use graph::*;

// Re-export for convenience
pub mod prelude {
    pub use crate::graph::{
        AudioGraph, GraphError, GraphResult,
        GraphRole, GraphState, DataFlow, PortDirection,
        UnderflowPolicy, OverflowPolicy, PortFlowConfig,
        GraphStats, PortStats,
    };
    pub use kama_core::traits::{NodeId, ParameterId, PortId, PortType};
    pub use kama_core::queues::{CommandQueue, TelemetryQueue};
}

/// Version of the kama-graph crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default block size for audio processing
pub const DEFAULT_BLOCK_SIZE: usize = 64;

/// Maximum supported block size
pub const MAX_BLOCK_SIZE: usize = 8192;

/// Minimum supported block size
pub const MIN_BLOCK_SIZE: usize = 16;

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{Processor, ProcessResult};

    // Test processor for module-level tests
    struct TestProcessor<const BUF_SIZE: usize> {
        id: u32,
        gain: f32,
    }

    impl<const BUF_SIZE: usize> TestProcessor<BUF_SIZE> {
        fn new(id: u32, gain: f32) -> Self {
            Self { id, gain }
        }
    }

    impl<const BUF_SIZE: usize> Processor<BUF_SIZE> for TestProcessor<BUF_SIZE> {
        fn process(
            &mut self,
            inputs: &[&[f32; BUF_SIZE]],
            outputs: &mut [&mut [f32; BUF_SIZE]],
        ) -> ProcessResult<()> {
            if inputs.is_empty() || outputs.is_empty() {
                return Ok(());
            }

            for i in 0..BUF_SIZE {
                outputs[0][i] = inputs[0][i] * self.gain;
            }
            Ok(())
        }

        fn num_inputs(&self) -> usize { 1 }
        fn num_outputs(&self) -> usize { 1 }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
    }

    #[test]
    fn test_lib_re_exports() {
        // Verify that all expected types are accessible
        let _graph = AudioGraph::<64>::new(44100.0);
        let _node_id = NodeId(0);
        let _port_id = PortId::audio_out(_node_id, 0);
        let _role = GraphRole::Processor;
        let _state = GraphState::Idle;
        let _flow = DataFlow::Standalone;
    }

    #[test]
    fn test_graph_via_prelude() {
        use prelude::*;

        let mut graph = AudioGraph::<64>::new(48000.0);
        let proc = TestProcessor::<64>::new(1, 0.5);
        let id = graph.add_processor(Box::new(proc));

        assert!(graph.contains_node(id));
        assert_eq!(graph.sample_rate(), 48000.0);
    }

    #[test]
    fn test_constants() {
        assert!(VERSION.len() > 0);
        assert_eq!(DEFAULT_BLOCK_SIZE, 64);
        assert!(MAX_BLOCK_SIZE > MIN_BLOCK_SIZE);
    }
}