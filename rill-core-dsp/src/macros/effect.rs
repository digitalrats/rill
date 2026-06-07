//! Macro for creating an effect with dry/wet mix
//!
//! # Example
//! ```
//! use rill_core_dsp::effect_algorithm;
//! use rill_core::math::Transcendental;
//!
//! effect_algorithm! {
//!     /// Delay effect
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Delay<T: Transcendental> {
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
//!             // Process effect
//!             input
//!         }
//!     }
//! }
//! ```

/// Macro for creating an effect with dry/wet mix
///
/// # Example
/// ```
/// use rill_core_dsp::effect_algorithm;
/// use rill_core::math::Transcendental;
///
/// effect_algorithm! {
///     /// Delay effect
///     #[derive(Debug, Clone, Copy)]
///     pub struct Delay<T: Transcendental> {
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
///             // Process effect
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

            /// Dry/wet coefficient (0.0 = fully dry, 1.0 = fully wet)
            pub wet: T,

            /// Sample rate
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Create a new effect instance
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                    wet: $wet_default,
                    sample_rate: 44100.0,
                }
            }

            /// Set dry/wet ratio
            pub fn set_wet(&mut self, wet: T) {
                self.wet = wet.clamp(T::ZERO, T::ONE);
            }
        }

        impl<$($generic: $bound),+> rill_core::traits::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: rill_core::math::Transcendental,
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
                input: Option<&[T]>,
                output: &mut [T],
            ) -> rill_core::traits::ProcessResult<()> {
                let input = input.unwrap_or(&[]);
                let len = input.len().min(output.len());
                let process_fn: fn(&mut Self, T) -> T = $process;
                let wet = self.wet;
                let one = T::ONE;
                for i in 0..len {
                    let wet_signal = process_fn(self, input[i]);
                    let dry = input[i] * (one - wet);
                    let wet_mixed = wet_signal * wet;
                    output[i] = dry + wet_mixed;
                }
                Ok(())
            }

            fn metadata(&self) -> rill_core::traits::algorithm::AlgorithmMetadata {
                rill_core::traits::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: rill_core::traits::algorithm::AlgorithmCategory::Effect,
                    description: stringify!($name),
                    author: "Rill",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}
