//! Макросы для создания генераторов (источников сигнала).
//!
//! Генераторы - это узлы, которые производят сигнал без входов.
//! Они могут иметь состояние (фаза, счётчики) и параметры.
//!
//! # Пример
//! ```
//! use kama_core_dsp::macros::generator;
//! use kama_core_dsp::math::AudioNum;
//!
//! generator! {
//!     /// Sine wave oscillator
//!     pub SineOsc<T: AudioNum, const BLOCK_SIZE: usize> {
//!         params {
//!             /// Frequency in Hz
//!             frequency: f32 = 440.0,
//!             /// Amplitude (0.0 to 1.0)
//!             amplitude: f32 = 0.5,
//!         }
//!         
//!         state {
//!             /// Current phase (0.0 to 1.0)
//!             phase: T = T::ZERO
//!         }
//!     }
//!     
//!     ports {
//!         audio_out: 1,
//!     }
//!     
//!     generate_fn = |this| {
//!         use core::f32::consts::PI;
//!         
//!         let phase_rad = this.phase * T::from_f32(2.0 * PI);
//!         let sample = phase_rad.sin() * T::from_f32(this.amplitude);
//!         
//!         this.phase = this.phase + T::from_f32(this.frequency / this.sample_rate);
//!         if this.phase.as_f32() >= 1.0 {
//!             this.phase = this.phase - T::from_f32(1.0);
//!         }
//!         
//!         sample
//!     }
//! }
//! ```

#[macro_export]
macro_rules! generator {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            $(state {
                $($state_name:ident: $state_type:ty = $state_init:expr),* $(,)?
            })?
        }
        
        ports {
            audio_out: $audio_out:expr,
            $(control: $control:expr)?
        }
        
        generate_fn = $generate:expr
    ) => {
        // Реализация как в предыдущем ответе
    };
}

/// Специализированный макрос для LFO (низкочастотных генераторов)
#[macro_export]
macro_rules! lfo {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                /// Frequency in Hz (0.01 - 100.0)
                frequency: f32 = 1.0,
                /// Amplitude
                amplitude: T = T::from_f32(1.0),
                /// Offset (-1.0 to 1.0)
                offset: T = T::ZERO,
            }
            
            waveform: $waveform:expr
        }
        
        $(control: $control:expr)?
    ) => {
        generator! {
            $(#[$struct_meta])*
            $vis $name<T, BLOCK_SIZE> {
                params {
                    frequency: f32 = frequency,
                    amplitude: T = amplitude,
                    offset: T = offset,
                }
                
                state {
                    phase: T = T::ZERO
                }
            }
            
            ports {
                audio_out: 1,
                $(control: $control)?
            }
            
            generate_fn = |this| {
                use kama_core_dsp::generators::lfo_generate;
                let value = lfo_generate(this.phase, $waveform);
                
                this.phase = this.phase + T::from_f32(this.frequency / this.sample_rate);
                if this.phase.as_f32() >= 1.0 {
                    this.phase = this.phase - T::from_f32(1.0);
                }
                
                value * this.amplitude + this.offset
            }
        }
    };
}

/// Специализированный макрос для шумовых генераторов
#[macro_export]
macro_rules! noise_generator {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                /// Amplitude (0.0 to 1.0)
                amplitude: T = T::from_f32(1.0),
            }
            
            noise_type: $noise_type:expr
        }
        
        ports {
            audio_out: 1,
        }
    ) => {
        generator! {
            $(#[$struct_meta])*
            $vis $name<T, BLOCK_SIZE> {
                params {
                    amplitude: T = amplitude,
                }
                
                state {
                    rng_state: T = T::from_f32(123456789.0),
                    $(filter_state: [T; 6] = [T::ZERO; 6])?
                }
            }
            
            ports {
                audio_out: 1,
            }
            
            generate_fn = |this| {
                // Xorshift RNG
                let mut x = this.rng_state.as_f32();
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                this.rng_state = T::from_f32(x);
                
                let white = this.rng_state.fract() * T::from_f32(2.0) - T::from_f32(1.0);
                
                // Окраска шума в зависимости от типа
                let colored = match $noise_type {
                    NoiseType::White => white,
                    NoiseType::Pink => {
                        // Реализация розового шума
                        white // упрощённо
                    }
                    NoiseType::Brown => {
                        // Реализация броуновского шума
                        white // упрощённо
                    }
                };
                
                colored * this.amplitude
            }
        }
    };
}