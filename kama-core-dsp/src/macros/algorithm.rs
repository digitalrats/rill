//! # Макросы для создания DSP алгоритмов
//!
//! Этот модуль предоставляет макросы для удобного создания DSP алгоритмов,
//! реализующих трейты из `crate::algorithm` и использующих `AudioNum` из `kama_core`.
//!
//! ## Доступные макросы
//!
//! - [`simple_algorithm!`] - для простых алгоритмов без параметров
//! - [`parameterized_algorithm!`] - для алгоритмов с параметрами
//! - [`filter_algorithm!`] - для фильтров (с коэффициентами)
//! - [`effect_algorithm!`] - для эффектов (с dry/wet)
//! - [`generator_algorithm!`] - для генераторов
//!
//! ## Пример
//!
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

// ============================================================================
// SIMPLE ALGORITHM
// ============================================================================

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

// ============================================================================
// PARAMETERIZED ALGORITHM
// ============================================================================

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
            
            fn process_sample(&mut self, input: T) -> T {
                let process_fn: fn(&mut Self, T) -> T = $process;
                process_fn(self, input)
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

// ============================================================================
// FILTER ALGORITHM
// ============================================================================

/// Макрос для создания фильтра с коэффициентами
///
/// # Пример
/// ```
/// use kama_core_dsp::filter_algorithm;
/// use kama_core::math::AudioNum;
///
/// filter_algorithm! {
///     /// Биквадратный фильтр
///     #[derive(Debug, Clone, Copy)]
///     pub struct Biquad<T: AudioNum> {
///         params: {
///             cutoff: T = T::from_f32(1000.0),
///             q: T = T::from_f32(0.707),
///         },
///         coeffs: {
///             b0: T = T::ZERO,
///             b1: T = T::ZERO,
///             b2: T = T::ZERO,
///             a1: T = T::ZERO,
///             a2: T = T::ZERO,
///         },
///         state: {
///             x1: T = T::ZERO,
///             x2: T = T::ZERO,
///             y1: T = T::ZERO,
///             y2: T = T::ZERO,
///         },
///         update_coeffs: |this| {
///             // Расчет коэффициентов из параметров
///         },
///         process: |this, input| {
///             // Применение фильтра
///             input
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! filter_algorithm {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident<$($generic:ident: $bound:path),+> {
            params: {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident : $param_type:ty = $param_default:expr
                ),* $(,)?
            },
            coeffs: {
                $(
                    $(#[$coeff_meta:meta])*
                    $coeff_name:ident : $coeff_type:ty = $coeff_default:expr
                ),* $(,)?
            },
            state: {
                $(
                    $(#[$state_meta:meta])*
                    $state_name:ident : $state_type:ty = $state_default:expr
                ),* $(,)?
            },
            update_coeffs: $update:expr,
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
                $(#[$coeff_meta])*
                pub $coeff_name: $coeff_type,
            )*
            
            $(
                $(#[$state_meta])*
                pub $state_name: $state_type,
            )*
            
            /// Частота дискретизации
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Создать новый экземпляр фильтра
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($coeff_name: $coeff_default),*,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                }
            }
            
            /// Обновить коэффициенты фильтра
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
            
            fn process_sample(&mut self, input: T) -> T {
                let process_fn: fn(&mut Self, T) -> T = $process;
                process_fn(self, input)
            }
            
            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Filter,
                    description: stringify!($name),
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

// ============================================================================
// EFFECT ALGORITHM
// ============================================================================

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

// ============================================================================
// GENERATOR ALGORITHM
// ============================================================================

/// Макрос для создания генератора
///
/// # Пример
/// use kama_core_dsp::generator_algorithm;
/// use kama_core::math::AudioNum;
///
/// generator_algorithm! {
///     /// Генератор синуса
///     #[derive(Debug, Clone, Copy)]
///     pub struct SineGen<T: AudioNum> {
///         params: {
///             freq: T = T::from_f32(440.0),
///             amp: T = T::from_f32(0.5),
///         },
///         state: {
///             phase: T = T::ZERO,
///         },
///         generate: |this| {
///             let output = (this.phase * T::from_f32(2.0 * std::f32::consts::PI)).sin() * this.amp;
///             let phase_inc = this.freq / T::from_f32(this.sample_rate);
///             this.phase = (this.phase + phase_inc) % T::from_f32(1.0);
///             output
///         }
///     }
/// }
#[macro_export]
macro_rules! generator_algorithm {
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
            generate: $generate:expr
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
            /// Создать новый экземпляр генератора
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                }
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
            
            fn process_sample(&mut self, _input: T) -> T {
                let generate_fn: fn(&mut Self) -> T = $generate;
                generate_fn(self)
            }
            
            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Generator,
                    description: stringify!($name),
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}