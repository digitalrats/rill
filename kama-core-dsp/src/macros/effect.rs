//! Макросы для создания эффектов.
//!
//! Эффекты - это узлы с входом и выходом, которые могут иметь
//! внутреннее состояние и буферы.
//!
//! # Пример
//! ```
//! use kama_core_dsp::macros::effect;
//! use kama_core_dsp::math::AudioNum;
//! use kama_core_dsp::buffer::DelayLine;
//!
//! effect! {
//!     /// Delay effect
//!     pub DelayEffect<T: AudioNum, const BLOCK_SIZE: usize> {
//!         params {
//!             /// Delay time in seconds
//!             delay_time: f32 = 0.5,
//!             /// Feedback amount
//!             feedback: f32 = 0.3,
//!             /// Dry/wet mix
//!             mix: f32 = 0.5,
//!         }
//!         
//!         buffers {
//!             delay: DelayLine<T, {BLOCK_SIZE * 4}> = DelayLine::new(),
//!         }
//!         
//!         state {
//!             delay_samples: usize = 0
//!         }
//!     }
//!     
//!     ports {
//!         audio_in: 1,
//!         audio_out: 1,
//!     }
//!     
//!     init_fn = |this| {
//!         this.delay_samples = (this.delay_time * this.sample_rate) as usize;
//!     }
//!     
//!     process_fn = |this, input| {
//!         let delayed = this.delay.read();
//!         let output = input * (1.0 - this.mix) + delayed * this.mix;
//!         let write_sample = input + delayed * this.feedback;
//!         let _ = this.delay.write(write_sample);
//!         output
//!     }
//! }
//! ```

#[macro_export]
macro_rules! effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            $(buffers {
                $(
                    $(#[$buf_meta:meta])*
                    $buf_name:ident: $buf_type:ty = $buf_init:expr
                ),* $(,)?
            })?
            
            $(state {
                $($state_name:ident: $state_type:ty = $state_init:expr),* $(,)?
            })?
        }
        
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
            $(control: $control:expr)?
        }
        
        $(init_fn: $init:expr,)?
        $(reset_fn: $reset:expr,)?
        process_fn: $process:expr
    ) => {
        // Реализация (как в предыдущем ответе, но с запятыми)
    };
}

/// Макрос для создания эффекта с dry/wet
#[macro_export]
macro_rules! dry_wet_effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            $(buffers { $($buf:tt)* })?
            $(state { $($state:tt)* })?
            
            wet: f32 = $wet_default:expr,
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        effect_fn: $effect:expr,
    ) => {
        // Реализация с запятыми
    };
}