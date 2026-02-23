//! # Высокоточные фильтры
//!
//! Предоставляет реализации фильтров с повышенной точностью:
//!
//! - [`HighPrecisionBiquad`] — биквадратный фильтр (LowPass, HighPass, BandPass, Notch, Peak, Shelf)
//! - [`HighPrecisionLadderFilter`] — лестничный фильтр (Moog ladder)

use std::f64::consts::PI;

/// Тип биквадратного фильтра.
#[derive(Debug, Clone, Copy)]
pub enum BiquadType {
    /// Фильтр нижних частот
    LowPass,
    /// Фильтр верхних частот
    HighPass,
    /// Полосовой фильтр
    BandPass,
    /// Режекторный фильтр
    Notch,
    /// Пиковый фильтр (эквалайзер)
    Peak,
    /// Полочный фильтр низких частот
    LowShelf,
    /// Полочный фильтр высоких частот
    HighShelf,
}

/// Высокоточный биквадратный фильтр.
pub struct HighPrecisionBiquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    sample_rate: f64,
    filter_type: BiquadType,
    frequency: f64,
    q: f64,
    gain_db: f64, // для пиковых и shelving фильтров
}

impl HighPrecisionBiquad {
    /// Создать фильтр нижних частот.
    pub fn new_lowpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let mut filter = Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            filter_type: BiquadType::LowPass,
            frequency: cutoff,
            q,
            gain_db: 0.0,
        };
        filter.update_coefficients();
        filter
    }

    /// Создать фильтр верхних частот.
    pub fn new_highpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let mut filter = Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            filter_type: BiquadType::HighPass,
            frequency: cutoff,
            q,
            gain_db: 0.0,
        };
        filter.update_coefficients();
        filter
    }

    /// Создать полосовой фильтр.
    pub fn new_bandpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let mut filter = Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            filter_type: BiquadType::BandPass,
            frequency: cutoff,
            q,
            gain_db: 0.0,
        };
        filter.update_coefficients();
        filter
    }

    /// Создать режекторный фильтр.
    pub fn new_notch(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let mut filter = Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            filter_type: BiquadType::Notch,
            frequency: cutoff,
            q,
            gain_db: 0.0,
        };
        filter.update_coefficients();
        filter
    }

    /// Создать пиковый фильтр с усилением.
    pub fn new_peak(frequency: f64, q: f64, gain_db: f64, sample_rate: f64) -> Self {
        let mut filter = Self {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            filter_type: BiquadType::Peak,
            frequency,
            q,
            gain_db,
        };
        filter.update_coefficients();
        filter
    }

    /// Обновить коэффициенты на основе текущих параметров
    fn update_coefficients(&mut self) {
        let omega = 2.0 * PI * self.frequency / self.sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * self.q);

        match self.filter_type {
            BiquadType::LowPass => {
                let b0 = (1.0 - cos_omega) / 2.0;
                let b1 = 1.0 - cos_omega;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::HighPass => {
                let b0 = (1.0 + cos_omega) / 2.0;
                let b1 = -(1.0 + cos_omega);
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::BandPass => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::Notch => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::Peak => {
                let a = 10.0_f64.powf(self.gain_db / 40.0);
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha / a;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::LowShelf => {
                let a = 10.0_f64.powf(self.gain_db / 40.0);
                let beta = (a * alpha).sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_omega + 2.0 * beta * sin_omega);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_omega - 2.0 * beta * sin_omega);
                let a0 = (a + 1.0) + (a - 1.0) * cos_omega + 2.0 * beta * sin_omega;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) + (a - 1.0) * cos_omega - 2.0 * beta * sin_omega;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }

            BiquadType::HighShelf => {
                let a = 10.0_f64.powf(self.gain_db / 40.0);
                let beta = (a * alpha).sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_omega + 2.0 * beta * sin_omega);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_omega - 2.0 * beta * sin_omega);
                let a0 = (a + 1.0) - (a - 1.0) * cos_omega + 2.0 * beta * sin_omega;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) - (a - 1.0) * cos_omega - 2.0 * beta * sin_omega;

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
        }
    }

    /// Обработать один семпл.
    pub fn process(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;

        output
    }

    /// Обработать буфер целиком.
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process(input[i]);
        }
    }

    /// Изменить частоту среза.
    pub fn set_cutoff(&mut self, cutoff: f64) {
        self.frequency = cutoff.max(20.0).min(self.sample_rate / 2.0);
        self.update_coefficients();
    }

    /// Изменить добротность.
    pub fn set_q(&mut self, q: f64) {
        self.q = q.max(0.1).min(20.0);
        self.update_coefficients();
    }

    /// Изменить усиление (для peak/shelving фильтров).
    pub fn set_gain_db(&mut self, gain_db: f64) {
        self.gain_db = gain_db;
        self.update_coefficients();
    }

    /// Сбросить внутреннее состояние.
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Высокоточный лестничный фильтр (Moog ladder).
pub struct HighPrecisionLadderFilter {
    cutoff: f64,
    resonance: f64,
    sample_rate: f64,
    stage1: f64,
    stage2: f64,
    stage3: f64,
    stage4: f64,
}

impl HighPrecisionLadderFilter {
    /// Создать новый лестничный фильтр.
    pub fn new(cutoff: f64, resonance: f64, sample_rate: f64) -> Self {
        Self {
            cutoff: cutoff.max(20.0).min(sample_rate / 2.0),
            resonance: resonance.clamp(0.0, 1.0),
            sample_rate,
            stage1: 0.0,
            stage2: 0.0,
            stage3: 0.0,
            stage4: 0.0,
        }
    }

    /// Установить частоту среза.
    pub fn set_cutoff(&mut self, cutoff: f64) {
        self.cutoff = cutoff.max(20.0).min(self.sample_rate / 2.0);
    }

    /// Установить резонанс.
    pub fn set_resonance(&mut self, resonance: f64) {
        self.resonance = resonance.clamp(0.0, 1.0);
    }

    /// Обработать один семпл.
    pub fn process(&mut self, input: f64) -> f64 {
        let f = 2.0 * (PI * self.cutoff / self.sample_rate).sin();
        let fb = self.resonance * 4.0;

        let x = input - fb * self.stage4;

        self.stage1 = x * f + self.stage1 - f * self.stage1;
        self.stage2 = self.stage1 * f + self.stage2 - f * self.stage2;
        self.stage3 = self.stage2 * f + self.stage3 - f * self.stage3;
        self.stage4 = self.stage3 * f + self.stage4 - f * self.stage4;

        self.stage4
    }

    /// Обработать буфер целиком.
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process(input[i]);
        }
    }

    /// Сбросить состояние.
    pub fn reset(&mut self) {
        self.stage1 = 0.0;
        self.stage2 = 0.0;
        self.stage3 = 0.0;
        self.stage4 = 0.0;
    }
}
