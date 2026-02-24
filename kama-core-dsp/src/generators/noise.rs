//! Генераторы шума (White, Pink, Brown, Blue, Violet)

use crate::math::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use crate::filters::{OnePole, FilterParams, FilterType};
use super::Generator;

/// Тип шума
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoiseType {
    White,  // Равномерный спектр
    Pink,   // 3dB/октава (1/f)
    Brown,  // 6dB/октава (1/f²)
    Blue,   // +3dB/октава
    Violet, // +6dB/октава
}

impl NoiseType {
    pub fn name(&self) -> &'static str {
        match self {
            NoiseType::White => "White Noise",
            NoiseType::Pink => "Pink Noise",
            NoiseType::Brown => "Brown Noise",
            NoiseType::Blue => "Blue Noise",
            NoiseType::Violet => "Violet Noise",
        }
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            NoiseType::White => "Equal energy per Hz",
            NoiseType::Pink => "Equal energy per octave (1/f)",
            NoiseType::Brown => "Brownian motion (1/f²)",
            NoiseType::Blue => "Increasing with frequency (+3dB/oct)",
            NoiseType::Violet => "Strongly increasing (+6dB/oct)",
        }
    }
}

/// Генератор шума (Xorshift RNG)
pub struct NoiseGenerator<T: AudioNum> {
    /// Тип шума
    noise_type: NoiseType,
    /// Амплитуда
    amplitude: T,
    /// Состояние RNG (Xorshift) - храним как u32 для битовых операций
    state: u32,
    /// Фильтры для окраски
    pink_filters: [OnePole<T>; 6],
    brown_state: T,
    /// Частота дискретизации
    sample_rate: f32,
    /// Для синего шума
    last_white: T,
    /// Для фиолетового шума
    last_white1: T,
    last_white2: T,
}

impl<T: AudioNum> NoiseGenerator<T> {
    /// Создать новый генератор шума
    pub fn new(noise_type: NoiseType, amplitude: T) -> Self {
        // Создаем OnePole фильтры через new с правильными параметрами
        let filter_params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1.0,
            q: 0.707,
            gain_db: 0.0,
        };
        
        Self {
            noise_type,
            amplitude,
            state: 123456789,
            pink_filters: [
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params),
            ],
            brown_state: T::ZERO,
            sample_rate: 44100.0,
            last_white: T::ZERO,
            last_white1: T::ZERO,
            last_white2: T::ZERO,
        }
    }
    
    /// Xorshift RNG (работает с u32, возвращает f32 через AudioNum)
    #[inline(always)]
    fn xorshift(&mut self) -> T {
        let mut x = self.state;
        
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        
        self.state = x;
        
        // Конвертируем u32 в f32 в диапазоне [-1, 1]
        // Берем старшие 24 бита для равномерного распределения
        let float_val = (x as f32 / 2147483648.0) - 1.0; // 2^31
        T::from_f32(float_val)
    }
    
    /// Генерация белого шума
    #[inline(always)]
    fn generate_white(&mut self) -> T {
        self.xorshift().mul(self.amplitude)
    }
    
    /// Генерация розового шума (1/f)
    /// Метод Paul Kellett'a
    fn generate_pink(&mut self) -> T {
        let white = self.xorshift();
        
        // 6-полосный фильтр для аппроксимации 1/f
        let mut output = T::ZERO;
        for filter in &mut self.pink_filters {
            output = output.add(filter.process_sample(white));
        }
        
        output.mul(self.amplitude).div(T::from_f32(3.0)) // нормализация
    }
    
    /// Генерация броуновского шума (1/f²)
    fn generate_brown(&mut self) -> T {
        let white = self.xorshift();
        
        // Интегратор с ограничением
        self.brown_state = self.brown_state.add(white.mul(T::from_f32(0.1)));
        // Клиппинг
        let one = T::from_f32(1.0);
        let neg_one = T::from_f32(-1.0);
        if self.brown_state > one {
            self.brown_state = one;
        } else if self.brown_state < neg_one {
            self.brown_state = neg_one;
        }
        
        self.brown_state.mul(self.amplitude)
    }
    
    /// Генерация синего шума (+3dB/октава)
    fn generate_blue(&mut self) -> T {
        let white = self.xorshift();
        
        // Дифференциатор (high-pass)
        let diff = white.sub(self.last_white);
        self.last_white = white;
        
        diff.mul(self.amplitude)
    }
    
    /// Генерация фиолетового шума (+6dB/октава)
    fn generate_violet(&mut self) -> T {
        let white = self.xorshift();
        
        // Двойной дифференциатор
        let diff1 = white.sub(self.last_white1);
        let diff2 = diff1.sub(self.last_white2);
        self.last_white2 = diff1;
        self.last_white1 = white;
        
        diff2.mul(self.amplitude)
    }
}

impl<T: AudioNum> Algorithm<T> for NoiseGenerator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        
        // Настройка фильтров для розового шума
        let freqs = [5.0, 15.0, 45.0, 135.0, 405.0, 1215.0];
        for (i, &freq) in freqs.iter().enumerate() {
            // Обновляем параметры фильтра через set_cutoff
            // Для этого нужно импортировать трейт Filter
            use crate::filters::Filter;
            self.pink_filters[i].set_cutoff(freq);
        }
        
        self.reset();
    }
    
    fn reset(&mut self) {
        self.state = 123456789;
        self.brown_state = T::ZERO;
        self.last_white = T::ZERO;
        self.last_white1 = T::ZERO;
        self.last_white2 = T::ZERO;
        for filter in &mut self.pink_filters {
            filter.reset();
        }
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        match self.noise_type {
            NoiseType::White => self.generate_white(),
            NoiseType::Pink => self.generate_pink(),
            NoiseType::Brown => self.generate_brown(),
            NoiseType::Blue => self.generate_blue(),
            NoiseType::Violet => self.generate_violet(),
        }
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: self.noise_type.name(),
            category: AlgorithmCategory::Generator,
            description: self.noise_type.description(),
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

impl<T: AudioNum> Generator<T> for NoiseGenerator<T> {
    fn phase(&self) -> T { T::ZERO } // Шум не имеет фазы
    
    fn set_phase(&mut self, _phase: T) {}
    
    fn frequency(&self) -> f32 { 0.0 }
    
    fn set_frequency(&mut self, _freq: f32) {}
    
    fn amplitude(&self) -> T { self.amplitude }
    
    fn set_amplitude(&mut self, amp: T) {
        let one = T::from_f32(1.0);
        self.amplitude = if amp > one { one } else if amp < T::ZERO { T::ZERO } else { amp };
    }
}