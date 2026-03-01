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
//! use kama_core::traits::{Processor, Source, Sink, ParameterId, ParamValue};
//! use kama_core::ProcessResult;
//!
//! const BLOCK_SIZE: usize = 64;
//!
//! // Simple gain processor
//! struct GainProcessor {
//!     gain: f32,
//! }
//!
//! impl Processor<f32, BLOCK_SIZE> for GainProcessor {
//!     fn process(
//!         &mut self,
//!         inputs: &[&[f32; BLOCK_SIZE]],
//!         outputs: &mut [&mut [f32; BLOCK_SIZE]],
//!         _control: &[f32],
//!     ) -> ProcessResult<()> {
//!         for (input, output) in inputs.iter().zip(outputs.iter_mut()) {
//!             for i in 0..BLOCK_SIZE {
//!                 output[i] = input[i] * self.gain;
//!             }
//!         }
//!         Ok(())
//!     }
//!
//!     fn num_audio_inputs(&self) -> usize { 1 }
//!     fn num_audio_outputs(&self) -> usize { 1 }
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
//!     fn init(&mut self, _sample_rate: f32) {}
//!     fn reset(&mut self) {}
//! }
//!
//! // Simple source generating silence
//! struct SilenceSource;
//!
//! impl Source<f32, BLOCK_SIZE> for SilenceSource {
//!     fn generate(&mut self, outputs: &mut [&mut [f32; BLOCK_SIZE]], _control: &[f32]) -> ProcessResult<()> {
//!         for channel in outputs.iter_mut() {
//!             for sample in channel.iter_mut() {
//!                 *sample = 0.0;
//!             }
//!         }
//!         Ok(())
//!     }
//!
//!     fn num_audio_outputs(&self) -> usize { 1 }
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
//!     fn init(&mut self, _sample_rate: f32) {}
//!     fn reset(&mut self) {}
//! }
//!
//! // Simple sink that discards audio
//! struct DiscardSink;
//!
//! impl Sink<f32, BLOCK_SIZE> for DiscardSink {
//!     fn process(&mut self, _inputs: &[&[f32; BLOCK_SIZE]], _control: &[f32]) -> ProcessResult<()> {
//!         Ok(())
//!     }
//!
//!     fn num_audio_inputs(&self) -> usize { 1 }
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
//!     fn init(&mut self, _sample_rate: f32) {}
//!     fn reset(&mut self) {}
//! }
//!
//! // Now build the graph
//! let mut graph = AudioGraph::<BLOCK_SIZE>::new(44100.0);
//!
//! let source_id = graph.add_source(Box::new(SilenceSource));
//! let gain_id = graph.add_processor(Box::new(GainProcessor { gain: 0.5 }));
//! let sink_id = graph.add_sink(Box::new(DiscardSink));
//!
//! // Connect source to gain
//! graph.connect_audio(
//!     PortId::audio_out(source_id, 0),
//!     PortId::audio_in(gain_id, 0),
//! )?;
//!
//! // Connect gain to sink
//! graph.connect_audio(
//!     PortId::audio_out(gain_id, 0),
//!     PortId::audio_in(sink_id, 0),
//! )?;
//!
//! // Process one block (push model)
//! graph.push_block()?;
//!
//! # Ok::<(), GraphError>(())
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
    use kama_core::traits::{Processor, Source, Sink};
    use kama_core::ProcessResult;
    use kama_digital_filters::BiquadProcessor;

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

    impl<const BUF_SIZE: usize> Processor<f32, BUF_SIZE> for TestProcessor<BUF_SIZE> {
        fn process(
            &mut self,
            inputs: &[&[f32; BUF_SIZE]],
            outputs: &mut [&mut [f32; BUF_SIZE]],
            _control: &[f32],
        ) -> ProcessResult<()> {
            if inputs.is_empty() || outputs.is_empty() {
                return Ok(());
            }

            for i in 0..BUF_SIZE {
                outputs[0][i] = inputs[0][i] * self.gain;
            }
            Ok(())
        }

        fn num_audio_inputs(&self) -> usize { 1 }
        fn num_audio_outputs(&self) -> usize { 1 }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
    }

    // Test source for module-level tests
    struct TestSource<const BUF_SIZE: usize> {
        id: u32,
    }

    impl<const BUF_SIZE: usize> TestSource<BUF_SIZE> {
        fn new(id: u32) -> Self {
            Self { id }
        }
    }

    impl<const BUF_SIZE: usize> Source<f32, BUF_SIZE> for TestSource<BUF_SIZE> {
        fn generate(&mut self, outputs: &mut [&mut [f32; BUF_SIZE]], _control: &[f32]) -> ProcessResult<()> {
            for channel in outputs.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample = 0.0;
                }
            }
            Ok(())
        }

        fn num_audio_outputs(&self) -> usize { 1 }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
    }

    // Test sink for module-level tests
    struct TestSink<const BUF_SIZE: usize> {
        id: u32,
    }

    impl<const BUF_SIZE: usize> TestSink<BUF_SIZE> {
        fn new(id: u32) -> Self {
            Self { id }
        }
    }

    impl<const BUF_SIZE: usize> Sink<f32, BUF_SIZE> for TestSink<BUF_SIZE> {
        fn process(&mut self, inputs: &[&[f32; BUF_SIZE]], _control: &[f32]) -> ProcessResult<()> {
            // Do nothing, just consume
            Ok(())
        }

        fn num_audio_inputs(&self) -> usize { 1 }
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

    #[test]
    fn test_graph_with_biquad_filter() {
        let mut graph = AudioGraph::<64>::new(44100.0);

        // Add nodes
        let source_id = graph.add_source(Box::new(TestSource::<64>::new(1)));
        let biquad = BiquadProcessor::new(1000.0, 0.707, 0.0);
        let biquad_id = graph.add_processor(Box::new(biquad));
        let sink_id = graph.add_sink(Box::new(TestSink::<64>::new(2)));

        // Connect source output to biquad input
        graph.connect_audio(
            PortId::audio_out(source_id, 0),
            PortId::audio_in(biquad_id, 0),
        ).unwrap();

        // Connect biquad output to sink input
        graph.connect_audio(
            PortId::audio_out(biquad_id, 0),
            PortId::audio_in(sink_id, 0),
        ).unwrap();

        // Process one block (push model)
        graph.push_block().unwrap();

        // If no panic, test passes
    }
}