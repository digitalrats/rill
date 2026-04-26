//! Макрос для создания генератора
//!
//! # Пример
//! use rill_core_dsp::generator_algorithm;
//! use rill_core::math::AudioNum;
//!
//! generator_algorithm! {
//!     /// Генератор синуса
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct SineGen<T: AudioNum> {
//!         params: {
//!             freq: T = T::from_f32(440.0),
//!             amp: T = T::from_f32(0.5),
//!         },
//!         state: {
//!             phase: T = T::ZERO,
//!         },
//!         generate: |this| {
//!             let output = (this.phase * T::from_f32(2.0 * std::f32::consts::PI)).sin() * this.amp;
//!             let phase_inc = this.freq / T::from_f32(this.sample_rate);
//!             this.phase = (this.phase + phase_inc) % T::from_f32(1.0);
//!             output
//!         }
//!     }
//! }

/// Макрос для создания генератора
///
/// # Пример
/// use rill_core_dsp::generator_algorithm;
/// use rill_core::math::AudioNum;
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
            T: rill_core::math::AudioNum,
        {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }

            fn reset(&mut self) {
                $(
                    self.$state_name = $state_default;
                )*
            }

            fn process(
                &mut self,
                _input: Option<&[T]>,
                output: &mut [T],
                _ctx: &$crate::algorithm::ActionContext,
            ) -> $crate::algorithm::ProcessResult<()> {
                let generate_fn: fn(&mut Self) -> T = $generate;
                for out in output.iter_mut() {
                    *out = generate_fn(self);
                }
                Ok(())
            }

            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Generator,
                    description: stringify!($name),
                    author: "Rill",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}
