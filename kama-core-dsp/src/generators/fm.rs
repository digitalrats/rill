
use crate::math::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use crate::generators::{Generator, ModulatableGenerator};
use super::basic::{BasicOscillator, Waveform};
use super::envelope::EnvelopeGenerator;

/// Простой FM синтезатор на основе двух операторов
pub struct SimpleFmSynth<T: AudioNum> {
    /// Несущий осциллятор (carrier)
    carrier: BasicOscillator<T>,
    /// Модулирующий осциллятор (modulator)
    modulator: BasicOscillator<T>,
    /// Индекс модуляции
    modulation_index: T,
    /// Соотношение частот модулятора к несущей
    ratio: f32,
}

impl<T: AudioNum> SimpleFmSynth<T> {
    /// Создать новый FM синтезатор
    pub fn new(carrier_freq: f32, modulator_ratio: f32, modulation_index: T) -> Self {
        Self {
            carrier: BasicOscillator::new(Waveform::Sine, carrier_freq, T::from_f32(1.0)),
            modulator: BasicOscillator::new(Waveform::Sine, carrier_freq * modulator_ratio, T::from_f32(1.0)),
            modulation_index,
            ratio: modulator_ratio,
        }
    }
    
    /// Установить форму волны для несущей
    pub fn with_carrier_waveform(mut self, waveform: Waveform) -> Self {
        let freq = self.carrier.frequency();
        self.carrier = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        self
    }
    
    /// Установить форму волны для модулятора
    pub fn with_modulator_waveform(mut self, waveform: Waveform) -> Self {
        let freq = self.modulator.frequency();
        self.modulator = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        self
    }
    
    /// Установить частоту несущей
    pub fn set_carrier_frequency(&mut self, freq: f32) {
        self.carrier.set_frequency(freq);
        self.modulator.set_frequency(freq * self.ratio);
    }
    
    /// Установить индекс модуляции
    pub fn set_modulation_index(&mut self, index: T) {
        self.modulation_index = index;
    }
    
    /// Установить соотношение частот
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio;
        self.modulator.set_frequency(self.carrier.frequency() * ratio);
    }
}

impl<T: AudioNum> Algorithm<T> for SimpleFmSynth<T> {
    fn init(&mut self, sample_rate: f32) {
        self.carrier.init(sample_rate);
        self.modulator.init(sample_rate);
    }
    
    fn reset(&mut self) {
        self.carrier.reset();
        self.modulator.reset();
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        // Получаем модулирующий сигнал
        let mod_signal = self.modulator.process_sample(T::ZERO);
        
        // Модулируем частоту несущей
        self.carrier.modulate_frequency(mod_signal.mul(self.modulation_index));
        
        // Возвращаем сигнал несущей
        self.carrier.process_sample(T::ZERO)
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Simple FM Synth",
            category: AlgorithmCategory::Generator,
            description: "Simple FM synthesizer with one carrier and one modulator".to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// Многооператорный FM синтезатор (как в Yamaha DX7)
pub struct FmSynth<T: AudioNum, const N: usize> {
    /// Операторы (все используют BasicOscillator)
    operators: [BasicOscillator<T>; N],
    /// Алгоритм соединения (матрица маршрутов)
    algorithm: [[bool; N]; N],
    /// Индексы модуляции для каждого оператора
    modulation_indices: [T; N],
}

impl<T: AudioNum, const N: usize> FmSynth<T, N> {
    /// Создать новый FM синтезатор
    pub fn new(frequencies: [f32; N], algorithm: [[bool; N]; N]) -> Self {
        // Благодаря Copy, мы можем использовать этот простой синтаксис
        let mut operators = [BasicOscillator::new(Waveform::Sine, 440.0, T::from_f32(1.0)); N];
        for i in 0..N {
            operators[i].set_frequency(frequencies[i]);
        }
        
        Self {
            operators,
            algorithm,
            modulation_indices: [T::from_f32(1.0); N],
        }
    }
    
    /// Создать новый FM синтезатор со всеми операторами на одной частоте
    pub fn new_with_freq(frequency: f32, algorithm: [[bool; N]; N]) -> Self {
        let operators = [BasicOscillator::new(Waveform::Sine, frequency, T::from_f32(1.0)); N];
        
        Self {
            operators,
            algorithm,
            modulation_indices: [T::from_f32(1.0); N],
        }
    }
    
    /// Установить форму волны для оператора
    pub fn set_waveform(&mut self, index: usize, waveform: Waveform) {
        if index < N {
            let freq = self.operators[index].frequency();
            self.operators[index] = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        }
    }
    
    /// Установить частоту для оператора
    pub fn set_frequency(&mut self, index: usize, freq: f32) {
        if index < N {
            self.operators[index].set_frequency(freq);
        }
    }
    
    /// Установить индекс модуляции для оператора
    pub fn set_modulation_index(&mut self, index: usize, idx: T) {
        if index < N {
            self.modulation_indices[index] = idx;
        }
    }
}

impl<T: AudioNum, const N: usize> Algorithm<T> for FmSynth<T, N> {
    fn init(&mut self, sample_rate: f32) {
        for op in &mut self.operators {
            op.init(sample_rate);
        }
    }
    
    fn reset(&mut self) {
        for op in &mut self.operators {
            op.reset();
        }
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        // Сохраняем текущие значения операторов
        let mut values = [T::ZERO; N];
        for i in 0..N {
            values[i] = self.operators[i].process_sample(T::ZERO);
        }
        
        // Применяем модуляцию согласно алгоритму
        for i in 0..N {
            let mut mod_sum = T::ZERO;
            for j in 0..N {
                if self.algorithm[i][j] {
                    mod_sum = mod_sum.add(values[j].mul(self.modulation_indices[j]));
                }
            }
            if mod_sum != T::ZERO {
                self.operators[i].modulate_frequency(mod_sum);
            }
        }
        
        // Последний оператор (обычно) даёт выходной сигнал
        values[N-1]
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: format!("{}-operator FM Synth", N).to_string().leak(),
            category: AlgorithmCategory::Generator,
            description: format!("{}-operator FM synthesizer", N).to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}