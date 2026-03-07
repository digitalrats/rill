//! # Фильтры Чебышева (Chebyshev Filters)

use kama_core::AudioNum;
use super::{FilterParams, FilterType};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use std::f64::consts::PI as PI64;
use num_complex::Complex64;

// -----------------------------------------------------------------------------
// Вспомогательные функции
// -----------------------------------------------------------------------------

/// Полином Чебышева первого рода T_n(x)
fn chebyshev_poly_t(n: usize, x: f64) -> f64 {
    match n {
        0 => 1.0,
        1 => x,
        _ => {
            let mut t_prev2 = 1.0;
            let mut t_prev1 = x;
            let mut t_curr = 0.0;
            
            for _ in 2..=n {
                t_curr = 2.0 * x * t_prev1 - t_prev2;
                t_prev2 = t_prev1;
                t_prev1 = t_curr;
            }
            
            t_curr
        }
    }
}

/// Вычислить полюса фильтра Чебышева типа I
fn chebyshev_type_i_poles(n: usize, ripple_db: f64) -> Vec<Complex64> {
    let eps = (10.0_f64.powf(ripple_db / 10.0) - 1.0).sqrt();
    let a = (1.0 / eps + (1.0 + 1.0 / (eps * eps)).sqrt()).asinh() / n as f64;
    
    let mut poles = Vec::with_capacity(n);
    
    for k in 0..n {
        let theta = PI64 * (2.0 * k as f64 + 1.0) / (2.0 * n as f64);
        let sigma = -a.sinh() * theta.sin();
        let omega = a.cosh() * theta.cos();
        
        poles.push(Complex64::new(sigma, omega));
    }
    
    poles
}

/// Вычислить полюса и нули фильтра Чебышева типа II
fn chebyshev_type_ii_poles_zeros(n: usize, ripple_db: f64) -> (Vec<Complex64>, Vec<Complex64>) {
    let eps = 1.0 / (10.0_f64.powf(ripple_db / 10.0) - 1.0).sqrt();
    let a = (1.0 / eps + (1.0 + 1.0 / (eps * eps)).sqrt()).asinh() / n as f64;
    
    let mut poles = Vec::with_capacity(n);
    let mut zeros = Vec::with_capacity(n);
    
    for k in 0..n {
        let theta = PI64 * (2.0 * k as f64 + 1.0) / (2.0 * n as f64);
        
        // Полюса
        let sigma = -a.sinh() * theta.sin();
        let omega = a.cosh() * theta.cos();
        poles.push(Complex64::new(sigma, omega));
        
        // Нули (на мнимой оси)
        if k % 2 == 1 {
            let omega_zero = 1.0 / theta.cos();
            zeros.push(Complex64::new(0.0, omega_zero));
        }
    }
    
    (poles, zeros)
}

// -----------------------------------------------------------------------------
// Биквадратная секция
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct BiquadSection<T: AudioNum> {
    coeffs: (T, T, T, T, T),
    state: (T, T, T, T),
}

impl<T: AudioNum> BiquadSection<T> {
    fn new() -> Self {
        Self {
            coeffs: (T::from_f32(1.0), T::ZERO, T::ZERO, T::ZERO, T::ZERO),
            state: (T::ZERO, T::ZERO, T::ZERO, T::ZERO),
        }
    }
    
