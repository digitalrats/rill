//! # Kama Graph - Audio graph with clock synchronization
//!
//! This crate provides a flexible audio graph for real-time processing
//! with clock synchronization and integration with the automaton world.
//!
//! ## Key Features
//!
//! - **Clock-driven processing** - All nodes process on clock ticks
//! - **Multiple signal types** - Audio, control, clock, feedback
//! - **Two-way communication** - Commands from automata, telemetry back
//! - **Micro-control observation** - Track real-time violations
//! - **Topological processing** - Automatic dependency resolution
//!
//! ## Example
//!
//! ```rust
//! use kama_graph::prelude::*;
//! use kama_core::prelude::*;
//! use kama_core::traits::*;
//! use std::marker::PhantomData;
//!
//! const BLOCK_SIZE: usize = 64;
//!
//! // Dummy oscillator implementing Source
//! struct MyOscillator<T: AudioNum, const BUF_SIZE: usize> {
//!     frequency: T,
//!     amplitude: T,
//!     phase: T,
//!     sample_rate: T,
//!     state: NodeState<T, BUF_SIZE>,
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> MyOscillator<T, BUF_SIZE> {
//!     fn new(frequency: f32) -> Self {
//!         Self {
//!             frequency: T::from_f32(frequency),
//!             amplitude: T::from_f32(0.5),
//!             phase: T::ZERO,
//!             sample_rate: T::from_f32(44100.0),
//!             state: NodeState::new(44100.0),
//!         }
//!     }
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for MyOscillator<T, BUF_SIZE> {
//!     fn metadata(&self) -> NodeMetadata {
//!         NodeMetadata {
//!             name: "MyOscillator".to_string(),
//!             category: NodeCategory::Source,
//!             description: "Dummy oscillator".to_string(),
//!             author: "Kama".to_string(),
//!             version: env!("CARGO_PKG_VERSION").to_string(),
//!             audio_inputs: 0,
//!             audio_outputs: 1,
//!             control_inputs: 0,
//!             control_outputs: 0,
//!             clock_inputs: 1,
//!             clock_outputs: 0,
//!             feedback_ports: 0,
//!             parameters: vec![],
//!         }
//!     }
//!     
//!     fn init(&mut self, sample_rate: f32) {
//!         self.sample_rate = T::from_f32(sample_rate);
//!         self.state.sample_rate = sample_rate;
//!     }
//!     
//!     fn reset(&mut self) {
//!         self.phase = T::ZERO;
//!         self.state.reset();
//!     }
//!     
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
//!         None
//!     }
//!     
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
//!         Ok(())
//!     }
//!     
//!     fn id(&self) -> NodeId { NodeId(0) }
//!     fn set_id(&mut self, _id: NodeId) {}
//!     
//!     fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     
//!     fn state(&self) -> &NodeState<T,BUF_SIZE> { &self.state }
//!     fn state_mut(&mut self) -> &mut NodeState<T,BUF_SIZE> { &mut self.state }
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> Source<T, BUF_SIZE> for MyOscillator<T, BUF_SIZE> {
//!     fn generate(
//!         &mut self,
//!         clock: &ClockTick,
//!         _control_inputs: &[T],
//!         _clock_inputs: &[ClockTick],
//!         outputs: &mut [&mut [T; BUF_SIZE]],
//!     ) -> ProcessResult<()> {
//!         if outputs.is_empty() {
//!             return Ok(());
//!         }
//!         let two_pi = T::from_f32(2.0 * std::f32::consts::PI);
//!         let phase_inc = self.frequency / T::from_f32(clock.sample_rate);
//!         
//!         for i in 0..BUF_SIZE {
//!             let phase_rad = self.phase * two_pi;
//!             outputs[0][i] = phase_rad.sin() * self.amplitude;
//!             self.phase = self.phase + phase_inc;
//!             if self.phase >= T::from_f32(1.0) {
//!                 self.phase = self.phase - T::from_f32(1.0);
//!             }
//!         }
//!         Ok(())
//!     }
//!     
//!     fn num_audio_outputs(&self) -> usize { 1 }
//!     fn num_control_inputs(&self) -> usize { 0 }
//!     fn num_clock_inputs(&self) -> usize { 1 }
//! }
//!
//! // Dummy filter implementing Processor
//! struct MyFilter<T: AudioNum, const BUF_SIZE: usize> {
//!     cutoff: T,
//!     state: NodeState<T, BUF_SIZE>,
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> MyFilter<T, BUF_SIZE> {
//!     fn new(cutoff: f32) -> Self {
//!         Self {
//!             cutoff: T::from_f32(cutoff),
//!             state: NodeState::new(44100.0),
//!         }
//!     }
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for MyFilter<T, BUF_SIZE> {
//!     fn metadata(&self) -> NodeMetadata {
//!         NodeMetadata {
//!             name: "MyFilter".to_string(),
//!             category: NodeCategory::Processor,
//!             description: "Dummy filter".to_string(),
//!             author: "Kama".to_string(),
//!             version: env!("CARGO_PKG_VERSION").to_string(),
//!             audio_inputs: 1,
//!             audio_outputs: 1,
//!             control_inputs: 0,
//!             control_outputs: 0,
//!             clock_inputs: 0,
//!             clock_outputs: 0,
//!             feedback_ports: 0,
//!             parameters: vec![],
//!         }
//!     }
//!     
//!     fn init(&mut self, sample_rate: f32) {
//!         self.state.sample_rate = sample_rate;
//!     }
//!     
//!     fn reset(&mut self) {
//!         self.state.reset();
//!     }
//!     
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
//!         None
//!     }
//!     
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
//!         Ok(())
//!     }
//!     
//!     fn id(&self) -> NodeId { NodeId(0) }
//!     fn set_id(&mut self, _id: NodeId) {}
//!     
//!     fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     
//!     fn state(&self) -> &NodeState<T,BUF_SIZE> { &self.state }
//!     fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
//! }
//!
//! impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for MyFilter<T, BUF_SIZE> {
//!     fn process(
//!         &mut self,
//!         _clock: &ClockTick,
//!         audio_inputs: &[&[T; BUF_SIZE]],
//!         _control_inputs: &[T],
//!         _clock_inputs: &[ClockTick],
//!         _feedback_inputs: &[&[T; BUF_SIZE]],
//!         audio_outputs: &mut [&mut [T; BUF_SIZE]],
//!         _control_outputs: &mut [T],
//!         _clock_outputs: &mut [ClockTick],
//!         _feedback_outputs: &mut [&mut [T; BUF_SIZE]],
//!     ) -> ProcessResult<()> {
//!         // Simple passthrough for demonstration
//!         if !audio_inputs.is_empty() && !audio_outputs.is_empty() {
//!             audio_outputs[0].copy_from_slice(audio_inputs[0]);
//!         }
//!         Ok(())
//!     }
//! }
//!
//! // Example usage
//! let mut builder = GraphBuilder::<f32, BLOCK_SIZE>::new();
//!
//! // Add nodes
//! let osc_id = builder.add_source(Box::new(MyOscillator::<f32, BLOCK_SIZE>::new(440.0)));
//! let filter_id = builder.add_processor(Box::new(MyFilter::<f32, BLOCK_SIZE>::new(1000.0)));
//!
//! // Connect oscillator output to filter input
//! builder.connect_audio(osc_id, 0, filter_id, 0).unwrap();
//!
//! // Build graph with system clock
//! let mut graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0)));
//!
//! // Process a block (processes entire graph)
//! graph.process_block().unwrap();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod graph;

pub use graph::{AudioGraph, GraphBuilder, GraphStats};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{AudioGraph, GraphBuilder};
    pub use kama_core::prelude::*;
}
