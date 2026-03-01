//! Макрос для создания простого алгоритма без параметров
//!
//! # Пример
//! ```
//! use kama_core_dsp::simple_algorithm;
//! use kama_core::math::AudioNum;
//!
//! simple_algorithm! {
//!     /// Простой усилитель
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Gain<T: AudioNum> {
//!         params: {
//!             /// Коэффициент усиления
//!             gain: T = T::from_f32(1.0),
//!         },
//!         state: {
//!             /// Последнее значение (для статистики)
//!             last_output: T = T::ZERO,
//!         },
//!         process: |this, input| {
//!             let output = input * this.gain;
//!             this.last_output = output;
//!             output
//!         }
//!     }
//! }
//! ```

/// Макрос для создания простого алгоритма без параметров
///
/// # Пример
/// ```
/// use kama_core_dsp::simple_algorithm;
/// use kama_core::math::AudioNum;
///
/// simple_algorithm! {
///     /// Простой усилитель
///     #[derive(Debug, Clone, Copy)]
///     pub struct Gain<T: AudioNum> {
///         params: {
///             /// Коэффициент усиления
///             gain: T = T::from_f32(1.0),
///         },
///         state: {
///             /// Последнее значение (для статистики)
///             last_output: T = T::ZERO,
///         },
///         process: |this, input| {
///             let output = input * this.gain;
///             this.last_output = output;
///             output
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! simple_algorithm {
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
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Создать новый экземпляр алгоритма
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                }
            }
        }

        impl<$($generic: $bound),+> $crate::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: kama_core::math::AudioNum,
        {
            fn init(&mut self, _sample_rate: f32) {}
            
            fn reset(&mut self) {
                $(
                    self.$state_name = $state_default;
                )*
            }
            
            fn process_sample(&mut self, input: T) -> T {
                let process_fn: fn(&mut Self, T) -> T = $process;
                process_fn(self, input)
            }
            
            fn process_block(&mut self, input: &[T], output: &mut [T]) {
                let len = input.len().min(output.len());
                for i in 0..len {
                    output[i] = self.process_sample(input[i]);
                }
            }
            
            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Utility,
                    description: stringify!($name),
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}