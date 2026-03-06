//! Equalizer band implementation

use kama_core::traits::{AudioError, ParamValue};
use kama_core_dsp::filters::{Filter, FilterParams, FilterType};

/// Type of EQ band
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BandType {
    /// Peaking/parametric band
    Peak,
    /// Low shelf filter
    LowShelf,
    /// High shelf filter
    HighShelf,
    /// Low pass filter
    LowPass,
    /// High pass filter
    HighPass,
    /// Band pass filter
    BandPass,
    /// Notch filter
    Notch,
}

impl BandType {
    /// Get band type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "peak" => Some(BandType::Peak),
            "lowshelf" | "low_shelf" => Some(BandType::LowShelf),
            "highshelf" | "high_shelf" => Some(BandType::HighShelf),
            "lowpass" | "low_pass" => Some(BandType::LowPass),
            "highpass" | "high_pass" => Some(BandType::HighPass),
            "bandpass" | "band_pass" => Some(BandType::BandPass),
            "notch" => Some(BandType::Notch),
            _ => None,
        }
    }

    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            BandType::Peak => "peak",
            BandType::LowShelf => "low_shelf",
            BandType::HighShelf => "high_shelf",
            BandType::LowPass => "low_pass",
            BandType::HighPass => "high_pass",
            BandType::BandPass => "band_pass",
            BandType::Notch => "notch",
        }
    }

    /// Convert to FilterType
    pub fn to_filter_type(&self) -> FilterType {
        match self {
            BandType::Peak => FilterType::Peak,
            BandType::LowShelf => FilterType::LowShelf,
            BandType::HighShelf => FilterType::HighShelf,
            BandType::LowPass => FilterType::LowPass,
            BandType::HighPass => FilterType::HighPass,
            BandType::BandPass => FilterType::BandPass,
            BandType::Notch => FilterType::Notch,
        }
    }
}

/// A single band of an equalizer
pub struct EqBand<F: Filter<f32>> {
    /// The filter for this band
    pub(crate) filter: F,
    /// Center/corner frequency in Hz
    pub(crate) frequency: f32,
    /// Quality factor (for peaking/parametric bands)
    pub(crate) q: f32,
    /// Gain in dB (for peaking/shelving bands)
    pub(crate) gain_db: f32,
    /// Whether this band is enabled
    pub(crate) enabled: bool,
    /// Band type
    pub(crate) band_type: BandType,
}

impl<F: Filter<f32>> EqBand<F> {
    /// Create a new EQ band
    pub fn new(filter: F, band_type: BandType, frequency: f32, q: f32, gain_db: f32) -> Self {
        Self {
            filter,
            band_type,
            frequency,
            q,
            gain_db,
            enabled: true,
        }
    }

    /// Process a single sample through this band
    pub fn process(&mut self, input: f32) -> f32 {
        if !self.enabled {
            return input;
        }

        // Создаём входной буфер как срез
        let input_slice = [input];
        let mut output = [0.0];

        // Вызываем process_block для фильтра
        self.filter.process_block(&input_slice, &mut output);
        output[0]
    }

    /// Update the filter with current parameters
    pub fn update_filter(&mut self) {
        let params = FilterParams {
            filter_type: self.band_type.to_filter_type(),
            cutoff: self.frequency,
            q: self.q,
            gain_db: self.gain_db,
        };
        println!("DEBUG update_filter: type={:?}, cutoff={}, q={}, gain_db={}", params.filter_type, params.cutoff, params.q, params.gain_db);
        self.filter.set_params(params);
    }

    /// Set frequency
    pub fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq.max(20.0).min(20000.0);
    }

    /// Set Q factor
    pub fn set_q(&mut self, q: f32) {
        self.q = q.max(0.1).min(20.0);
    }

    /// Set gain in dB
    pub fn set_gain_db(&mut self, gain: f32) {
        self.gain_db = gain.max(-24.0).min(24.0);
    }

    /// Enable/disable band
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get current frequency
    pub fn frequency(&self) -> f32 {
        self.frequency
    }

    /// Get current Q
    pub fn q(&self) -> f32 {
        self.q
    }

    /// Get current gain in dB
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Check if band is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get band type
    pub fn band_type(&self) -> BandType {
        self.band_type
    }
}
