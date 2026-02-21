//! Common filter traits and types for DSP filters

use kama_core_traits::AudioNode;

/// Type of filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
    AllPass,
}

impl FilterType {
    /// Get filter type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "lowpass" | "low_pass" => Some(FilterType::LowPass),
            "highpass" | "high_pass" => Some(FilterType::HighPass),
            "bandpass" | "band_pass" => Some(FilterType::BandPass),
            "notch" => Some(FilterType::Notch),
            "peak" => Some(FilterType::Peak),
            "lowshelf" | "low_shelf" => Some(FilterType::LowShelf),
            "highshelf" | "high_shelf" => Some(FilterType::HighShelf),
            "allpass" | "all_pass" => Some(FilterType::AllPass),
            _ => None,
        }
    }
    
    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterType::LowPass => "lowpass",
            FilterType::HighPass => "highpass",
            FilterType::BandPass => "bandpass",
            FilterType::Notch => "notch",
            FilterType::Peak => "peak",
            FilterType::LowShelf => "lowshelf",
            FilterType::HighShelf => "highshelf",
            FilterType::AllPass => "allpass",
        }
    }
}

/// Common trait for all filters
pub trait Filter: AudioNode {
    /// Set cutoff frequency in Hz
    fn set_cutoff(&mut self, freq: f32);
    
    /// Get current cutoff frequency
    fn cutoff(&self) -> f32;
    
    /// Set Q factor (resonance)
    fn set_q(&mut self, q: f32);
    
    /// Get current Q factor
    fn q(&self) -> f32;
    
    /// Set gain in dB (for peak/shelving filters)
    fn set_gain_db(&mut self, gain: f32);
    
    /// Get current gain in dB
    fn gain_db(&self) -> f32;
    
    /// Get filter type
    fn filter_type(&self) -> FilterType;
    
    /// Reset filter state
    fn reset_filter(&mut self);
}

/// Factory for creating filters
pub trait FilterFactory<F: Filter> {
    /// Create a new filter
    fn create_filter(&self, filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> F;
    
    /// Get factory name (for metadata)
    fn factory_name(&self) -> &str;
}