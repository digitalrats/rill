//! Graphic equalizer implementation

use super::band::{BandType, EqBand};
use super::FilterFactory;
use crate::{Filter, FilterType};
use rill_core::{Error, ErrorCode};

/// Graphic equalizer with fixed frequency bands
///
/// Common configurations:
/// - 10-band (31.25, 62.5, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz)
/// - 31-band (1/3 octave)
pub struct GraphicEq<F: Filter<f32> + 'static> {
    /// EQ bands
    bands: Vec<EqBand<F>>,
    /// Center frequencies
    frequencies: Vec<f32>,
    /// Sample rate
    sample_rate: f32,
    /// Output gain
    output_gain: f32,
}

impl<F: Filter<f32> + 'static> GraphicEq<F> {
    /// Create a new graphic equalizer with ISO 1/3 octave frequencies
    pub fn new_third_octave(factory: impl FilterFactory<F> + Send + Sync + 'static, sample_rate: f32) -> Self {
        let frequencies = vec![
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0,
            400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0,
            5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
        ];

        Self::with_frequencies(factory, frequencies, sample_rate)
    }

    /// Create a new graphic equalizer with custom frequencies
    pub fn with_frequencies(factory: impl FilterFactory<F> + Send + Sync + 'static, frequencies: Vec<f32>, sample_rate: f32) -> Self {
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
            bands,
            frequencies,
            sample_rate,
            output_gain: 1.0,
        }
    }

    /// Set gain for a specific band (in dB)
    pub fn set_band_gain(&mut self, index: usize, gain_db: f32) -> Result<(), rill_core::Error> {
        if index >= self.bands.len() {
            return Err(Error::new(
                ErrorCode::InvalidParameter,
                format!("Band index {} out of range", index),
            ));
        }

        let band = &mut self.bands[index];
        band.set_gain_db(gain_db);
        band.update_filter();

        Ok(())
    }

    /// Enable/disable band
    pub fn set_band_enabled(
        &mut self,
        index: usize,
        enabled: bool,
    ) -> Result<(), rill_core::Error> {
        if index >= self.bands.len() {
            return Err(Error::new(
                ErrorCode::InvalidParameter,
                format!("Band index {} out of range", index),
            ));
        }

        self.bands[index].set_enabled(enabled);
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

    /// Get band gain (dB)
    pub fn get_band_gain(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|b| b.gain_db())
    }

    /// Get band enabled state
    pub fn get_band_enabled(&self, index: usize) -> Option<bool> {
        self.bands.get(index).map(|b| b.is_enabled())
    }

    /// Number of bands
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    /// Initialize all bands with sample rate
    pub fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for band in &mut self.bands {
            band.init(sample_rate);
        }
    }

    /// Reset all bands
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }

    /// Process a block of samples
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        for i in 0..input.len() {
            let mut sample = 0.0;
            for band in &mut self.bands {
                sample += band.process(input[i]);
            }
            output[i] = (sample / self.bands.len() as f32) * self.output_gain;
        }
    }
}
