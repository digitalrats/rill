//! Parametric equalizer implementation

use crate::band::{BandType, EqBand};
use crate::utils::parse_band_param;
use crate::FilterFactory;
use kama_core::traits::{
    ParamMetadata, ParamType, ParamRange, AudioError, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
    ParameterId, DynProcessor, Processor,
};
use kama_core::{ProcessResult, ProcessError, DEFAULT_BLOCK_SIZE};
use kama_core_dsp::filters::{Filter, FilterType};
use std::sync::Arc;

#[cfg(feature = "automation")]
use kama_core::signal::{ParameterChanged, SignalBus, SignalSource};

/// Parametric equalizer with configurable bands
///
/// Uses a filter factory to create filters for each band.
/// Can work with any filter implementation (digital, analog, etc.)
pub struct ParametricEq<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> {
    /// Filter factory
    factory: Factory,
    /// EQ bands
    bands: Vec<EqBand<F>>,
    /// Sample rate
    sample_rate: f32,
    /// Output gain
    output_gain: f32,
    /// Band names for automation
    band_names: Vec<String>,

    #[cfg(feature = "automation")]
    /// Signal bus for automation
    signal_bus: Option<SignalBus<ParameterChanged>>,
}

impl<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static>
    ParametricEq<F, Factory>
{
    /// Create a new parametric equalizer with specified number of bands
    pub fn new(factory: Factory, num_bands: usize, sample_rate: f32) -> Self {
        let mut eq = Self {
            factory,
            bands: Vec::with_capacity(num_bands),
            sample_rate,
            output_gain: 1.0,
            band_names: (0..num_bands).map(|i| format!("band_{}", i)).collect(),

            #[cfg(feature = "automation")]
            signal_bus: None,
        };

        // Initialize bands with reasonable default frequencies
        for i in 0..num_bands {
            // Logarithmic spacing from 20Hz to 20kHz
            let freq = if num_bands > 1 {
                20.0 * (1000.0_f32).powf(i as f32 / (num_bands - 1) as f32)
            } else {
                1000.0
            };
            let band = EqBand::new(
                eq.factory.create_filter(FilterType::Peak, freq, 1.0, 0.0),
                BandType::Peak,
                freq,
                1.0,
                0.0,
            );
            eq.bands.push(band);
        }

        eq
    }

    /// Set parameters for a specific band
    pub fn set_band(
        &mut self,
        index: usize,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!(
                "Band index {} out of range",
                index
            )));
        }

        println!("DEBUG set_band: idx={}, freq={}, q={}, gain_db={}", index, frequency, q, gain_db);

        let band = &mut self.bands[index];
        band.set_frequency(frequency);
        band.set_q(q);
        band.set_gain_db(gain_db);
        band.update_filter();

        #[cfg(feature = "automation")]
        self.send_band_update(index);

        Ok(())
    }

    /// Set band type
    pub fn set_band_type(&mut self, index: usize, band_type: BandType) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!(
                "Band index {} out of range",
                index
            )));
        }

        let band = &mut self.bands[index];
        band.band_type = band_type;
        band.update_filter();

        #[cfg(feature = "automation")]
        self.send_band_update(index);

        Ok(())
    }

    /// Enable/disable band
    pub fn set_band_enabled(&mut self, index: usize, enabled: bool) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!(
                "Band index {} out of range",
                index
            )));
        }

        self.bands[index].set_enabled(enabled);

        #[cfg(feature = "automation")]
        self.send_band_update(index);

        Ok(())
    }

    /// Set output gain
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.max(0.0).min(4.0);

        #[cfg(feature = "automation")]
        self.send_parameter_update("output_gain", self.output_gain);
    }

    /// Get band frequency
    pub fn get_band_frequency(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|b| b.frequency())
    }

    /// Get band Q
    pub fn get_band_q(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|b| b.q())
    }

    /// Get band gain
    pub fn get_band_gain(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|b| b.gain_db())
    }

    /// Get band type
    pub fn get_band_type(&self, index: usize) -> Option<BandType> {
        self.bands.get(index).map(|b| b.band_type())
    }

    #[cfg(feature = "automation")]
    /// Connect to signal bus for automation
    pub fn connect_signals(&mut self, bus: SignalBus<ParameterChanged>) {
        self.signal_bus = Some(bus);
    }

    #[cfg(feature = "automation")]
    fn send_band_update(&self, index: usize) {
        if let Some(bus) = &self.signal_bus {
            if let Some(band) = self.bands.get(index) {
                let signal = ParameterChanged {
                    node_id: "eq".to_string(),
                    parameter_id: format!("band_{}_freq", index),
                    value: band.frequency(),
                    normalized_value: (band.frequency() - 20.0) / (20000.0 - 20.0),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    source: SignalSource::Automation,
                };
                let _ = bus.send(signal);

                let signal = ParameterChanged {
                    node_id: "eq".to_string(),
                    parameter_id: format!("band_{}_gain", index),
                    value: band.gain_db(),
                    normalized_value: (band.gain_db() + 24.0) / 48.0,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    source: SignalSource::Automation,
                };
                let _ = bus.send(signal);
            }
        }
    }

    #[cfg(feature = "automation")]
    fn send_parameter_update(&self, param: &str, value: f32) {
        if let Some(bus) = &self.signal_bus {
            let signal = ParameterChanged {
                node_id: "eq".to_string(),
                parameter_id: param.to_string(),
                value,
                normalized_value: value / 4.0, // output_gain max 4.0
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
                source: SignalSource::Automation,
            };
            let _ = bus.send(signal);
        }
    }
}