    #[inline(always)]
    fn process(&mut self, input: T) -> T {
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
    
    fn set_coeffs(&mut self, b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) {
        self.coeffs = (
            T::from_f32(b0 as f32),
            T::from_f32(b1 as f32),
            T::from_f32(b2 as f32),
            T::from_f32(a1 as f32),
            T::from_f32(a2 as f32),
        );
    }
    
    fn reset(&mut self) {
        self.state = (T::ZERO, T::ZERO, T::ZERO, T::ZERO);
    }
}

// -----------------------------------------------------------------------------
// Параметры фильтра Чебышева
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChebyshevParams {
    pub filter_params: FilterParams,
    pub order: usize,
    pub ripple_db: f32,
}

// -----------------------------------------------------------------------------
// Фильтр Чебышева типа I
// -----------------------------------------------------------------------------

pub struct ChebyshevI<T: AudioNum, const MAX_SECTIONS: usize> {
    params: ChebyshevParams,
    sections: [BiquadSection<T>; MAX_SECTIONS],
    num_sections: usize,
    gain: T,
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_SECTIONS: usize> ChebyshevI<T, MAX_SECTIONS> {
    pub fn new(params: FilterParams, order: usize, ripple_db: f32) -> Self {
        let mut filter = Self {
            params: ChebyshevParams {
                filter_params: params,
                order,
                ripple_db,
            },
            sections: [(); MAX_SECTIONS].map(|_| BiquadSection::new()),
            num_sections: 0,
            gain: T::from_f32(1.0),
            sample_rate: 44100.0,
        };
        filter.design();
        filter
    }
    
    pub fn design(&mut self) {
        let n = self.params.order;
        let ripple = self.params.ripple_db as f64;
        let cutoff = self.params.filter_params.cutoff as f64;
        let sample_rate_f64 = self.sample_rate as f64;
        
        let analog_poles = chebyshev_type_i_poles(n, ripple);
        
        let warp_cutoff = 2.0 * (PI64 * cutoff / sample_rate_f64).tan();
        
        self.num_sections = (n + 1) / 2;
        self.gain = T::from_f32(1.0); // Упрощённо, в реальности нужно вычислять gain
        
        for i in 0..self.num_sections {
            let idx1 = i * 2;
            let idx2 = i * 2 + 1;
            
            if idx2 < n {
                let p1 = analog_poles[idx1];
                let p2 = analog_poles[idx2];
                
                let sp1 = p1 * warp_cutoff;
                let sp2 = p2 * warp_cutoff;
                
                let zp1 = (Complex64::new(2.0, 0.0) + sp1) / (Complex64::new(2.0, 0.0) - sp1);
                let zp2 = (Complex64::new(2.0, 0.0) + sp2) / (Complex64::new(2.0, 0.0) - sp2);
                
                let a1 = -(zp1 + zp2).re;
                let a2 = (zp1 * zp2).re;
                
                self.sections[i].set_coeffs(1.0, 2.0, 1.0, a1, a2);
            } else {
                let p = analog_poles[idx1];
                
                let sp = p * warp_cutoff;
                let zp = (Complex64::new(2.0, 0.0) + sp) / (Complex64::new(2.0, 0.0) - sp);
                
                let a1 = -zp.re;
                let a2 = 0.0;
                
                self.sections[i].set_coeffs(1.0, 2.0, 1.0, a1, a2);
            }
        }
    }
}

impl<T: AudioNum, const MAX_SECTIONS: usize> Algorithm<T> for ChebyshevI<T, MAX_SECTIONS> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.design();
        self.reset();
    }
    
    fn reset(&mut self) {
        for section in &mut self.sections[..self.num_sections] {
            section.reset();
        }
    }
    
    fn process_sample(&mut self, input: T) -> T {
        let mut x = input.mul(self.gain);
        for section in &mut self.sections[..self.num_sections] {
            x = section.process(x);
        }
        x
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Chebyshev Type I Filter",
            category: AlgorithmCategory::Filter,
            description: format!("Chebyshev Type I filter (order {}, ripple {} dB)", 
                                  self.params.order, self.params.ripple_db).leak(),
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

impl<T: AudioNum, const MAX_SECTIONS: usize> ParameterizedAlgorithm<T> for ChebyshevI<T, MAX_SECTIONS> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params.filter_params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params.filter_params = params;
        self.design();
    }
}

// -----------------------------------------------------------------------------
// Фильтр Чебышева типа II
// -----------------------------------------------------------------------------

