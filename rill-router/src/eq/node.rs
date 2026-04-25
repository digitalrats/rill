//! Processor nodes for integration with rill-core audio graphs.

use rill_core::{
    AudioNode, AudioNum, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId,
    Port, ProcessError, ProcessResult, Processor,
};
use rill_core_dsp::filters::{Biquad, Filter, FilterParams, FilterType};

use super::{BandType, FilterFactory, GraphicEq, ParametricEq};

/// Default factory that creates Biquad<f32> filters.
#[derive(Debug, Clone, Default)]
pub struct BiquadFactory;

impl FilterFactory<Biquad<f32>> for BiquadFactory {
    fn create_filter(
        &self,
        filter_type: FilterType,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Biquad<f32> {
        let params = FilterParams {
            filter_type,
            cutoff: frequency,
            q,
            gain_db,
        };
        Biquad::new(params)
    }
}

/// Parametric equalizer processor node for audio graphs.
pub struct ParametricEqProcessor<T: AudioNum, const BUF_SIZE: usize> {
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
    /// Inner parametric equalizer (works with f32)
    eq: ParametricEq<Biquad<f32>, BiquadFactory>,
    /// Output gain (linear)
    pub output_gain: f32,
    /// Number of bands
    num_bands: usize,
}

impl<T: AudioNum, const BUF_SIZE: usize> ParametricEqProcessor<T, BUF_SIZE> {
    /// Creates a new parametric equalizer processor with default parameters.
    pub fn new(sample_rate: f32, num_bands: usize) -> Self {
        let metadata = NodeMetadata::new("ParametricEqProcessor", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        // Create one audio input and one audio output
        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let factory = BiquadFactory;
        let mut eq = ParametricEq::new(factory, num_bands, sample_rate);
        eq.init(sample_rate);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            eq,
            output_gain: 1.0,
            num_bands,
        }
    }

    /// Set parameters for a specific band.
    pub fn set_band(
        &mut self,
        index: usize,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Result<(), rill_core::Error> {
        self.eq.set_band(index, frequency, q, gain_db)?;
        Ok(())
    }

    /// Set band type.
    pub fn set_band_type(
        &mut self,
        index: usize,
        band_type: BandType,
    ) -> Result<(), rill_core::Error> {
        self.eq.set_band_type(index, band_type)?;
        Ok(())
    }

    /// Enable/disable band.
    pub fn set_band_enabled(
        &mut self,
        index: usize,
        enabled: bool,
    ) -> Result<(), rill_core::Error> {
        self.eq.set_band_enabled(index, enabled)?;
        Ok(())
    }

    /// Set output gain (linear).
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.max(0.0).min(4.0);
        self.eq.set_output_gain(self.output_gain);
    }

    /// Get number of bands.
    pub fn num_bands(&self) -> usize {
        self.num_bands
    }

    /// Get reference to inner equalizer.
    pub fn eq(&self) -> &ParametricEq<Biquad<f32>, BiquadFactory> {
        &self.eq
    }

