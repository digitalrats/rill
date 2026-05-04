//! FM (Frequency Modulation) синтез
//!
//! Этот модуль предоставляет инструменты для частотной модуляции:
//! - Простой 2-операторный FM синтезатор
//! - Многооператорный FM синтезатор (как в Yamaha DX7)
//! - Поддержка различных форм волны для каждого оператора
//! - Гибкая маршрутизация модуляции

use super::basic::{BasicOscillator, Waveform};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::generators::{Generator, ModulatableGenerator};
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

// =============================================================================
// Простой 2-операторный FM синтезатор
// =============================================================================

/// Простой FM синтезатор на основе двух операторов
///
/// Базовая FM архитектура: один модулятор модулирует один carrier.
/// Идеально подходит для:
/// - Создания металлических тембров
/// - Эмуляции колокольчиков
/// - Сложных гармонических структур
///
/// # Пример
/// ```
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
/// use rill_core_dsp::generators::*;
/// use rill_core_dsp::Algorithm;
///
/// let tick = ClockTick::default();
/// let ctx = ActionContext::new(&tick);
///
/// // Создаём FM синтезатор с соотношением частот 2:1
/// let mut fm = SimpleFmSynth::<f32>::new(
///     440.0,  // частота несущей (A4)
///     2.0,    // модулятор на октаву выше
///     1.5     // индекс модуляции
/// );
/// fm.init(44100.0);
///
/// // Генерируем семпл
/// let mut output = [0.0_f32];
/// fm.process(None, &mut output, &ctx).unwrap();
/// let sample = output[0];
/// ```
#[derive(Clone, Copy)]
pub struct SimpleFmSynth<T: Transcendental> {
    /// Несущий осциллятор (carrier) - производит выходной сигнал
    carrier: BasicOscillator<T>,
    /// Модулирующий осциллятор (modulator) - модулирует частоту carrier
    modulator: BasicOscillator<T>,
    /// Индекс модуляции (глубина модуляции)
    modulation_index: ScalarVector1<T>,
    /// Соотношение частот модулятора к несущей
    ratio: f32,
}

impl<T: Transcendental> SimpleFmSynth<T> {
    /// Создать новый FM синтезатор
    ///
    /// # Arguments
    /// * `carrier_freq` - частота несущей в Hz
    /// * `modulator_ratio` - соотношение частот (модулятор/carrier)
    /// * `modulation_index` - индекс модуляции (0.0 - 10.0)
    pub fn new(carrier_freq: f32, modulator_ratio: f32, modulation_index: T) -> Self {
        let one = T::from_f32(1.0);
        Self {
            carrier: BasicOscillator::new(Waveform::Sine, carrier_freq, one),
            modulator: BasicOscillator::new(Waveform::Sine, carrier_freq * modulator_ratio, one),
            modulation_index: ScalarVector1::splat(modulation_index),
            ratio: modulator_ratio,
        }
    }

    /// Установить форму волны для несущей
    ///
    /// По умолчанию используется синусоида
    pub fn with_carrier_waveform(mut self, waveform: Waveform) -> Self {
        let freq = self.carrier.frequency();
        self.carrier = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        self
    }

    /// Установить форму волны для модулятора
    ///
    /// По умолчанию используется синусоида
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
    ///
    /// # Arguments
    /// * `index` - индекс модуляции (0.0 - 10.0)
    pub fn set_modulation_index(&mut self, index: T) {
        self.modulation_index = ScalarVector1::splat(index);
        self.carrier.set_modulation_index(index);
    }

    /// Установить соотношение частот
    ///
    /// # Arguments
    /// * `ratio` - соотношение (модулятор/carrier), обычно 0.1 - 10.0
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio;
        self.modulator
            .set_frequency(self.carrier.frequency() * ratio);
    }

    /// Получить текущий индекс модуляции
    pub fn modulation_index(&self) -> T {
        self.modulation_index.extract(0)
    }

    /// Получить текущее соотношение частот
    pub fn ratio(&self) -> f32 {
        self.ratio
    }
}

