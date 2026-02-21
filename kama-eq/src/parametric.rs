//! Parametric equalizer implementation

use std::sync::Arc;
use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use kama_dsp_common::filter::{Filter, FilterFactory, FilterType};
use crate::band::{EqBand, BandType};
use crate::utils::parse_band_param;

#[cfg(feature = "automation")]
use kama_signal::{SignalBus, ParameterChanged, SignalSource};

/// Parametric equalizer with configurable bands
///
/// Uses a filter factory to create filters for each band.
/// Can work with any filter implementation (digital, analog, etc.)
pub struct ParametricEq<F: Filter, Factory: FilterFactory<F>> {
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

impl<F: Filter, Factory: FilterFactory<F>> ParametricEq<F, Factory> {
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
            let freq = 20.0 * (1000.0f32).powf(i as f32 / (num_bands - 1) as f32);
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
            return Err(AudioError::Parameter(format!("Band index {} out of range", index)));
        }
        
        let band = &mut self.bands[index];
        band.set_frequency(frequency);
        band.set_q(q);
        band.set_gain_db(gain_db);
        band.update_filter(&self.factory);
        
        #[cfg(feature = "automation")]
        self.send_band_update(index);
        
        Ok(())
    }
    
    /// Set band type
    pub fn set_band_type(&mut self, index: usize, band_type: BandType) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!("Band index {} out of range", index)));
        }
        
        let band = &mut self.bands[index];
        band.band_type = band_type;
        band.update_filter(&self.factory);
        
        #[cfg(feature = "automation")]
        self.send_band_update(index);
        
        Ok(())
    }
    
    /// Enable/disable band
    pub fn set_band_enabled(&mut self, index: usize, enabled: bool) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!("Band index {} out of range", index)));
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
                    value: band.frequency,
                    normalized_value: (band.frequency - 20.0) / (20000.0 - 20.0),
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
                    value: band.gain_db,
                    normalized_value: (band.gain_db + 24.0) / 48.0,
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

impl<F: Filter, Factory: FilterFactory<F>> AudioNode for ParametricEq<F, Factory> {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let buffer_size = input.len().min(output.len());
        
