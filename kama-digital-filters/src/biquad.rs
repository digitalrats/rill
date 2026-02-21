//! Biquad filter implementation

use std::f32::consts::PI;
use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use kama_dsp_common::filter::{Filter, FilterType, FilterFactory};  // <-- импортируем из kama-dsp-common

/// Biquad filter coefficients
#[derive(Debug, Clone)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// Biquad filter implementation
///
/// Can be configured as:
/// - LowPass
/// - HighPass
/// - BandPass
/// - Notch
/// - Peak
/// - LowShelf
/// - HighShelf
pub struct BiquadFilter {
    /// Filter type
    filter_type: FilterType,
    /// Cutoff frequency in Hz
    cutoff: f32,
    /// Q factor (0.1 - 20.0)
    q: f32,
    /// Gain in dB (for peak/shelving filters)
    gain_db: f32,
    /// Sample rate in Hz
    sample_rate: f32,
    
    /// Filter coefficients
    coeffs: BiquadCoeffs,
    
    /// Filter state
    x1: f32, x2: f32,
    y1: f32, y2: f32,
}

impl BiquadFilter {
    /// Create a new biquad filter
    pub fn new(filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> Self {
        let mut filter = Self {
            filter_type,
            cutoff: cutoff.max(20.0).min(20000.0),
            q: q.max(0.1).min(20.0),
            gain_db: gain_db.max(-24.0).min(24.0),
            sample_rate: 44100.0,
            coeffs: BiquadCoeffs {
                b0: 1.0, b1: 0.0, b2: 0.0,
                a1: 0.0, a2: 0.0,
            },
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
        };
        filter.update_coeffs();
        filter
    }
    
    /// Update filter coefficients based on current parameters
    fn update_coeffs(&mut self) {
        let omega = 2.0 * PI * self.cutoff / self.sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * self.q);
        
        match self.filter_type {
            FilterType::LowPass => {
                let b0 = (1.0 - cos_omega) / 2.0;
                let b1 = 1.0 - cos_omega;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::HighPass => {
                let b0 = (1.0 + cos_omega) / 2.0;
                let b1 = -(1.0 + cos_omega);
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::BandPass => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::Notch => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::Peak => {
                let a = 10.0_f32.powf(self.gain_db / 40.0);
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha / a;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::LowShelf => {
                let a = 10.0_f32.powf(self.gain_db / 40.0);
                let beta = (a * alpha).sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_omega + 2.0 * beta * sin_omega);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_omega - 2.0 * beta * sin_omega);
                let a0 = (a + 1.0) + (a - 1.0) * cos_omega + 2.0 * beta * sin_omega;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) + (a - 1.0) * cos_omega - 2.0 * beta * sin_omega;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::HighShelf => {
                let a = 10.0_f32.powf(self.gain_db / 40.0);
                let beta = (a * alpha).sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_omega + 2.0 * beta * sin_omega);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_omega - 2.0 * beta * sin_omega);
                let a0 = (a + 1.0) - (a - 1.0) * cos_omega + 2.0 * beta * sin_omega;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) - (a - 1.0) * cos_omega - 2.0 * beta * sin_omega;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
            
            FilterType::AllPass => {
                let b0 = 1.0 - alpha;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0 + alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs.b0 = b0 / a0;
                self.coeffs.b1 = b1 / a0;
                self.coeffs.b2 = b2 / a0;
                self.coeffs.a1 = a1 / a0;
                self.coeffs.a2 = a2 / a0;
            }
        }
    }
    
    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let output = self.coeffs.b0 * input
            + self.coeffs.b1 * self.x1
            + self.coeffs.b2 * self.x2
            - self.coeffs.a1 * self.y1
            - self.coeffs.a2 * self.y2;
        
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        
        output
    }
}

impl Filter for BiquadFilter {
    fn set_cutoff(&mut self, freq: f32) {
        self.cutoff = freq.max(20.0).min(20000.0);
        self.update_coeffs();
    }
    
    fn cutoff(&self) -> f32 {
        self.cutoff
    }
    
    fn set_q(&mut self, q: f32) {
        self.q = q.max(0.1).min(20.0);
        self.update_coeffs();
    }
    
    fn q(&self) -> f32 {
        self.q
    }
    
    fn set_gain_db(&mut self, gain: f32) {
        self.gain_db = gain.max(-24.0).min(24.0);
        self.update_coeffs();
    }
    
    fn gain_db(&self) -> f32 {
        self.gain_db
    }
    
    fn filter_type(&self) -> FilterType {
        self.filter_type
    }
    
    fn reset_filter(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

impl AudioNode for BiquadFilter {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let len = input.len().min(output.len());
        
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "type" => Some(ParamValue::Choice(self.filter_type.as_str().to_string())),
            "cutoff" => Some(ParamValue::Float(self.cutoff)),
            "q" => Some(ParamValue::Float(self.q)),
            "gain_db" => Some(ParamValue::Float(self.gain_db)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("cutoff", ParamValue::Float(f)) => {
                self.set_cutoff(f);
                Ok(())
            }
            ("q", ParamValue::Float(q)) => {
                self.set_q(q);
                Ok(())
            }
            ("gain_db", ParamValue::Float(g)) => {
                self.set_gain_db(g);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
    }
    
    fn reset(&mut self) {
        self.reset_filter();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: format!("{:?} Biquad Filter", self.filter_type),
            category: NodeCategory::Filter,
            description: "Digital biquad filter".to_string(),
            author: "Kama Digital Filters".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "cutoff".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1000.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "q".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.707),
                    min: Some(0.1),
                    max: Some(20.0),
                    step: Some(0.1),
                    unit: Some("Q".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "gain_db".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(-24.0),
                    max: Some(24.0),
                    step: Some(0.5),
                    unit: Some("dB".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

/// Factory for creating biquad filters
pub struct BiquadFactory;

impl FilterFactory<BiquadFilter> for BiquadFactory {
    fn create_filter(&self, filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> BiquadFilter {
        BiquadFilter::new(filter_type, cutoff, q, gain_db)
    }
    
    fn factory_name(&self) -> &str {
        "Biquad"
    }
}