//! Макросы для создания фильтров.
//!
//! Фильтры - это специализированные эффекты, которые реализуют трейт `Filter`.
//! Они имеют частоту среза, добротность и другие параметры.
//!
//! # Пример
//! ```
//! use kama_core_dsp::macros::filter;
//! use kama_core_dsp::math::AudioNum;
//!
//! filter! {
//!     /// Biquad low-pass filter
//!     pub LowPassFilter<T: AudioNum, const BLOCK_SIZE: usize> {
//!         params {
//!             /// Cutoff frequency in Hz
//!             cutoff: f32 = 1000.0,
//!             /// Q factor (resonance)
//!             q: f32 = 0.707,
//!         }
//!         
//!         state {
//!             x1: T = T::ZERO,
//!             x2: T = T::ZERO,
//!             y1: T = T::ZERO,
//!             y2: T = T::ZERO,
//!         }
//!     }
//!     
//!     ports {
//!         audio_in: 1,
//!         audio_out: 1,
//!     }
//!     
//!     filter_type = FilterType::LowPass
//!     
//!     update_coeffs_fn = |this| {
//!         let omega = 2.0 * PI * this.cutoff / this.sample_rate;
//!         let sin_omega = omega.sin();
//!         let cos_omega = omega.cos();
//!         let alpha = sin_omega / (2.0 * this.q);
//!         
//!         this.b0 = ((1.0 - cos_omega) / 2.0) / (1.0 + alpha);
//!         this.b1 = (1.0 - cos_omega) / (1.0 + alpha);
//!         this.b2 = this.b0;
//!         this.a1 = (-2.0 * cos_omega) / (1.0 + alpha);
//!         this.a2 = (1.0 - alpha) / (1.0 + alpha);
//!     }
//!     
//!     process_fn = |this, input| {
//!         let output = this.b0 * input + this.b1 * this.x1 + this.b2 * this.x2
//!                     - this.a1 * this.y1 - this.a2 * this.y2;
//!         
//!         this.x2 = this.x1;
//!         this.x1 = input;
//!         this.y2 = this.y1;
//!         this.y1 = output;
//!         
//!         output
//!     }
//! }

#[macro_export]
macro_rules! filter {
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
                $($buf_name:ident: $buf_type:ty = $buf_init:expr),* $(,)?
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
        
        filter_type: $filter_type:expr,
        
        $(update_coeffs_fn: $update:expr,)?
        
        process_fn: $process:expr
    ) => {
        // Реализация с использованием effect_node
        $crate::effect! {
            $(#[$struct_meta])*
            $vis $name<T, BLOCK_SIZE> {
                params {
                    $($param_name: $param_type = $param_default),*
                }
                
                $(buffers {
                    $($buf_name: $buf_type = $buf_init),*
                })?
                
                $(state {
                    $($state_name: $state_type = $state_init),*
                })?
            }
            
            ports {
                audio_in: $audio_in,
                audio_out: $audio_out,
                $(control: $control)?
            }
            
            $(init_fn: |this| { $update(this); })?
            
            process_fn: $process
        }
        
        impl<T: AudioNum, const BLOCK_SIZE: usize> $crate::filters::Filter<T> 
            for $name<T, BLOCK_SIZE> 
        {
            fn filter_type(&self) -> $crate::filters::FilterType {
                $filter_type
            }
            
            fn update_coefficients(&mut self) {
                $(($update)(self);)?
            }
        }
    };
}

/// Макрос для создания фильтра Баттерворта
#[macro_export]
macro_rules! butterworth {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                cutoff: f32 = $cutoff_default:expr,
            }
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        order: $order:expr,
        filter_type: $filter_type:expr,
    ) => {
        filter! {
            $(#[$struct_meta])*
            $vis $name<T, BLOCK_SIZE> {
                params {
                    cutoff: f32 = $cutoff_default,
                }
                
                buffers {
                    sections: [BiquadSection<T>; {$order / 2 + 1}] = [BiquadSection::new(); {$order / 2 + 1}],
                }
                
                state {
                    num_sections: usize = 0,
                    gain: T = T::from_f32(1.0),
                }
            }
            
            ports {
                audio_in: 1,
                audio_out: 1,
            }
            
            filter_type: $filter_type,
            
            update_coeffs_fn: |this| {
                use $crate::filters::butterworth::*;
                let poles = butterworth_poles($order);
                this.num_sections = poles.len() / 2;
                
                // Расчёт коэффициентов для каждой секции
                let warp_cutoff = 2.0 * (core::f32::consts::PI * this.cutoff / this.sample_rate).tan();
                
                for i in 0..this.num_sections {
                    let p1 = poles[i * 2];
                    let p2 = if i * 2 + 1 < poles.len() { poles[i * 2 + 1] } else { p1.conj() };
                    
                    // Bilinear transform
                    let sp1 = p1 * warp_cutoff as f64;
                    let sp2 = p2 * warp_cutoff as f64;
                    
                    let zp1 = (2.0 + sp1) / (2.0 - sp1);
                    let zp2 = (2.0 + sp2) / (2.0 - sp2);
                    
                    // Коэффициенты секции
                    let a1 = -(zp1 + zp2).re;
                    let a2 = (zp1 * zp2).re;
                    
                    this.sections[i].set_coeffs(1.0, 2.0, 1.0, a1, a2);
                }
                
                // Расчёт gain
                this.gain = T::from_f32(butterworth_gain($order, this.cutoff, this.sample_rate));
            },
            
            process_fn: |this, input| {
                let mut x = input * this.gain;
                for i in 0..this.num_sections {
                    x = this.sections[i].process(x);
                }
                x
            }
        }
    };
}

/// Макрос для создания фильтра Чебышева
#[macro_export]
macro_rules! chebyshev {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident<T: AudioNum, const BLOCK_SIZE: usize> {
            params {
                cutoff: f32 = $cutoff_default:expr,
                ripple_db: f32 = $ripple_default:expr,
            }
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        order: $order:expr,
        filter_type: $filter_type:expr,
        chebyshev_type: $chebyshev_type:expr, // TypeI или TypeII
    ) => {
        // Аналогично butterworth, но с использованием chebyshev_poles()
    };
}