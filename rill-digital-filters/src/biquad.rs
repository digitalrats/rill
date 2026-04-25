//! Biquad filter implementation using rill-core-dsp
//!
//! This module provides a Processor wrapper around the `Biquad` filter from `rill-core-dsp`
//! for use in audio graphs.

use rill_core::{
    AudioNode, AudioNum, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId,
    Port, ProcessError, ProcessResult, Processor,
};
use rill_core_dsp::algorithm::{Algorithm, ParameterizedAlgorithm};
use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};

/// Biquad processor with configurable filter type and parameters.
pub struct BiquadProcessor<T: AudioNum, const BUF_SIZE: usize> {
    /// Node identifier
    id: NodeId,
    /// Node metadata
    metadata: NodeMetadata,
    /// Input ports
    inputs: Vec<Port<T, BUF_SIZE>>,
    /// Output ports
    outputs: Vec<Port<T, BUF_SIZE>>,
    /// Control ports
    controls: Vec<Port<T, BUF_SIZE>>,
    /// Node state
    state: NodeState<T, BUF_SIZE>,
    /// Cutoff frequency (Hz)
    pub cutoff: f32,
    /// Q factor
    pub q: f32,
    /// Gain in dB (for peak/shelving filters)
    pub gain_db: f32,
    /// Current filter type
    pub filter_type: FilterType,
    /// Inner biquad algorithm
    pub algorithm: Biquad<T>,
}

