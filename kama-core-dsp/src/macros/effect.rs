//! Макрос для создания эффекта с dry/wet миксом
//!
//! # Пример
//! ```
//! use kama_core_dsp::effect_algorithm;
//! use kama_core::math::AudioNum;
//!
//! effect_algorithm! {
//!     /// Эффект задержки
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Delay<T: AudioNum> {
//!         params: {
//!             time: T = T::from_f32(0.3),
//!             feedback: T = T::from_f32(0.5),
//!         },
//!         state: {
//!             buffer: [T; 1024] = [T::ZERO; 1024],
//!             pos: usize = 0,
//!         },
//!         wet: T::from_f32(0.5),
//!         process: |this, input| {
//!             // Обработка эффекта
//!             input
//!         }
//!     }
//! }
//! ```

/// Макрос для создания эффекта с dry/wet миксом
///
/// # Пример
/// ```
/// use kama_core_dsp::effect_algorithm;
/// use kama_core::math::AudioNum;
///
/// effect_algorithm! {
///     /// Эффект задержки
///     #[derive(Debug, Clone, Copy)]
///     pub struct Delay<T: AudioNum> {
///         params: {
///             time: T = T::from_f32(0.3),
///             feedback: T = T::from_f32(0.5),
///         },
///         state: {
///             buffer: [T; 1024] = [T::ZERO; 1024],
///             pos: usize = 0,
///         },
///         wet: T::from_f32(0.5),
///         process: |this, input| {
///             // Обработка эффекта
///             input
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! effect_algorithm {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident<$($generic:ident: $bound:path),+> {
            params: {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident : $param_type:ty = $param_default:expr
                ),* $(,)?
            },
            state: {
                $(
                    $(#[$state_meta:meta])*
                    $state_name:ident : $state_type:ty = $state_default:expr
                ),* $(,)?
            },
            wet: $wet_default:expr,
            process: $process:expr
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name<$($generic: $bound),+> {
            $(
                $(#[$param_meta])*
                pub $param_name: $param_type,
            )*
            
            $(
                $(#[$state_meta])*
                pub $state_name: $state_type,
            )*
            
            /// Коэффициент dry/wet (0.0 = только dry, 1.0 = только wet)
            pub wet: T,
            
            /// Частота дискретизации
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Создать новый экземпляр эффекта
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                    wet: $wet_default,
                    sample_rate: 44100.0,
                }
            }
            
            /// Установить соотношение dry/wet
            pub fn set_wet(&mut self, wet: T) {
                self.wet = wet.clamp(T::ZERO, T::ONE);
            }
        }

        impl<$($generic: $bound),+> $crate::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: kama_core::math::AudioNum,
        {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                $(
                    self.$state_name = $state_default;
                )*
            }
            
            fn process_sample(&mut self, input: T) -> T {
                let process_fn: fn(&mut Self, T) -> T = $process;
                let wet = process_fn(self, input);
                
                // Dry/wet mix
                let one = T::ONE;
                let dry = input * (one - self.wet);
                let wet = wet * self.wet;
                
                dry + wet
            }
            
            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Effect,
                    description: stringify!($name),
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}