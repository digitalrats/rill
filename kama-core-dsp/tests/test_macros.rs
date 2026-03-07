//! Тесты для макросов DSP алгоритмов
//!
//! Эти тесты вынесены в отдельный файл, чтобы избежать конфликтов
//! с экспортом макросов и проблем видимости.

use kama_core::math::AudioNum;
use kama_core_dsp::algorithm::Algorithm;

// Макросы доступны напрямую из корня крейта благодаря #[macro_export]
use kama_core_dsp::{
    simple_algorithm,
    parameterized_algorithm,
    filter_algorithm,
    effect_algorithm,
    generator_algorithm,
};

#[test]
fn test_simple_algorithm_f32() {
    simple_algorithm! {
        /// Test gain
        #[derive(Debug, Clone, Copy)]
        pub struct TestGain<T: AudioNum> {
            params: {
                gain: T = T::from_f32(2.0),
            },
            state: {
                last: T = T::ZERO,
            },
            process: |this, input| {
                let out = input * this.gain;
                this.last = out;
                out
            }
        }
    }

    let mut gain = TestGain::<f32>::new(2.0);
    assert_eq!(gain.process_sample(1.0), 2.0);
    assert_eq!(gain.last, 2.0);
}

#[test]
fn test_simple_algorithm_f64() {
    simple_algorithm! {
        /// Test gain
        #[derive(Debug, Clone, Copy)]
        pub struct TestGain<T: AudioNum> {
            params: {
                gain: T = T::from_f32(2.0),
            },
            state: {
                last: T = T::ZERO,
            },
            process: |this, input| {
                let out = input * this.gain;
                this.last = out;
                out
            }
        }
    }

    let mut gain = TestGain::<f64>::new(2.0);
    assert_eq!(gain.process_sample(1.0), 2.0);
    assert!((gain.last - 2.0).abs() < 1e-10);
}

#[test]
fn test_parameterized_algorithm() {
    parameterized_algorithm! {
        /// Test parameterized
        #[derive(Debug, Clone, Copy)]
        pub struct TestParam<T: AudioNum> {
            params: {
                value: T = T::from_f32(1.0),
            },
            state: {
                last: T = T::ZERO,
            },
            update: |_this| {
                // Update coefficients based on params
            },
            process: |this, input| {
                let out = input * this.value;
                this.last = out;
                out
            }
        }
    }

    let mut algo = TestParam::<f32>::new(2.0);
    assert_eq!(algo.process_sample(1.0), 2.0);
}

#[test]
fn test_filter_algorithm() {
    filter_algorithm! {
        /// Test filter
        #[derive(Debug, Clone, Copy)]
        pub struct TestFilter<T: AudioNum> {
            params: {
                cutoff: T = T::from_f32(1000.0),
                q: T = T::from_f32(0.707),
            },
            coeffs: {
                b0: T = T::ZERO,
                b1: T = T::ZERO,
                b2: T = T::ZERO,
                a1: T = T::ZERO,
                a2: T = T::ZERO,
            },
            state: {
                x1: T = T::ZERO,
                x2: T = T::ZERO,
                y1: T = T::ZERO,
                y2: T = T::ZERO,
            },
            update_coeffs: |this| {
                // Calculate coefficients from params
                this.b0 = T::ONE;
                this.b1 = T::ZERO;
                this.b2 = T::ZERO;
                this.a1 = T::ZERO;
                this.a2 = T::ZERO;
            },
            process: |_this, input| {
                // Simple passthrough for testing
                input
            }
        }
    }

    let mut filter = TestFilter::<f32>::new(1000.0, 0.707);
    filter.init(44100.0);
    assert_eq!(filter.process_sample(1.0), 1.0);
}

#[test]
fn test_effect_algorithm_f32() {
    effect_algorithm! {
        /// Test effect
        #[derive(Debug, Clone, Copy)]
        pub struct TestEffect<T: AudioNum> {
            params: {
                amount: T = T::from_f32(0.5),
            },
            state: {
                last: T = T::ZERO,
            },
            wet: T::from_f32(0.3),
            process: |this, input| {
                let out = input * this.amount;
                this.last = out;
                out
            }
        }
    }

    let mut effect = TestEffect::<f32>::new(0.5);
    effect.set_wet(0.7);
    let result = effect.process_sample(1.0);
    let expected = 0.5 * 1.0 * 0.7 + 1.0 * 0.3;
    assert!((result - expected).abs() < 1e-6);
}

#[test]
fn test_effect_algorithm_f64() {
    effect_algorithm! {
        /// Test effect
        #[derive(Debug, Clone, Copy)]
        pub struct TestEffect<T: AudioNum> {
            params: {
                amount: T = T::from_f32(0.5),
            },
            state: {
                last: T = T::ZERO,
            },
            wet: T::from_f32(0.3),
            process: |this, input| {
                let out = input * this.amount;
                this.last = out;
                out
            }
        }
    }

    let mut effect = TestEffect::<f64>::new(0.5);
    effect.set_wet(0.7);
    let result = effect.process_sample(1.0);
    let expected = 0.5 * 1.0 * 0.7 + 1.0 * 0.3;
    assert!((result - expected).abs() < 1e-10);
}

#[test]
fn test_generator_algorithm_f32() {
    generator_algorithm! {
        /// Test generator
        #[derive(Debug, Clone, Copy)]
        pub struct TestGen<T: AudioNum> {
            params: {
                value: T = T::from_f32(1.0),
            },
            state: {
                counter: i32 = 0,
            },
            generate: |this| {
                this.counter += 1;
                this.value
            }
        }
    }

    let mut gen = TestGen::<f32>::new(1.0);
    assert_eq!(gen.process_sample(0.0), 1.0);
    assert_eq!(gen.counter, 1);
}

#[test]
fn test_generator_algorithm_f64() {
    generator_algorithm! {
        /// Test generator
        #[derive(Debug, Clone, Copy)]
        pub struct TestGen<T: AudioNum> {
            params: {
                value: T = T::from_f32(1.0),
            },
            state: {
                counter: i32 = 0,
            },
            generate: |this| {
                this.counter += 1;
                this.value
            }
        }
    }

    let mut gen = TestGen::<f64>::new(1.0);
    assert_eq!(gen.process_sample(0.0), 1.0);
    assert_eq!(gen.counter, 1);
}