impl<T: AudioNum, const BUF_SIZE: usize> BiquadProcessor<T, BUF_SIZE> {
    /// Creates a new Biquad processor with default parameters.
    pub fn new(sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("BiquadProcessor", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        // Create one audio input and one audio output
        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
        };

        let mut algorithm = Biquad::new(params);
        algorithm.init(sample_rate);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
            filter_type: FilterType::LowPass,
            algorithm,
        }
    }

    /// Creates a new Biquad processor with the given filter parameters.
    pub fn from_params(params: FilterParams) -> Self {
        let mut instance = Self::new(44100.0); // sample rate will be updated later
        instance.cutoff = params.cutoff;
        instance.q = params.q;
        instance.gain_db = params.gain_db;
        instance.filter_type = params.filter_type;
        instance.update_algorithm();
        instance
    }

    /// Creates a new Biquad processor with individual parameters (backward compatibility).
    pub fn new_with_params(filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> Self {
        let params = FilterParams {
            filter_type,
            cutoff,
            q,
            gain_db,
        };
        Self::from_params(params)
    }

    /// Returns the current cutoff frequency (Hz).
    pub fn cutoff(&self) -> f32 {
        self.cutoff
    }

    /// Sets the cutoff frequency (Hz) and updates coefficients.
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.max(20.0).min(20000.0);
        self.update_algorithm();
    }

    /// Returns the current Q factor.
    pub fn q(&self) -> f32 {
        self.q
    }

    /// Sets the Q factor and updates coefficients.
    pub fn set_q(&mut self, q: f32) {
        self.q = q.max(0.1).min(20.0);
        self.update_algorithm();
    }

    /// Returns the current gain in dB (for peak/shelving filters).
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Sets the gain in dB and updates coefficients.
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain_db = gain_db.max(-24.0).min(24.0);
        self.update_algorithm();
    }

    /// Returns the current filter type.
    pub fn filter_type(&self) -> FilterType {
        self.filter_type
    }

    /// Sets the filter type and updates coefficients.
    pub fn set_filter_type(&mut self, filter_type: FilterType) {
        self.filter_type = filter_type;
        self.update_algorithm();
    }

    /// Returns a reference to the inner algorithm.
    pub fn algorithm(&self) -> &Biquad<T> {
        &self.algorithm
    }

    /// Returns a mutable reference to the inner algorithm.
    pub fn algorithm_mut(&mut self) -> &mut Biquad<T> {
        &mut self.algorithm
    }

    /// Updates the inner algorithm with current parameters.
    fn update_algorithm(&mut self) {
        let params = FilterParams {
            filter_type: self.filter_type,
            cutoff: self.cutoff,
            q: self.q,
            gain_db: self.gain_db,
        };
        self.algorithm.set_params(params);
        // Re‑initialize if sample rate has changed (should be done via `init`)
        if self.state.sample_rate > 0.0 {
            self.algorithm.init(self.state.sample_rate);
        }
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for BiquadProcessor<T, BUF_SIZE> {
    fn node_type_id(&self) -> rill_core::NodeTypeId
    where
        Self: 'static + Sized,
    {
        rill_core::NodeTypeId::of::<Self>()
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
        // Update port IDs? Ports store node ID, but they are created with NodeId(0).
        // For simplicity, we ignore for now.
    }

    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        self.algorithm.init(sample_rate);
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.algorithm.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "cutoff" => Some(ParamValue::Float(self.cutoff)),
            "q" => Some(ParamValue::Float(self.q)),
            "gain_db" => Some(ParamValue::Float(self.gain_db)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "cutoff" => {
                    self.set_cutoff(v);
                    Ok(())
                }
                "q" => {
                    self.set_q(v);
                    Ok(())
                }
                "gain_db" => {
                    self.set_gain_db(v);
                    Ok(())
                }
                _ => Err(ProcessError::parameter(format!(
                    "Unknown parameter: {}",
                    name
                ))),
            }
        } else {
            Err(ProcessError::parameter("Expected float value"))
        }
    }

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(index)
    }

    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.controls.get(index)
    }

    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.controls.get_mut(index)
    }

    fn num_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for BiquadProcessor<T, BUF_SIZE> {
    fn process(
        &mut self,
        _clock: &rill_core::ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[rill_core::ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
        audio_outputs: &mut [&mut [T; BUF_SIZE]],
        _control_outputs: &mut [T],
        _clock_outputs: &mut [rill_core::ClockTick],
        _feedback_outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if audio_outputs.is_empty() {
            return Ok(());
        }

        // We have exactly one audio input and one audio output (as per construction)
        if let (Some(input_buffer), Some(output_buffer)) =
            (audio_inputs.first(), audio_outputs.first_mut())
        {
            self.algorithm
                .process_block(&input_buffer[..], &mut output_buffer[..]);
        }

        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}

/// Re-export of the generic Biquad filter from rill-core-dsp for advanced use.
pub use rill_core_dsp::filters::Biquad as BiquadFilterGeneric;

/// Type alias for backward compatibility (f32 specialization).
/// NOTE: This type does NOT implement Processor; use `BiquadProcessor` for graph integration.
pub type BiquadFilter = BiquadFilterGeneric<f32>;

/// Extension trait providing convenience methods for Biquad filter.
pub trait BiquadExt<T> {
    /// Get cutoff frequency (Hz)
    fn cutoff(&self) -> f32;
    /// Set cutoff frequency (Hz)
    fn set_cutoff(&mut self, cutoff: f32);
    /// Get Q factor
    fn q(&self) -> f32;
    /// Set Q factor
    fn set_q(&mut self, q: f32);
    /// Get gain in dB (for peak/shelving filters)
    fn gain_db(&self) -> f32;
    /// Set gain in dB
    fn set_gain_db(&mut self, gain_db: f32);
    /// Get filter type
    fn filter_type(&self) -> FilterType;
    /// Set filter type
    fn set_filter_type(&mut self, filter_type: FilterType);
}

impl<T: rill_core::AudioNum> BiquadExt<T> for Biquad<T>
where
    Biquad<T>: ParameterizedAlgorithm<T, Params = FilterParams>,
{
    fn cutoff(&self) -> f32 {
        self.params().cutoff
    }

    fn set_cutoff(&mut self, cutoff: f32) {
        let mut params = self.params().clone();
        params.cutoff = cutoff.max(20.0).min(20000.0);
        self.set_params(params);
    }

    fn q(&self) -> f32 {
        self.params().q
    }

    fn set_q(&mut self, q: f32) {
        let mut params = self.params().clone();
        params.q = q.max(0.1).min(20.0);
        self.set_params(params);
    }

    fn gain_db(&self) -> f32 {
        self.params().gain_db
    }

    fn set_gain_db(&mut self, gain_db: f32) {
        let mut params = self.params().clone();
        params.gain_db = gain_db.max(-24.0).min(24.0);
        self.set_params(params);
    }

    fn filter_type(&self) -> FilterType {
        self.params().filter_type
    }

    fn set_filter_type(&mut self, filter_type: FilterType) {
        let mut params = self.params().clone();
        params.filter_type = filter_type;
        self.set_params(params);
    }
}

// Helper functions for converting FilterType to/from string.
#[allow(dead_code)]
fn filter_type_to_string(ft: FilterType) -> String {
    match ft {
        FilterType::LowPass => "LowPass".to_string(),
        FilterType::HighPass => "HighPass".to_string(),
        FilterType::BandPass => "BandPass".to_string(),
        FilterType::Notch => "Notch".to_string(),
        FilterType::Peak => "Peak".to_string(),
        FilterType::LowShelf => "LowShelf".to_string(),
        FilterType::HighShelf => "HighShelf".to_string(),
        FilterType::AllPass => "AllPass".to_string(),
    }
}

#[allow(dead_code)]
fn string_to_filter_type(s: &str) -> Option<FilterType> {
    match s {
        "LowPass" => Some(FilterType::LowPass),
        "HighPass" => Some(FilterType::HighPass),
        "BandPass" => Some(FilterType::BandPass),
        "Notch" => Some(FilterType::Notch),
        "Peak" => Some(FilterType::Peak),
        "LowShelf" => Some(FilterType::LowShelf),
        "HighShelf" => Some(FilterType::HighShelf),
        "AllPass" => Some(FilterType::AllPass),
        _ => None,
    }
}

/// Backward‑compatibility wrapper for `BiquadFilter::new` with four arguments.
pub fn new(filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> BiquadFilter {
    BiquadFilter::new(FilterParams {
        filter_type,
        cutoff,
        q,
        gain_db,
    })
}
