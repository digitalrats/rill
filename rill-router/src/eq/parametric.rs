//! Parametric equalizer implementation

use super::band::{BandType, EqBand};
use super::FilterFactory;
use crate::{Filter, FilterType};
use rill_core::{Error, ErrorCode};

/// Parametric equalizer with configurable bands
///
/// Uses a filter factory to create filters for each band.
/// Can work with any filter implementation (digital, analog, etc.)
pub struct ParametricEq<F: Filter<f32> + 'static, Factory: FilterFactory<F> + Send + Sync + 'static>
{
    /// Filter factory
    factory: Factory,
    /// EQ bands
    bands: Vec<EqBand<F>>,
    /// Sample rate
    sample_rate: f32,
    /// Output gain
    output_gain: f32,
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
    ) -> Result<(), Error> {
        if index >= self.bands.len() {
            return Err(Error::new(
                ErrorCode::InvalidParameter,
                format!("Band index {} out of range", index),
            ));
        }

        let band = &mut self.bands[index];
        band.set_frequency(frequency);
        band.set_q(q);
        band.set_gain_db(gain_db);
        band.update_filter();

        Ok(())
    }

    /// Set band type
    pub fn set_band_type(&mut self, index: usize, band_type: BandType) -> Result<(), Error> {
        if index >= self.bands.len() {
            return Err(Error::new(
                ErrorCode::InvalidParameter,
                format!("Band index {} out of range", index),
            ));
        }

        let band = &mut self.bands[index];
        band.band_type = band_type;
        band.update_filter();

        Ok(())
    }

    /// Enable/disable band
    pub fn set_band_enabled(&mut self, index: usize, enabled: bool) -> Result<(), Error> {
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
            let mut sample = input[i];
            for band in &mut self.bands {
                if band.is_enabled() {
                    sample = band.process(sample);
                }
            }
            output[i] = sample * self.output_gain;
        }
    }
}