impl<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> Processor<f32, DEFAULT_BLOCK_SIZE>
    for ParametricEq<F, Factory>
{
    fn process(
        &mut self,
        inputs: &[&[f32; DEFAULT_BLOCK_SIZE]],
        outputs: &mut [&mut [f32; DEFAULT_BLOCK_SIZE]],
        _control: &[f32],
    ) -> ProcessResult<()> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }

        let input = inputs[0];
        let output = &mut outputs[0];

        for i in 0..DEFAULT_BLOCK_SIZE {
            let mut sample = input[i];
            for band in &mut self.bands {
                if band.is_enabled() {
                    sample = band.process(sample);
                }
            }
            output[i] = sample * self.output_gain;
        }

        Ok(())
    }

    fn num_audio_inputs(&self) -> usize {
        1
    }

    fn num_audio_outputs(&self) -> usize {
        1
    }

    fn num_control_inputs(&self) -> usize {
        0
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "num_bands" => Some(ParamValue::Int(self.bands.len() as i32)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            _ => {
                if let Some((index, param)) = parse_band_param(id.as_str()) {
                    if let Some(band) = self.bands.get(index) {
                        match param {
                            "freq" | "frequency" => Some(ParamValue::Float(band.frequency())),
                            "q" => Some(ParamValue::Float(band.q())),
                            "gain" => Some(ParamValue::Float(band.gain_db())),
                            "enabled" => Some(ParamValue::Bool(band.is_enabled())),
                            "type" => {
                                Some(ParamValue::Choice(band.band_type().as_str().to_string()))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    fn set_parameter(
        &mut self,
        id: &ParameterId,
        value: ParamValue,
    ) -> ProcessResult<()> {
        match (id.as_str(), value) {
            ("output_gain", ParamValue::Float(g)) => {
                self.set_output_gain(g);
                Ok(())
            }
            (name, val) => {
                if let Some((index, param)) = parse_band_param(name) {
                    if index >= self.bands.len() {
                        return Err(kama_core::ProcessError::Parameter(format!(
                            "Band index {} out of range",
                            index
                        )));
                    }
                    match param {
                        "freq" | "frequency" => {
                            if let ParamValue::Float(f) = val {
                                self.bands[index].set_frequency(f);
                                self.bands[index].update_filter();
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                Ok(())
                            } else {
                                Err(kama_core::ProcessError::Parameter("Expected float".into()))
                            }
                        }
                        "q" => {
                            if let ParamValue::Float(q) = val {
                                self.bands[index].set_q(q);
                                self.bands[index].update_filter();
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                Ok(())
                            } else {
                                Err(kama_core::ProcessError::Parameter("Expected float".into()))
                            }
                        }
                        "gain" => {
                            if let ParamValue::Float(g) = val {
                                self.bands[index].set_gain_db(g);
                                self.bands[index].update_filter();
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                Ok(())
                            } else {
                                Err(kama_core::ProcessError::Parameter("Expected float".into()))
                            }
                        }
                        "enabled" => {
                            if let ParamValue::Bool(e) = val {
                                self.bands[index].set_enabled(e);
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                Ok(())
                            } else {
                                Err(kama_core::ProcessError::Parameter("Expected bool".into()))
                            }
                        }
                        "type" => {
                            if let ParamValue::Choice(type_str) = val {
                                if let Some(band_type) = BandType::from_str(&type_str) {
                                    self.bands[index].band_type = band_type;
                                    self.bands[index].update_filter();
                                    #[cfg(feature = "automation")]
                                    self.send_band_update(index);
                                    Ok(())
                                } else {
                                    Err(kama_core::ProcessError::Parameter(format!(
                                        "Unknown band type: {}",
                                        type_str
                                    )))
                                }
                            } else {
                                Err(kama_core::ProcessError::Parameter("Expected choice".into()))
                            }
                        }
                        _ => Err(kama_core::ProcessError::Parameter(format!(
                            "Unknown band parameter: {}",
                            param
                        ))),
                    }
                } else {
                    Err(kama_core::ProcessError::Parameter(format!(
                        "Unknown parameter: {}",
                        name
                    )))
                }
            }
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for band in &mut self.bands {
            band.filter.init(sample_rate);
        }
    }

    fn reset(&mut self) {
        for band in &mut self.bands {
            band.filter.reset();
        }
    }
}