    /// Get mutable reference to inner equalizer.
    pub fn eq_mut(&mut self) -> &mut ParametricEq<Biquad<f32>, BiquadFactory> {
        &mut self.eq
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
    for ParametricEqProcessor<T, BUF_SIZE>
{
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
        self.eq.init(sample_rate);
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.eq.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        if name == "output_gain" {
            return Some(ParamValue::Float(self.output_gain));
        }

        // Parse band parameter: band_<index>_<field>
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() >= 3 && parts[0] == "band" {
            if let Ok(index) = parts[1].parse::<usize>() {
                if index < self.num_bands {
                    let field = parts[2];
                    match field {
                        "freq" => {
                            return self.eq.get_band_frequency(index).map(ParamValue::Float);
                        }
                        "q" => {
                            return self.eq.get_band_q(index).map(ParamValue::Float);
                        }
                        "gain" => {
                            return self.eq.get_band_gain(index).map(ParamValue::Float);
                        }
                        "enabled" => {
                            return self.eq.get_band_enabled(index).map(ParamValue::Bool);
                        }
                        _ => {}
                    }
                }
            }
        }

        None
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if name == "output_gain" {
            if let Some(v) = value.as_f32() {
                self.set_output_gain(v);
                Ok(())
            } else {
                Err(ProcessError::parameter("Expected float value"))
            }
        } else {
            // Parse band parameter
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 && parts[0] == "band" {
                if let Ok(index) = parts[1].parse::<usize>() {
                    if index >= self.num_bands {
                        return Err(ProcessError::parameter(format!(
                            "Band index {} out of range",
                            index
                        )));
                    }
                    let field = parts[2];
                    match field {
                        "freq" => {
                            if let Some(v) = value.as_f32() {
                                self.eq
                                    .set_band(
                                        index,
                                        v,
                                        self.eq.get_band_q(index).unwrap_or(1.0),
                                        self.eq.get_band_gain(index).unwrap_or(0.0),
                                    )
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected float value"))
                            }
                        }
                        "q" => {
                            if let Some(v) = value.as_f32() {
                                self.eq
                                    .set_band(
                                        index,
                                        self.eq.get_band_frequency(index).unwrap_or(1000.0),
                                        v,
                                        self.eq.get_band_gain(index).unwrap_or(0.0),
                                    )
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected float value"))
                            }
                        }
                        "gain" => {
                            if let Some(v) = value.as_f32() {
                                self.eq
                                    .set_band(
                                        index,
                                        self.eq.get_band_frequency(index).unwrap_or(1000.0),
                                        self.eq.get_band_q(index).unwrap_or(1.0),
                                        v,
                                    )
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected float value"))
                            }
                        }
                        "enabled" => {
                            if let Some(b) = value.as_bool() {
                                self.eq
                                    .set_band_enabled(index, b)
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected boolean value"))
                            }
                        }
                        _ => Err(ProcessError::parameter(format!(
                            "Unknown band field: {}",
                            field
                        ))),
                    }
                } else {
                    Err(ProcessError::parameter(format!(
                        "Invalid band index: {}",
                        parts[1]
                    )))
                }
            } else {
                Err(ProcessError::parameter(format!(
                    "Unknown parameter: {}",
                    name
                )))
            }
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

impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for ParametricEqProcessor<T, BUF_SIZE>
{
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
            // Convert input from T to f32
            let mut input_f32 = [0.0f32; BUF_SIZE];
            for i in 0..BUF_SIZE {
                input_f32[i] = input_buffer[i].to_f32();
            }

            // Process through equalizer
            let mut output_f32 = [0.0f32; BUF_SIZE];
            self.eq.process_block(&input_f32, &mut output_f32);

            // Convert output back to T
            for i in 0..BUF_SIZE {
                output_buffer[i] = T::from_f32(output_f32[i]);
            }
        }

        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}

/// Graphic equalizer processor node for audio graphs.
pub struct GraphicEqProcessor<T: AudioNum, const BUF_SIZE: usize> {
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
    /// Inner graphic equalizer (works with f32)
    eq: GraphicEq<Biquad<f32>, BiquadFactory>,
    /// Output gain (linear)
    pub output_gain: f32,
    /// Number of bands
    num_bands: usize,
}

impl<T: AudioNum, const BUF_SIZE: usize> GraphicEqProcessor<T, BUF_SIZE> {
    /// Creates a new graphic equalizer processor with ISO 1/3 octave bands.
    pub fn new_third_octave(sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("GraphicEqProcessor", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let factory = BiquadFactory;
        let mut eq = GraphicEq::new_third_octave(factory, sample_rate);
        eq.init(sample_rate);

        let num_bands = eq.num_bands();

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            eq,
            output_gain: 1.0,
            num_bands,
        }
    }

    /// Creates a new graphic equalizer processor with custom frequencies.
    pub fn with_frequencies(frequencies: Vec<f32>, sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("GraphicEqProcessor", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let factory = BiquadFactory;
        let mut eq = GraphicEq::with_frequencies(factory, frequencies, sample_rate);
        eq.init(sample_rate);

        let num_bands = eq.num_bands();

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            eq,
            output_gain: 1.0,
            num_bands,
        }
    }

    /// Set gain for a specific band (in dB).
    pub fn set_band_gain(&mut self, index: usize, gain_db: f32) -> Result<(), rill_core::Error> {
        self.eq.set_band_gain(index, gain_db)?;
        Ok(())
    }

    /// Enable/disable band.
    pub fn set_band_enabled(
        &mut self,
        index: usize,
        enabled: bool,
    ) -> Result<(), rill_core::Error> {
        self.eq.set_band_enabled(index, enabled)?;
        Ok(())
    }

    /// Set output gain (linear).
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.max(0.0).min(4.0);
        self.eq.set_output_gain(self.output_gain);
    }

    /// Get number of bands.
    pub fn num_bands(&self) -> usize {
        self.num_bands
    }

    /// Get reference to inner equalizer.
    pub fn eq(&self) -> &GraphicEq<Biquad<f32>, BiquadFactory> {
        &self.eq
    }

    /// Get mutable reference to inner equalizer.
    pub fn eq_mut(&mut self) -> &mut GraphicEq<Biquad<f32>, BiquadFactory> {
        &mut self.eq
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
    for GraphicEqProcessor<T, BUF_SIZE>
{
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
    }

    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        self.eq.init(sample_rate);
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.eq.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        if name == "output_gain" {
            return Some(ParamValue::Float(self.output_gain));
        }

        // Parse band parameter: band_<index>_<field>
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() >= 3 && parts[0] == "band" {
            if let Ok(index) = parts[1].parse::<usize>() {
                if index < self.num_bands {
                    let field = parts[2];
                    match field {
                        "gain" => {
                            return self.eq.get_band_gain(index).map(ParamValue::Float);
                        }
                        "enabled" => {
                            return self.eq.get_band_enabled(index).map(ParamValue::Bool);
                        }
                        _ => {}
                    }
                }
            }
        }

        None
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if name == "output_gain" {
            if let Some(v) = value.as_f32() {
                self.set_output_gain(v);
                Ok(())
            } else {
                Err(ProcessError::parameter("Expected float value"))
            }
        } else {
            // Parse band parameter
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 && parts[0] == "band" {
                if let Ok(index) = parts[1].parse::<usize>() {
                    if index >= self.num_bands {
                        return Err(ProcessError::parameter(format!(
                            "Band index {} out of range",
                            index
                        )));
                    }
                    let field = parts[2];
                    match field {
                        "gain" => {
                            if let Some(v) = value.as_f32() {
                                self.eq
                                    .set_band_gain(index, v)
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected float value"))
                            }
                        }
                        "enabled" => {
                            if let Some(b) = value.as_bool() {
                                self.eq
                                    .set_band_enabled(index, b)
                                    .map_err(|e| ProcessError::parameter(e.to_string()))?;
                                Ok(())
                            } else {
                                Err(ProcessError::parameter("Expected boolean value"))
                            }
                        }
                        _ => Err(ProcessError::parameter(format!(
                            "Unknown band field: {}",
                            field
                        ))),
                    }
                } else {
                    Err(ProcessError::parameter(format!(
                        "Invalid band index: {}",
                        parts[1]
                    )))
                }
            } else {
                Err(ProcessError::parameter(format!(
                    "Unknown parameter: {}",
                    name
                )))
            }
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

impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for GraphicEqProcessor<T, BUF_SIZE>
{
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

        if let (Some(input_buffer), Some(output_buffer)) =
            (audio_inputs.first(), audio_outputs.first_mut())
        {
            // Convert input from T to f32
            let mut input_f32 = [0.0f32; BUF_SIZE];
            for i in 0..BUF_SIZE {
                input_f32[i] = input_buffer[i].to_f32();
            }

            // Process through equalizer
            let mut output_f32 = [0.0f32; BUF_SIZE];
            self.eq.process_block(&input_f32, &mut output_f32);

            // Convert output back to T
            for i in 0..BUF_SIZE {
                output_buffer[i] = T::from_f32(output_f32[i]);
            }
        }

        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}
