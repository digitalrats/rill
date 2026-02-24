//! Низкочастотные генераторы для модуляции
//!
//! LFO (Low Frequency Oscillator) используются для модуляции параметров
//! звука: вибрато (частота), тремоло (амплитуда), фильтр-свип (частота среза)
//! и другие эффекты.

use crate::math::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use crate::generators::{Generator, SyncableGenerator};
use super::basic::{BasicOscillator, Waveform};

/// LFO генератор (Low Frequency Oscillator)
///
/// Генерирует низкочастотные сигналы для модуляции параметров.
/// Частотный диапазон: 0.01 Hz - 100 Hz.
///
/// # Режимы работы
///
/// - **Биполярный**: выходной сигнал в диапазоне [-1, 1]
/// - **Униполярный**: выходной сигнал в диапазоне [0, 1]
///
/// # Пример
/// ```
/// use kama_core_dsp::generators::*;
/// use kama_core_dsp::Algorithm;
///
/// // Создаём LFO для модуляции частоты фильтра
/// let mut lfo = LFO::<f32>::new(
///     5.0,              // 5 Hz
///     Waveform::Sine,
///     true              // биполярный режим (-1..1)
/// );
/// lfo.init(44100.0);
///
/// // Генерируем модуляционный сигнал
/// let modulation = lfo.process_sample(0.0);
/// ```
#[derive(Clone, Copy)]
pub struct LFO<T: AudioNum> {
    /// Внутренний осциллятор
    osc: BasicOscillator<T>,
    /// Биполярный режим (-1..1) или униполярный (0..1)
    bipolar: bool,
    /// Смещение фазы (для синхронизации)
    phase_offset: T,
}

impl<T: AudioNum> LFO<T> {
    /// Создать новый LFO
    ///
    /// # Arguments
    /// * `frequency` - частота в Hz (0.01 - 100)
    /// * `waveform` - форма волны
    /// * `bipolar` - true для биполярного режима (-1..1), false для униполярного (0..1)
    pub fn new(frequency: f32, waveform: Waveform, bipolar: bool) -> Self {
        let one = T::from_f32(1.0);
        Self {
            osc: BasicOscillator::new(waveform, frequency, one),
            bipolar,
            phase_offset: T::ZERO,
        }
    }
    
    /// Создать LFO с фазовым смещением
    pub fn with_phase_offset(mut self, offset: T) -> Self {
        self.set_phase_offset(offset);
        self
    }
    
    /// Установить биполярный режим
    ///
    /// # Arguments
    /// * `bipolar` - true: выход в [-1, 1], false: выход в [0, 1]
    pub fn set_bipolar(&mut self, bipolar: bool) {
        self.bipolar = bipolar;
    }
    
    /// Установить смещение фазы (0..1)
    ///
    /// Позволяет сдвинуть фазу LFO относительно опорной точки.
    /// Полезно для создания стерео эффектов или синхронизации нескольких LFO.
    pub fn set_phase_offset(&mut self, offset: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.phase_offset = if offset > one { one } else if offset < zero { zero } else { offset };
    }
    
    /// Получить текущее смещение фазы
    pub fn phase_offset(&self) -> T {
        self.phase_offset
    }
    
    /// Проверить, работает ли LFO в биполярном режиме
    pub fn is_bipolar(&self) -> bool {
        self.bipolar
    }
    
    /// Синхронизировать с внешним clock
    ///
    /// # Arguments
    /// * `reset` - если true, сбросить фазу в значение phase_offset
    pub fn sync(&mut self, reset: bool) {
        if reset {
            self.osc.set_phase(self.phase_offset);
        }
    }
    
    /// Получить значение для модуляции (текущий семпл)
    pub fn modulate(&mut self) -> T {
        let raw = self.osc.process_sample(T::ZERO);
        
        if self.bipolar {
            raw // уже -1..1
        } else {
            // Конвертируем из -1..1 в 0..1
            raw.mul(T::from_f32(0.5)).add(T::from_f32(0.5))
        }
    }
    
    /// Сбросить LFO в начальное состояние
    pub fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset);
    }
}

// ==================== Реализация трейта Algorithm ====================

impl<T: AudioNum> Algorithm<T> for LFO<T> {
    fn init(&mut self, sample_rate: f32) {
        self.osc.init(sample_rate);
        self.osc.set_phase(self.phase_offset);
    }
    
    fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset);
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        self.modulate()
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        // Получаем имя волны из поля waveform самого LFO
        // Но у нас нет прямого доступа к waveform, поэтому используем описание из BasicOscillator
        AlgorithmMetadata {
            name: "LFO",
            category: AlgorithmCategory::Generator,
            description: format!(
                "{} wave LFO ({}polar)",
                match self.osc.frequency() {
                    _ if self.osc.frequency() < 1.0 => "Very low frequency",
                    _ if self.osc.frequency() < 10.0 => "Low frequency",
                    _ => "Audio rate",
                },
                if self.bipolar { "bi" } else { "uni" }
            ).leak(),
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

// ==================== Реализация трейта Generator ====================

impl<T: AudioNum> Generator<T> for LFO<T> {
    fn phase(&self) -> T {
        self.osc.phase()
    }
    
    fn set_phase(&mut self, phase: T) {
        self.osc.set_phase(phase);
    }
    
    fn frequency(&self) -> f32 {
        self.osc.frequency()
    }
    
    fn set_frequency(&mut self, freq: f32) {
        self.osc.set_frequency(freq);
    }
    
    fn amplitude(&self) -> T {
        self.osc.amplitude()
    }
    
    fn set_amplitude(&mut self, amp: T) {
        self.osc.set_amplitude(amp);
    }
}

// ==================== Реализация трейта SyncableGenerator ====================

impl<T: AudioNum> SyncableGenerator<T> for LFO<T> {
    fn sync(&mut self, reset: bool) {
        if reset {
            self.osc.set_phase(self.phase_offset);
        }
    }
    
    fn periods(&self) -> u32 {
        self.osc.periods()
    }
}

// ==================== Тесты ====================

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;
    
    #[test]
    fn test_lfo_creation() {
        let lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        assert_eq!(lfo.frequency(), 5.0);
        assert!(lfo.is_bipolar());
        assert_eq!(lfo.phase_offset(), 0.0);
    }
    
    #[test]
    fn test_lfo_bipolar_mode() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);
        
        // В биполярном режиме значения должны быть в [-1, 1]
        for _ in 0..100 {
            let val = lfo.process_sample(0.0);
            assert!(val >= -1.0 && val <= 1.0, "Value {} out of range [-1,1]", val);
        }
    }
    
    #[test]
    fn test_lfo_unipolar_mode() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, false);
        lfo.init(44100.0);
        
        // В униполярном режиме значения должны быть в [0, 1]
        for _ in 0..100 {
            let val = lfo.process_sample(0.0);
            assert!(val >= 0.0 && val <= 1.0, "Value {} out of range [0,1]", val);
        }
    }
    
    #[test]
    fn test_lfo_phase_offset() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.set_phase_offset(0.25);
        lfo.init(44100.0);
        
        // Проверяем, что фаза установлена правильно
        assert!(approx_eq!(f32, lfo.phase(), 0.25, epsilon = 0.01));
    }
    
    #[test]
    fn test_lfo_sync() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.set_phase_offset(0.5);
        lfo.init(44100.0);
        
        // Продвигаем фазу
        for _ in 0..10 {
            lfo.process_sample(0.0);
        }
        
        // Синхронизируем со сбросом
        lfo.sync(true);
        assert!(approx_eq!(f32, lfo.phase(), 0.5, epsilon = 0.01));
    }
    
    #[test]
    fn test_lfo_waveforms() {
        let waveforms = [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ];
        
        for &wav in &waveforms {
            let mut lfo = LFO::<f32>::new(5.0, wav, true);
            lfo.init(44100.0);
            
            let val = lfo.process_sample(0.0);
            assert!(val >= -1.0 && val <= 1.0, "Waveform {:?} produced {}", wav, val);
        }
    }
    
    #[test]
    fn test_lfo_generator_trait() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);
        
        // Тестируем методы из трейта Generator
        assert_eq!(lfo.frequency(), 5.0);
        
        lfo.set_frequency(10.0);
        assert_eq!(lfo.frequency(), 10.0);
        
        lfo.set_amplitude(0.5);
        assert_eq!(lfo.amplitude(), 0.5);
        
        let phase = lfo.phase();
        assert!(phase >= 0.0 && phase <= 1.0);
    }
    
    #[test]
    fn test_lfo_syncable_trait() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);
        
        let initial_periods = lfo.periods();
        println!("Initial periods: {}", initial_periods);
        
        // Вычисляем количество семплов за период
        let samples_per_period = (44100.0 / 5.0) as usize;  // 8820 семплов
        println!("Samples per period: {}", samples_per_period);
        
        // Записываем начальную фазу
        let initial_phase = lfo.phase();
        println!("Initial phase: {}", initial_phase.as_f32());
        
        // Продвигаем фазу на несколько периодов
        for i in 0..samples_per_period * 3 {  // 3 полных периода
            let before_phase = lfo.phase();
            lfo.process_sample(0.0);
            let after_phase = lfo.phase();
            
            // Проверяем, не произошёл ли сброс фазы
            if after_phase < before_phase {
                println!("Phase reset at sample {}: {} -> {}", i, before_phase.as_f32(), after_phase.as_f32());
                println!("Periods count: {}", lfo.periods());
            }
            
            // Для отладки выведем информацию на ключевых точках
            if i == samples_per_period - 1 {
                println!("After 1 period (sample {}): phase={}, periods={}", 
                         i, lfo.phase().as_f32(), lfo.periods());
            } else if i == samples_per_period * 2 - 1 {
                println!("After 2 periods (sample {}): phase={}, periods={}", 
                         i, lfo.phase().as_f32(), lfo.periods());
            }
        }
        
        println!("Final phase: {}", lfo.phase().as_f32());
        println!("Final periods: {}", lfo.periods());
        
        assert!(lfo.periods() > initial_periods, 
                "Periods should increase: before={}, after={}", 
                initial_periods, lfo.periods());
        
        // Проверяем, что фаза продолжает меняться
        let mid_phase = lfo.phase();
        assert!(mid_phase != initial_phase, "Phase should change");
        
        // Синхронизируем со сбросом
        lfo.sync(true);
        assert!(approx_eq!(f32, lfo.phase(), 0.0, epsilon = 0.01));
    }
    
    #[test]
    fn test_lfo_clone_copy() {
        let lfo1 = LFO::<f32>::new(5.0, Waveform::Sine, true);
        let lfo2 = lfo1; // Копирование
        let lfo3 = lfo1.clone(); // Явное клонирование
        
        assert_eq!(lfo1.frequency(), lfo2.frequency());
        assert_eq!(lfo1.frequency(), lfo3.frequency());
        assert_eq!(lfo1.is_bipolar(), lfo2.is_bipolar());
    }
}