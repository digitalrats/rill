//! Генераторы шума (White, Pink, Brown, Blue, Violet)

use super::Generator;
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::filters::{FilterParams, FilterType, OnePole};
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

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
pub struct NoiseGenerator<T: Transcendental> {
    /// Тип шума
    noise_type: NoiseType,
    /// Амплитуда
    amplitude: ScalarVector1<T>,
    /// Состояние RNG (Xorshift) - храним как u32 для битовых операций
    state: u32,
    /// Фильтры для окраски
    pink_filters: [OnePole<T>; 6],
    brown_state: ScalarVector1<T>,
    /// Частота дискретизации
    sample_rate: f32,
    /// Для синего шума
    last_white: ScalarVector1<T>,
    /// Для фиолетового шума
    last_white1: ScalarVector1<T>,
    last_white2: ScalarVector1<T>,
}

impl<T: Transcendental> NoiseGenerator<T> {
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
            amplitude: ScalarVector1::splat(amplitude),
            state: 123456789,
            pink_filters: [
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params),
            ],
            brown_state: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
            last_white: ScalarVector1::splat(T::ZERO),
            last_white1: ScalarVector1::splat(T::ZERO),
            last_white2: ScalarVector1::splat(T::ZERO),
        }
    }

    /// Xorshift RNG (работает с u32, возвращает f32 через Transcendental)
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
    fn generate_white(&mut self) -> ScalarVector1<T> {
        ScalarVector1::splat(self.xorshift()) * self.amplitude
    }

    /// Генерация розового шума (1/f)
    /// Метод Paul Kellett'a
    fn generate_pink(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();

        // 6-полосный фильтр для аппроксимации 1/f
        let mut output = T::ZERO;
        for filter in &mut self.pink_filters {
            output = output.add(filter.process_sample(white));
        }

        ScalarVector1::splat(output) * self.amplitude / ScalarVector1::splat(T::from_f32(3.0))
        // нормализация
    }

    /// Генерация броуновского шума (1/f²)
    fn generate_brown(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();

        // Интегратор с ограничением
        self.brown_state =
            self.brown_state + ScalarVector1::splat(white) * ScalarVector1::splat(T::from_f32(0.1));
        // Клиппинг
        let one_vec = ScalarVector1::splat(T::from_f32(1.0));
        let neg_one_vec = ScalarVector1::splat(T::from_f32(-1.0));
        self.brown_state = self.brown_state.clamp(&neg_one_vec, &one_vec);

        self.brown_state * self.amplitude
    }

    /// Генерация синего шума (+3dB/октава)
    fn generate_blue(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);

        // Дифференциатор (high-pass)
        let diff = white_vec - self.last_white;
        self.last_white = white_vec;

        diff * self.amplitude
    }

    /// Генерация фиолетового шума (+6dB/октава)
    fn generate_violet(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);

        // Двойной дифференциатор
        let diff1 = white_vec - self.last_white1;
        let diff2 = diff1 - self.last_white2;
        self.last_white2 = diff1;
        self.last_white1 = white_vec;

        diff2 * self.amplitude
    }
}

impl<T: Transcendental> Algorithm<T> for NoiseGenerator<T> {
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
        self.brown_state = ScalarVector1::splat(T::ZERO);
        self.last_white = ScalarVector1::splat(T::ZERO);
        self.last_white1 = ScalarVector1::splat(T::ZERO);
        self.last_white2 = ScalarVector1::splat(T::ZERO);
        for filter in &mut self.pink_filters {
            filter.reset();
        }
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            *out = match self.noise_type {
                NoiseType::White => self.generate_white().extract(0),
                NoiseType::Pink => self.generate_pink().extract(0),
                NoiseType::Brown => self.generate_brown().extract(0),
                NoiseType::Blue => self.generate_blue().extract(0),
                NoiseType::Violet => self.generate_violet().extract(0),
            };
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: self.noise_type.name(),
            category: AlgorithmCategory::Generator,
            description: self.noise_type.description(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> Generator<T> for NoiseGenerator<T> {
    fn phase(&self) -> T {
        T::ZERO
    } // Шум не имеет фазы

    fn set_phase(&mut self, _phase: T) {}

    fn frequency(&self) -> f32 {
        0.0
    }

    fn set_frequency(&mut self, _freq: f32) {}

    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }

    fn set_amplitude(&mut self, amp: T) {
        let one = T::from_f32(1.0);
        let clamped = if amp > one {
            one
        } else if amp < T::ZERO {
            T::ZERO
        } else {
            amp
        };
        self.amplitude = ScalarVector1::splat(clamped);
    }
}
