//! Макрос для создания алгоритма с параметрами
//!
//! # Пример
//! ```
//! use kama_core_dsp::parameterized_algorithm;
//! use kama_core::math::AudioNum;
//!
//! parameterized_algorithm! {
//!     /// Фильтр с изменяемой частотой среза
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct LowPass<T: AudioNum> {
//!         params: {
//!             /// Частота среза в Hz
//!             cutoff: T = T::from_f32(1000.0),
//!             /// Добротность
//!             q: T = T::from_f32(0.707),
//!         },
//!         state: {
//!             /// Внутреннее состояние фильтра
//!             y1: T = T::ZERO,
//!             y2: T = T::ZERO,
//!         },
//!         update: |this| {
//!             // Обновление коэффициентов при изменении параметров
//!         },
//!         process: |this, input| {
//!             // Процессинг с текущими параметрами
//!             input
//!         }
//!     }
//! }
//! ```

/// Макрос для создания алгоритма с параметрами
///
/// # Пример
/// ```
/// use kama_core_dsp::parameterized_algorithm;
/// use kama_core::math::AudioNum;
///
/// parameterized_algorithm! {
///     /// Фильтр с изменяемой частотой среза
///     #[derive(Debug, Clone, Copy)]
///     pub struct LowPass<T: AudioNum> {
///         params: {
///             /// Частота среза в Hz
///             cutoff: T = T::from_f32(1000.0),
///             /// Добротность
///             q: T = T::from_f32(0.707),
///         },
///         state: {
///             /// Внутреннее состояние фильтра
///             y1: T = T::ZERO,
///             y2: T = T::ZERO,
///         },
///         update: |this| {
///             // Обновление коэффициентов при изменении параметров
///         },
///         process: |this, input| {
///             // Процессинг с текущими параметрами
///             input
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! parameterized_algorithm {
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
            update: $update:expr,
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

            /// Частота дискретизации
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Создать новый экземпляр алгоритма
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                }
            }

            /// Обновить внутренние коэффициенты
            pub fn update_coeffs(&mut self) {
                let update_fn: fn(&mut Self) = $update;
                update_fn(self);
            }
        }

        impl<$($generic: $bound),+> $crate::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: kama_core::math::AudioNum,
        {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
                self.update_coeffs();
            }

            fn reset(&mut self) {
                $(
                    self.$state_name = $state_default;
                )*
            }

            fn process_block(&mut self, input: &[T], output: &mut [T]) {
                let len = input.len().min(output.len());
                let process_fn: fn(&mut Self, T) -> T = $process;
                for i in 0..len {
                    output[i] = process_fn(self, input[i]);
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