pub struct ChebyshevII<T: AudioNum, const MAX_SECTIONS: usize> {
    params: ChebyshevParams,
    sections: [BiquadSection<T>; MAX_SECTIONS],
    num_sections: usize,
    gain: T,
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_SECTIONS: usize> ChebyshevII<T, MAX_SECTIONS> {
    pub fn new(params: FilterParams, order: usize, ripple_db: f32) -> Self {
        let mut filter = Self {
            params: ChebyshevParams {
                filter_params: params,
                order,
                ripple_db,
            },
            sections: [(); MAX_SECTIONS].map(|_| BiquadSection::new()),
            num_sections: 0,
            gain: T::from_f32(1.0),
            sample_rate: 44100.0,
        };
        filter.design();
        filter
    }
    
    pub fn design(&mut self) {
        let n = self.params.order;
        let ripple = self.params.ripple_db as f64;
        let cutoff = self.params.filter_params.cutoff as f64;
        let sample_rate_f64 = self.sample_rate as f64;
        
        let (analog_poles, analog_zeros) = chebyshev_type_ii_poles_zeros(n, ripple);
        
        let warp_cutoff = 2.0 * (PI64 * cutoff / sample_rate_f64).tan();
        
        self.num_sections = (n + 1) / 2;
        self.gain = T::from_f32(1.0);
        
        for i in 0..self.num_sections {
            let idx1 = i * 2;
            let idx2 = i * 2 + 1;
            
            if idx2 < n {
                let p1 = analog_poles[idx1];
                let p2 = analog_poles[idx2];
                
                let z1 = if idx1 < analog_zeros.len() { analog_zeros[idx1] } else { Complex64::new(-1.0, 0.0) };
                let z2 = if idx2 < analog_zeros.len() { analog_zeros[idx2] } else { z1.conj() };
                
                let sp1 = p1 * warp_cutoff;
                let sp2 = p2 * warp_cutoff;
                
                let zp1 = (Complex64::new(2.0, 0.0) + sp1) / (Complex64::new(2.0, 0.0) - sp1);
                let zp2 = (Complex64::new(2.0, 0.0) + sp2) / (Complex64::new(2.0, 0.0) - sp2);
                
                let sz1 = z1 * warp_cutoff;
                let sz2 = z2 * warp_cutoff;
                
                let zz1 = (Complex64::new(2.0, 0.0) + sz1) / (Complex64::new(2.0, 0.0) - sz1);
                let zz2 = (Complex64::new(2.0, 0.0) + sz2) / (Complex64::new(2.0, 0.0) - sz2);
                
                let b0 = 1.0;
                let b1 = -(zz1 + zz2).re;
                let b2 = (zz1 * zz2).re;
                
                let a1 = -(zp1 + zp2).re;
                let a2 = (zp1 * zp2).re;
                
                self.sections[i].set_coeffs(b0, b1, b2, a1, a2);
            }
        }
    }
}

impl<T: AudioNum, const MAX_SECTIONS: usize> Algorithm<T> for ChebyshevII<T, MAX_SECTIONS> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.design();
        self.reset();
    }
    
    fn reset(&mut self) {
        for section in &mut self.sections[..self.num_sections] {
            section.reset();
        }
    }
    
    fn process_sample(&mut self, input: T) -> T {
        let mut x = input.mul(self.gain);
        for section in &mut self.sections[..self.num_sections] {
            x = section.process(x);
        }
        x
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Chebyshev Type II Filter",
            category: AlgorithmCategory::Filter,
            description: format!("Chebyshev Type II filter (order {}, ripple {} dB)", 
                                  self.params.order, self.params.ripple_db).leak(),
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

impl<T: AudioNum, const MAX_SECTIONS: usize> ParameterizedAlgorithm<T> for ChebyshevII<T, MAX_SECTIONS> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params.filter_params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params.filter_params = params;
        self.design();
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chebyshev_polynomials() {
        assert!((chebyshev_poly_t(0, 0.5) - 1.0).abs() < 1e-10);
        assert!((chebyshev_poly_t(1, 0.5) - 0.5).abs() < 1e-10);
        assert!((chebyshev_poly_t(2, 0.5) - (2.0*0.25 - 1.0)).abs() < 1e-10);
    }
    
    #[test]
    fn test_chebyshev_i_poles() {
        let poles = chebyshev_type_i_poles(4, 0.5);
        assert_eq!(poles.len(), 4);
        assert!((poles[0] - poles[3].conj()).norm() < 1e-10);
        assert!((poles[1] - poles[2].conj()).norm() < 1e-10);
    }
}