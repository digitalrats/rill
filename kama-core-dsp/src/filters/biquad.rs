//! Биквадратный фильтр (Biquad)

use crate::math::AudioNum;
use super::{FilterParams, FilterType};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use std::f32::consts::PI;

/// Биквадратный фильтр
pub struct Biquad<T: AudioNum> {
    params: FilterParams,
    coeffs: (T, T, T, T, T),
    state: (T, T, T, T),
    sample_rate: f32,
}

impl<T: AudioNum> Biquad<T> {
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            coeffs: (T::from_f32(1.0), T::ZERO, T::ZERO, T::ZERO, T::ZERO),
            state: (T::ZERO, T::ZERO, T::ZERO, T::ZERO),
            sample_rate: 44100.0,
        };
        filter.update_coeffs();
        filter
    }
    
    fn update_coeffs(&mut self) {
        let omega = 2.0 * PI * self.params.cutoff / self.sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * self.params.q);
        
        match self.params.filter_type {
            FilterType::LowPass => {
                let b0 = (1.0 - cos_omega) / 2.0;
                let b1 = 1.0 - cos_omega;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs = (
                    T::from_f32(b0 / a0),
                    T::from_f32(b1 / a0),
                    T::from_f32(b2 / a0),
                    T::from_f32(a1 / a0),
                    T::from_f32(a2 / a0),
                );
            }
            
            FilterType::HighPass => {
                let b0 = (1.0 + cos_omega) / 2.0;
                let b1 = -(1.0 + cos_omega);
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;
                
                self.coeffs = (
                    T::from_f32(b0 / a0),
                    T::from_f32(b1 / a0),
                    T::from_f32(b2 / a0),
                    T::from_f32(a1 / a0),
                    T::from_f32(a2 / a0),
                );
            }
            
            // ... остальные типы
            _ => {}
        }
    }
}

impl<T: AudioNum> Algorithm<T> for Biquad<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
        self.reset();
    }
    
    fn reset(&mut self) {
        self.state = (T::ZERO, T::ZERO, T::ZERO, T::ZERO);
    }
    
    fn process_sample(&mut self, input: T) -> T {
        let (b0, b1, b2, a1, a2) = self.coeffs;
        let (x1, x2, y1, y2) = self.state;
        
        let output = b0.mul(input)
            .add(b1.mul(x1))
            .add(b2.mul(x2))
            .sub(a1.mul(y1))
            .sub(a2.mul(y2));
        
        self.state = (input, x1, output, y1);
        
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Biquad Filter",
            category: AlgorithmCategory::Filter,
            description: "Universal biquad filter".to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
}

impl<T: AudioNum> ParameterizedAlgorithm<T> for Biquad<T> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_coeffs();
    }
}