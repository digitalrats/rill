//! Graphic equalizer implementation

use crate::band::{BandType, EqBand};
use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};
use kama_dsp_common::filter::{Filter, FilterFactory, FilterType};

/// Graphic equalizer with fixed frequency bands
///
/// Common configurations:
/// - 10-band (31.25, 62.5, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz)
/// - 31-band (1/3 octave)
pub struct GraphicEq<F: Filter + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> {
    /// Filter factory
    factory: Factory,
    /// EQ bands
    bands: Vec<EqBand<F>>,
    /// Center frequencies
    frequencies: Vec<f32>,
    /// Sample rate
    sample_rate: f32,
    /// Output gain
    output_gain: f32,
}

impl<F: Filter + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> GraphicEq<F, Factory> {
    /// Create a new graphic equalizer with ISO 1/3 octave frequencies
    pub fn new_third_octave(factory: Factory, sample_rate: f32) -> Self {
        let frequencies = vec![
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0,
            400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0,
            5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
        ];

        Self::with_frequencies(factory, frequencies, sample_rate)
    }

    /// Create a new graphic equalizer with custom frequencies
    pub fn with_frequencies(factory: Factory, frequencies: Vec<f32>, sample_rate: f32) -> Self {
        let mut bands = Vec::with_capacity(frequencies.len());

        for &freq in &frequencies {
            let band = EqBand::new(
                factory.create_filter(FilterType::Peak, freq, 1.0, 0.0),
                BandType::Peak,
                freq,
                1.0,
                0.0,
            );
            bands.push(band);
        }

        Self {
            factory,
            bands,
            frequencies,
            sample_rate,
            output_gain: 1.0,
        }
    }

    /// Set gain for a specific band (in dB)
    pub fn set_band_gain(&mut self, index: usize, gain_db: f32) -> Result<(), AudioError> {
        if index >= self.bands.len() {
            return Err(AudioError::Parameter(format!(
                "Band index {} out of range",
                index
            )));
        }

        let band = &mut self.bands[index];
        band.set_gain_db(gain_db);
        band.update_filter(&self.factory);

        Ok(())
    }

    /// Set output gain
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.max(0.0).min(4.0);
    }

    /// Get frequency for a band
    pub fn band_frequency(&self, index: usize) -> Option<f32> {
        self.frequencies.get(index).copied()
    }

    /// Number of bands
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }
}

impl<F: Filter + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> AudioNode
    for GraphicEq<F, Factory>
{
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }

        let input = inputs[0];
        let output = &mut outputs[0];
        let buffer_size = input.len().min(output.len());

        // Process through all bands in parallel (for graphic EQ, bands are in parallel)
        // This is a simplification - real graphic EQ needs proper parallel processing
        for i in 0..buffer_size {
            let mut sample = 0.0;

            for band in &mut self.bands {
                sample += band.process(input[i]);
            }

            output[i] = (sample / self.bands.len() as f32) * self.output_gain;
        }

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "num_bands" => Some(ParamValue::Int(self.bands.len() as i32)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            _ => {
                if let Some(band_idx) = name
                    .strip_prefix("band_")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    if let Some(band) = self.bands.get(band_idx) {
                        return Some(ParamValue::Float(band.gain_db()));
                    }
                }
                None
            }
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value.clone()) {
            // <-- клонируем для проверки
            ("output_gain", ParamValue::Float(g)) => {
                self.set_output_gain(g);
                Ok(())
            }
            _ => {
                if let Some(band_idx) = name
                    .strip_prefix("band_")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    // Используем оригинальное значение для извлечения gain
                    if let ParamValue::Float(gain) = value {
                        self.set_band_gain(band_idx, gain)
                    } else {
                        Err(AudioError::Parameter("Expected float".into()))
                    }
                } else {
                    Err(AudioError::Parameter(format!(
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

    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }

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
                max: Some(31.0),
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

        // Add band gain parameters
        for (i, &freq) in self.frequencies.iter().enumerate() {
            params.push(ParamMetadata {
                name: format!("band_{}", i),
                typ: ParamType::Float,
                default: ParamValue::Float(0.0),
                min: Some(-24.0),
                max: Some(24.0),
                step: Some(0.5),
                unit: Some("dB".to_string()),
                choices: None,
            });
        }

        NodeMetadata {
            name: format!("Graphic EQ ({} bands)", self.bands.len()),
            category: NodeCategory::Filter,
            description: format!(
                "Graphic equalizer using {} filters",
                self.factory.factory_name()
            ),
            author: "Kama EQ".to_string(),
            version: "0.1.0".to_string(),
            parameters: params,
        }
    }
}