        // Process through all bands sequentially
        for i in 0..buffer_size {
            let mut sample = input[i];
            
            for band in &mut self.bands {
                sample = band.process(sample);
            }
            
            output[i] = sample * self.output_gain;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "num_bands" => Some(ParamValue::Int(self.bands.len() as i32)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            _ => {
                if let Some((index, param)) = parse_band_param(name) {
                    if let Some(band) = self.bands.get(index) {
                        match param {
                            "freq" | "frequency" => Some(ParamValue::Float(band.frequency())),
                            "q" => Some(ParamValue::Float(band.q())),
                            "gain" => Some(ParamValue::Float(band.gain_db())),
                            "enabled" => Some(ParamValue::Bool(band.is_enabled())),
                            "type" => Some(ParamValue::Choice(band.band_type().as_str().to_string())),
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
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("output_gain", ParamValue::Float(g)) => {
                self.set_output_gain(g);
                Ok(())
            }
            _ => {
                if let Some((index, param)) = parse_band_param(name) {
                    if index >= self.bands.len() {
                        return Err(AudioError::Parameter(format!("Band index {} out of range", index)));
                    }
                    
                    match param {
                        "freq" | "frequency" => {
                            if let ParamValue::Float(f) = value {
                                self.bands[index].set_frequency(f);
                                self.bands[index].update_filter(&self.factory);
                                
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                
                                Ok(())
                            } else {
                                Err(AudioError::Parameter("Expected float".into()))
                            }
                        }
                        "q" => {
                            if let ParamValue::Float(q) = value {
                                self.bands[index].set_q(q);
                                self.bands[index].update_filter(&self.factory);
                                
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                
                                Ok(())
                            } else {
                                Err(AudioError::Parameter("Expected float".into()))
                            }
                        }
                        "gain" => {
                            if let ParamValue::Float(g) = value {
                                self.bands[index].set_gain_db(g);
                                self.bands[index].update_filter(&self.factory);
                                
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                
                                Ok(())
                            } else {
                                Err(AudioError::Parameter("Expected float".into()))
                            }
                        }
                        "enabled" => {
                            if let ParamValue::Bool(e) = value {
                                self.bands[index].set_enabled(e);
                                
                                #[cfg(feature = "automation")]
                                self.send_band_update(index);
                                
                                Ok(())
                            } else {
                                Err(AudioError::Parameter("Expected bool".into()))
                            }
                        }
                        "type" => {
                            if let ParamValue::Choice(t) = value {
                                if let Some(band_type) = BandType::from_str(&t) {
                                    self.bands[index].band_type = band_type;
                                    self.bands[index].update_filter(&self.factory);
                                    
                                    #[cfg(feature = "automation")]
                                    self.send_band_update(index);
                                    
                                    Ok(())
                                } else {
                                    Err(AudioError::Parameter(format!("Unknown band type: {}", t)))
                                }
                            } else {
                                Err(AudioError::Parameter("Expected choice".into()))
                            }
                        }
                        _ => Err(AudioError::Parameter(format!("Unknown band parameter: {}", param))),
                    }
                } else {
                    Err(AudioError::Parameter(format!("Unknown parameter: {}", name)))
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
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        let mut params = vec![
            ParamMetadata {
                name: "num_bands".to_string(),
                typ: ParamType::Int,
                default: ParamValue::Int(self.bands.len() as i32),
                min: Some(1.0),
                max: Some(32.0),
                step: Some(1.0),
                unit: None,
                choices: None,
            },
            ParamMetadata {
                name: "output_gain".to_string(),
                typ: ParamType::Float,
                default: ParamValue::Float(1.0),
                min: Some(0.0),
                max: Some(4.0),
                step: Some(0.1),
                unit: Some("gain".to_string()),
                choices: None,
            },
        ];
        
        // Add band parameters to metadata
        for i in 0..self.bands.len() {
            params.push(ParamMetadata {
                name: format!("band_{}_freq", i),
                typ: ParamType::Float,
                default: ParamValue::Float(self.bands[i].frequency()),
                min: Some(20.0),
                max: Some(20000.0),
                step: Some(1.0),
                unit: Some("Hz".to_string()),
                choices: None,
            });
            
            params.push(ParamMetadata {
                name: format!("band_{}_gain", i),
                typ: ParamType::Float,
                default: ParamValue::Float(self.bands[i].gain_db()),
                min: Some(-24.0),
                max: Some(24.0),
                step: Some(0.5),
                unit: Some("dB".to_string()),
                choices: None,
            });
            
            params.push(ParamMetadata {
                name: format!("band_{}_q", i),
                typ: ParamType::Float,
                default: ParamValue::Float(self.bands[i].q()),
                min: Some(0.1),
                max: Some(20.0),
                step: Some(0.1),
                unit: Some("Q".to_string()),
                choices: None,
            });
            
            params.push(ParamMetadata {
                name: format!("band_{}_enabled", i),
                typ: ParamType::Bool,
                default: ParamValue::Bool(true),
                min: None,
                max: None,
                step: None,
                unit: None,
                choices: None,
            });
            
            params.push(ParamMetadata {
                name: format!("band_{}_type", i),
                typ: ParamType::Choice,
                default: ParamValue::Choice("peak".to_string()),
                min: None,
                max: None,
                step: None,
                unit: None,
                choices: Some(vec![
                    ("peak".to_string(), 0.0),
                    ("low_shelf".to_string(), 1.0),
                    ("high_shelf".to_string(), 2.0),
                    ("low_pass".to_string(), 3.0),
                    ("high_pass".to_string(), 4.0),
                    ("band_pass".to_string(), 5.0),
                    ("notch".to_string(), 6.0),
                ]),
            });
        }
        
        NodeMetadata {
            name: format!("Parametric EQ ({} bands)", self.bands.len()),
            category: NodeCategory::Filter,
            description: format!("Parametric equalizer using {} filters", self.factory.factory_name()),
            author: "Kama EQ".to_string(),
            version: "0.1.0".to_string(),
            parameters: params,
        }
    }
}