//! # Высокоточные осцилляторы
//!
//! Предоставляет генераторы сигналов с высокой точностью:
//!
//! - [`HighPrecisionSineOsc`] — чистый синус
//! - [`HighPrecisionFMOsc`] — частотная модуляция

use std::f64::consts::PI;

/// Высокоточный синусоидальный осциллятор.
pub struct HighPrecisionSineOsc {
    frequency: f64,
    phase: f64,
    sample_rate: f64,
    amplitude: f64,
}

impl HighPrecisionSineOsc {
    /// Создать новый осциллятор.
    ///
    /// # Аргументы
    /// * `frequency` — частота в Hz
    /// * `sample_rate` — частота дискретизации
    /// * `amplitude` — амплитуда (0.0-1.0)
    pub fn new(frequency: f64, sample_rate: f64, amplitude: f64) -> Self {
        Self {
            frequency,
            phase: 0.0,
            sample_rate,
            amplitude: amplitude.max(0.0).min(1.0),
        }
    }

    /// Установить частоту.
    pub fn set_frequency(&mut self, frequency: f64) {
        self.frequency = frequency.max(0.0).min(self.sample_rate / 2.0);
    }

    /// Установить амплитуду.
    pub fn set_amplitude(&mut self, amplitude: f64) {
        self.amplitude = amplitude.max(0.0).min(1.0);
    }

    /// Сгенерировать блок семплов (моно).
    pub fn generate(&mut self, output: &mut [f64]) {
        let phase_inc = 2.0 * PI * self.frequency / self.sample_rate;

        for out in output.iter_mut() {
            *out = self.phase.sin() * self.amplitude;
            self.phase += phase_inc;

            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }

    /// Сгенерировать стерео блок.
    pub fn generate_stereo(&mut self, left: &mut [f64], right: &mut [f64]) {
        let phase_inc = 2.0 * PI * self.frequency / self.sample_rate;
        let len = left.len().min(right.len());

        for i in 0..len {
            let val = self.phase.sin() * self.amplitude;
            left[i] = val;
            right[i] = val;

            self.phase += phase_inc;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }

    /// Сбросить фазу.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Получить текущую фазу.
    pub fn phase(&self) -> f64 {
        self.phase
    }
}

/// Высокоточный FM осциллятор.
pub struct HighPrecisionFMOsc {
    carrier_freq: f64,
    modulator_freq: f64,
    modulation_index: f64,
    carrier_phase: f64,
    modulator_phase: f64,
    sample_rate: f64,
    amplitude: f64,
}

impl HighPrecisionFMOsc {
    /// Создать новый FM осциллятор.
    ///
    /// # Аргументы
    /// * `carrier_freq` — частота несущей
    /// * `modulator_freq` — частота модулятора
    /// * `modulation_index` — индекс модуляции
    /// * `sample_rate` — частота дискретизации
    /// * `amplitude` — амплитуда
    pub fn new(
        carrier_freq: f64,
        modulator_freq: f64,
        modulation_index: f64,
        sample_rate: f64,
        amplitude: f64,
    ) -> Self {
        Self {
            carrier_freq,
            modulator_freq,
            modulation_index,
            carrier_phase: 0.0,
            modulator_phase: 0.0,
            sample_rate,
            amplitude: amplitude.max(0.0).min(1.0),
        }
    }

    /// Установить частоту несущей.
    pub fn set_carrier_freq(&mut self, freq: f64) {
        self.carrier_freq = freq.max(0.0).min(self.sample_rate / 2.0);
    }

    /// Установить частоту модулятора.
    pub fn set_modulator_freq(&mut self, freq: f64) {
        self.modulator_freq = freq.max(0.0).min(self.sample_rate / 2.0);
    }

    /// Установить индекс модуляции.
    pub fn set_modulation_index(&mut self, index: f64) {
        self.modulation_index = index.max(0.0);
    }

    /// Сгенерировать блок семплов.
    pub fn generate(&mut self, output: &mut [f64]) {
        let carrier_inc = 2.0 * PI * self.carrier_freq / self.sample_rate;
        let modulator_inc = 2.0 * PI * self.modulator_freq / self.sample_rate;

        for out in output.iter_mut() {
            let modulation = self.modulator_phase.sin() * self.modulation_index;
            *out = (self.carrier_phase + modulation).sin() * self.amplitude;

            self.carrier_phase += carrier_inc;
            self.modulator_phase += modulator_inc;

            if self.carrier_phase > 2.0 * PI {
                self.carrier_phase -= 2.0 * PI;
            }
            if self.modulator_phase > 2.0 * PI {
                self.modulator_phase -= 2.0 * PI;
            }
        }
    }

    /// Сбросить фазы.
    pub fn reset(&mut self) {
        self.carrier_phase = 0.0;
        self.modulator_phase = 0.0;
    }
}
