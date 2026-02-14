//! Сложные фильтры для микшера

/// Тип фильтра
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    Bitcrusher,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Custom,
}

/// Параметры фильтра
#[derive(Debug, Clone)]
pub struct FilterParams {
    pub bit_depth: Option<u8>,
    pub sample_rate_reduction: Option<f64>,
    pub cutoff: Option<f64>,
    pub resonance: Option<f64>,
    pub drive: Option<f64>,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            bit_depth: None,
            sample_rate_reduction: None,
            cutoff: None,
            resonance: None,
            drive: None,
        }
    }
}

/// Конфигурация фильтра
#[derive(Debug, Clone)]
pub struct FilterConfig {
    pub filter_type: FilterType,
    pub enabled: bool,
    pub params: FilterParams,
    pub position: usize,
}

/// Биткрашер
pub struct Bitcrusher {
    bit_depth: u8,
    last_sample: f64,
    counter: usize,
    reduction_factor: f64,
}

impl Bitcrusher {
    pub fn new(bit_depth: u8, reduction_factor: f64) -> Self {
        Self {
            bit_depth: bit_depth.clamp(1, 24),
            last_sample: 0.0,
            counter: 0,
            reduction_factor: reduction_factor.clamp(0.0, 1.0),
        }
    }
    
    pub fn process(&mut self, input: f64) -> f64 {
        self.counter += 1;
        
        if self.counter as f64 >= 1.0 / self.reduction_factor {
            self.counter = 0;
            
            // Квантование
            let steps = (1u32 << self.bit_depth) as f64;
            self.last_sample = (input * steps).round() / steps;
        }
        
        self.last_sample
    }
}

/// Композиция фильтров
pub struct FilterChain {
    filters: Vec<Box<dyn FnMut(f64) -> f64 + Send>>,
}

impl FilterChain {
    pub fn new() -> Self {
        Self { filters: Vec::new() }
    }
    
    pub fn add_filter<F>(&mut self, filter: F)
    where
        F: FnMut(f64) -> f64 + Send + 'static,
    {
        self.filters.push(Box::new(filter));
    }
    
    pub fn process(&mut self, input: f64) -> f64 {
        let mut result = input;
        for filter in &mut self.filters {
            result = filter(result);
        }
        result
    }
}