impl<T: Transcendental> Algorithm<T> for SimpleFmSynth<T> {
    fn init(&mut self, sample_rate: f32) {
        self.carrier.init(sample_rate);
        self.modulator.init(sample_rate);
    }

    fn reset(&mut self) {
        self.carrier.reset();
        self.modulator.reset();
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            // Получаем модулирующий сигнал
            let mod_signal = self.modulator.generate().extract(0);

            // Модулируем частоту несущей
            self.carrier
                .modulate_frequency(mod_signal * self.modulation_index.extract(0));

            // Возвращаем сигнал несущей
            *out = self.carrier.generate().extract(0);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Simple FM Synth",
            category: AlgorithmCategory::Generator,
            description: "2-operator FM synthesizer",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ==================== Реализация трейта Generator для SimpleFmSynth ====================

impl<T: Transcendental> Generator<T> for SimpleFmSynth<T> {
    fn phase(&self) -> T {
        self.carrier.phase()
    }

    fn set_phase(&mut self, phase: T) {
        self.carrier.set_phase(phase);
        self.modulator.set_phase(phase);
    }

    fn frequency(&self) -> f32 {
        self.carrier.frequency()
    }

    fn set_frequency(&mut self, freq: f32) {
        self.set_carrier_frequency(freq);
    }

    fn amplitude(&self) -> T {
        self.carrier.amplitude()
    }

    fn set_amplitude(&mut self, amp: T) {
        self.carrier.set_amplitude(amp);
        self.modulator.set_amplitude(amp);
    }
}

// ==================== Реализация трейта ModulatableGenerator для SimpleFmSynth ====================

impl<T: Transcendental> ModulatableGenerator<T> for SimpleFmSynth<T> {
    fn modulate_frequency(&mut self, amount: T) {
        self.carrier.modulate_frequency(amount);
        // Также обновляем modulation_index, чтобы он соответствовал
        self.modulation_index = ScalarVector1::splat(amount);
    }

    fn modulation_index(&self) -> T {
        SimpleFmSynth::modulation_index(self)
    }

    fn set_modulation_index(&mut self, index: T) {
        SimpleFmSynth::set_modulation_index(self, index);
    }
}

// =============================================================================
// Многооператорный FM синтезатор (как в Yamaha DX7)
// =============================================================================

/// Многооператорный FM синтезатор
///
/// Реализует архитектуру, аналогичную Yamaha DX7:
/// - N операторов (обычно 4 или 6)
/// - Каждый оператор может быть carrier или modulator
/// - Гибкая матрица маршрутов модуляции
/// - Индивидуальные индексы модуляции
///
/// # Пример
/// ```
/// use rill_core_dsp::generators::*;
/// use rill_core_dsp::Algorithm;
///
/// // 6-операторный FM (как в DX7)
/// let frequencies = [440.0, 880.0, 1320.0, 1760.0, 2200.0, 2640.0];
/// let algorithm = [
///     [false, true,  false, false, false, false],
///     [false, false, true,  false, false, false],
///     [false, false, false, true,  false, false],
///     [false, false, false, false, true,  false],
///     [false, false, false, false, false, true],
///     [false, false, false, false, false, false],
/// ];
///
/// let mut fm = FmSynth::<f32, 6>::new(frequencies, algorithm);
/// fm.init(44100.0);
/// ```
pub struct FmSynth<T: Transcendental, const N: usize> {
    /// Операторы (все используют BasicOscillator)
    operators: [BasicOscillator<T>; N],
    /// Алгоритм соединения (матрица маршрутов)
    /// matrix[i][j] = true означает, что оператор j модулирует оператор i
    algorithm: [[bool; N]; N],
    /// Индексы модуляции для каждого оператора
    modulation_indices: [ScalarVector1<T>; N],
}

impl<T: Transcendental, const N: usize> FmSynth<T, N> {
    /// Создать новый FM синтезатор
    ///
    /// # Arguments
    /// * `frequencies` - массив частот для каждого оператора
    /// * `algorithm` - матрица маршрутов модуляции N x N
    pub fn new(frequencies: [f32; N], algorithm: [[bool; N]; N]) -> Self {
        let one = T::from_f32(1.0);
        let mut operators = [BasicOscillator::new(Waveform::Sine, 440.0, one); N];
        for i in 0..N {
            operators[i].set_frequency(frequencies[i]);
        }

        Self {
            operators,
            algorithm,
            modulation_indices: [ScalarVector1::splat(T::ZERO); N],
        }
    }

    /// Создать новый FM синтезатор со всеми операторами на одной частоте
    pub fn new_with_freq(frequency: f32, algorithm: [[bool; N]; N]) -> Self {
        let one = T::from_f32(1.0);
        let operators = [BasicOscillator::new(Waveform::Sine, frequency, one); N];

        Self {
            operators,
            algorithm,
            modulation_indices: [ScalarVector1::splat(T::ZERO); N],
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
            self.modulation_indices[index] = ScalarVector1::splat(idx);
        }
    }

    /// Получить текущее значение оператора (без обработки)
    pub fn peek_operator(&self, index: usize) -> T {
        if index < N {
            self.operators[index].phase()
        } else {
            T::ZERO
        }
    }

    /// Сбросить все операторы
    pub fn reset_all(&mut self) {
        for op in &mut self.operators {
            op.reset();
        }
    }
}

impl<T: Transcendental, const N: usize> Algorithm<T> for FmSynth<T, N> {
    fn init(&mut self, sample_rate: f32) {
        for op in &mut self.operators {
            op.init(sample_rate);
        }
    }

    fn reset(&mut self) {
        self.reset_all();
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            // Сохраняем текущие значения всех операторов
            let mut values = [T::ZERO; N];
            for i in 0..N {
                values[i] = self.operators[i].generate().extract(0);
            }

            // Применяем модуляцию согласно алгоритму
            for i in 0..N {
                let mut mod_sum = T::ZERO;

                // Суммируем все модуляции для этого оператора
                for j in 0..N {
                    if self.algorithm[i][j] {
                        mod_sum += values[j] * self.modulation_indices[j].extract(0);
                    }
                }

                // Применяем модуляцию, если есть
                if mod_sum != T::ZERO {
                    self.operators[i].modulate_frequency(mod_sum);
                }
            }

            // Последний оператор даёт выходной сигнал
            // (в классической FM архитектуре)
            *out = values[N - 1];
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        // Создаём статические строки для разных размеров
        match N {
            2 => AlgorithmMetadata {
                name: "2-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "2-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            3 => AlgorithmMetadata {
                name: "3-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "3-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            4 => AlgorithmMetadata {
                name: "4-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "4-operator FM synthesizer (DX7 style)",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            5 => AlgorithmMetadata {
                name: "5-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "5-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            6 => AlgorithmMetadata {
                name: "6-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "6-operator FM synthesizer (DX7 style)",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            _ => AlgorithmMetadata {
                name: "FM Synth",
                category: AlgorithmCategory::Generator,
                description: "Multi-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
        }
    }
}

// =============================================================================
// Вспомогательные функции и константы
// =============================================================================

/// Предустановленные алгоритмы для 4-операторного FM
pub mod algorithms_4op {
    /// Алгоритм 1: все операторы последовательно
    pub const ALGORITHM_1: [[bool; 4]; 4] = [
        [false, true, false, false],
        [false, false, true, false],
        [false, false, false, true],
        [false, false, false, false],
    ];

    /// Алгоритм 2: два параллельных каскада
    pub const ALGORITHM_2: [[bool; 4]; 4] = [
        [false, true, false, false],
        [false, false, false, false],
        [false, false, false, true],
        [false, false, false, false],
    ];

    /// Алгоритм 3: операторы 1 и 2 модулируют 3 и 4
    pub const ALGORITHM_3: [[bool; 4]; 4] = [
        [false, false, false, false],
        [false, false, false, false],
        [true, true, false, false],
        [false, false, false, false],
    ];
}

/// Предустановленные алгоритмы для 6-операторного FM (DX7 стиль)
pub mod algorithms_6op {
    /// Алгоритм 1: последовательная цепочка
    pub const ALGORITHM_1: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [false, false, true, false, false, false],
        [false, false, false, true, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];

    /// Алгоритм 2: два параллельных каскада по 3
    pub const ALGORITHM_2: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [false, false, true, false, false, false],
        [false, false, false, false, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];

    /// Алгоритм 3: сложная структура с обратными связями
    pub const ALGORITHM_3: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [true, false, true, false, false, false],
        [false, false, false, true, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::time::ClockTick;
    use rill_core::traits::ActionContext;

    #[test]
    fn test_simple_fm_synth() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        fm.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_simple_fm_with_different_waveforms() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5)
            .with_carrier_waveform(Waveform::Saw)
            .with_modulator_waveform(Waveform::Square);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        fm.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_simple_fm_parameters() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        assert_eq!(fm.frequency(), 440.0);
        assert_eq!(fm.ratio(), 2.0);
        assert_eq!(fm.modulation_index(), 1.5);

        fm.set_carrier_frequency(880.0);
        assert_eq!(fm.frequency(), 880.0);

        fm.set_ratio(3.0);
        assert_eq!(fm.ratio(), 3.0);

        fm.set_modulation_index(2.0);
        assert_eq!(fm.modulation_index(), 2.0);
    }

    #[test]
    fn test_fm_synth_4op() {
        let frequencies = [440.0, 880.0, 1320.0, 1760.0];
        let mut fm = FmSynth::<f32, 4>::new(frequencies, algorithms_4op::ALGORITHM_1);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        fm.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_fm_synth_6op() {
        let frequencies = [440.0, 880.0, 1320.0, 1760.0, 2200.0, 2640.0];
        let mut fm = FmSynth::<f32, 6>::new(frequencies, algorithms_6op::ALGORITHM_1);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        fm.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_fm_synth_set_waveform() {
        let frequencies = [440.0, 880.0];
        let algorithm = [[false, true], [false, false]];

        let mut fm = FmSynth::<f32, 2>::new(frequencies, algorithm);
        fm.init(44100.0);

        fm.set_waveform(0, Waveform::Saw);
        fm.set_waveform(1, Waveform::Square);

        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        fm.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_generator_trait() {
        use crate::generators::Generator;

        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        assert_eq!(fm.frequency(), 440.0);
        fm.set_frequency(880.0);
        assert_eq!(fm.frequency(), 880.0);

        fm.set_amplitude(0.5);
        assert_eq!(fm.amplitude(), 0.5);

        let phase = fm.phase();
        assert!(phase >= 0.0 && phase <= 1.0);
    }

    #[test]
    fn test_modulatable_trait() {
        use crate::generators::ModulatableGenerator;

        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        // Проверяем начальное значение
        assert_eq!(fm.modulation_index(), 1.5);

        // Модулируем частоту
        fm.modulate_frequency(0.3);
        assert_eq!(
            fm.modulation_index(),
            0.3,
            "modulation_index should be updated to 0.3"
        );

        // Устанавливаем новый индекс модуляции
        fm.set_modulation_index(0.8);
        assert_eq!(
            fm.modulation_index(),
            0.8,
            "modulation_index should be updated to 0.8"
        );
    }

    #[test]
    fn test_clone_copy() {
        let fm1 = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        let fm2 = fm1; // Копирование
        let fm3 = fm1.clone(); // Явное клонирование

        assert_eq!(fm1.frequency(), fm2.frequency());
        assert_eq!(fm1.frequency(), fm3.frequency());
        assert_eq!(fm1.ratio(), fm2.ratio());
    }
}
