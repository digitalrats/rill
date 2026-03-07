//! Graphic equalizer implementation

use crate::band::{BandType, EqBand};
use crate::FilterFactory;
use kama_core::traits::{
    ParamMetadata, ParamType, ParamRange,
    AudioError, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
    Processor, ParameterId,
};
use kama_core_dsp::filters::{Filter, FilterType};
use kama_core::{ProcessResult, DEFAULT_BLOCK_SIZE};

/// Graphic equalizer with fixed frequency bands
///
/// Common configurations:
/// - 10-band (31.25, 62.5, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz)
/// - 31-band (1/3 octave)
pub struct GraphicEq<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> {
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

impl<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> GraphicEq<F, Factory> {
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
        band.update_filter();

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

impl<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static> Processor<f32, DEFAULT_BLOCK_SIZE>
    for GraphicEq<F, Factory>
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
            let mut sample = 0.0;
            for band in &mut self.bands {
                sample += band.process(input[i]);
            }
            output[i] = (sample / self.bands.len() as f32) * self.output_gain;
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
                if let Some(band_idx) = id.as_str()
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
            (name, ParamValue::Float(gain)) => {
                if let Some(band_idx) = name
                    .strip_prefix("band_")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.set_band_gain(band_idx, gain).map_err(|e| kama_core::ProcessError::Parameter(e.to_string()))
                } else {
                    Err(kama_core::ProcessError::Parameter(format!("Unknown parameter: {}", name)))
                }
            }
            _ => Err(kama_core::ProcessError::Parameter("Expected float".into())),
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
