//! Вейвтейбл генераторы

use crate::math::{AudioNum, lerp};
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use super::Generator;

/// Вейвтейбл осциллятор
pub struct WavetableOscillator<T: AudioNum, const SIZE: usize> {
    /// Вейвтейбл (таблица волны)
    table: [T; SIZE],
    /// Частота
    frequency: f32,
    /// Амплитуда
    amplitude: T,
    /// Текущая фаза (в индексах таблицы)
    phase: T,
    /// Инкремент фазы
    phase_inc: T,
    /// Интерполяция (true = кубическая, false = линейная)
    cubic_interp: bool,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const SIZE: usize> WavetableOscillator<T, SIZE> {
    /// Создать новый вейвтейбл осциллятор
    pub fn new(table: [T; SIZE], frequency: f32) -> Self {
        let mut osc = Self {
            table,
            frequency,
            amplitude: T::from_f32(1.0),
            phase: T::ZERO,
            phase_inc: T::ZERO,
            cubic_interp: false,
            sample_rate: 44100.0,
        };
        osc.update_phase_inc();
        osc
    }
    
    /// Создать из синусоиды
    pub fn sine(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for i in 0..SIZE {
            let phase = (i as f32 / SIZE as f32) * 2.0 * std::f32::consts::PI;
            table[i] = T::from_f32(phase.sin());
        }
        Self::new(table, frequency)
    }
    
    /// Создать из пилообразной волны
    pub fn saw(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for i in 0..SIZE {
            table[i] = T::from_f32(2.0 * i as f32 / SIZE as f32 - 1.0);
        }
        Self::new(table, frequency)
    }
    
    fn update_phase_inc(&mut self) {
        self.phase_inc = T::from_f32(self.frequency / self.sample_rate)
            .mul(T::from_f32(SIZE as f32));
    }
    
    /// Линейная интерполяция
    #[inline(always)]
    fn read_linear(&self, idx: T) -> T {
        let idx_f = idx.as_f32();
        let i0 = idx_f.floor() as usize % SIZE;
        let i1 = (i0 + 1) % SIZE;
        let frac = T::from_f32(idx_f.fract());
        
        lerp(self.table[i0], self.table[i1], frac)
    }
    
    /// Кубическая интерполяция (Hermite)
    #[inline(always)]
    fn read_cubic(&self, idx: T) -> T {
        let idx_f = idx.as_f32();
        let i = idx_f.floor() as usize;
        
        let i0 = (i + SIZE - 1) % SIZE;
        let i1 = i % SIZE;
        let i2 = (i + 1) % SIZE;
        let i3 = (i + 2) % SIZE;
        let frac = T::from_f32(idx_f.fract());
        
        // Hermite interpolation
        let c0 = self.table[i1];
        let c1 = self.table[i2].sub(self.table[i0]).mul(T::from_f32(0.5));
        let c2 = self.table[i0].sub(self.table[i1]).mul(T::from_f32(1.5))
            .add(self.table[i2].sub(self.table[i3]).mul(T::from_f32(0.5)));
        let c3 = self.table[i2].sub(self.table[i1]).mul(T::from_f32(0.5))
            .add(self.table[i3].sub(self.table[i0]).mul(T::from_f32(0.5)))
            .sub(self.table[i1].sub(self.table[i2]).mul(T::from_f32(1.5)));
        
        let f2 = frac.mul(frac);
        let f3 = f2.mul(frac);
        
        c0.add(c1.mul(frac))
            .add(c2.mul(f2))
            .add(c3.mul(f3))
    }
}

impl<T: AudioNum, const SIZE: usize> Algorithm<T> for WavetableOscillator<T, SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = T::ZERO;
    }
    
    fn reset(&mut self) {
        self.phase = T::ZERO;
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        let output = if self.cubic_interp {
            self.read_cubic(self.phase)
        } else {
            self.read_linear(self.phase)
        }.mul(self.amplitude);
        
        self.phase = self.phase.add(self.phase_inc);
        while self.phase.as_f32() >= SIZE as f32 {
            self.phase = self.phase.sub(T::from_f32(SIZE as f32));
        }
        
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Wavetable Oscillator",
            category: AlgorithmCategory::Generator,
            description: "Wavetable oscillator with interpolation".to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum, const SIZE: usize> Generator<T> for WavetableOscillator<T, SIZE> {
    fn phase(&self) -> T { 
        self.phase.div(T::from_f32(SIZE as f32))
    }
    
    fn set_phase(&mut self, phase: T) {
        self.phase = phase.mul(T::from_f32(SIZE as f32))
            .clamp(T::ZERO, T::from_f32(SIZE as f32));
    }
    
    fn frequency(&self) -> f32 { self.frequency }
    
    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq;
        self.update_phase_inc();
    }
    
    fn amplitude(&self) -> T { self.amplitude }
    
    fn set_amplitude(&mut self, amp: T) {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
    }
}