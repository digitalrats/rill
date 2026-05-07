//! Macro for creating a filter with coefficients
//!
//! # Example
//! ```
//! use rill_core_dsp::filter_algorithm;
//! use rill_core::math::Transcendental;
//!
//! filter_algorithm! {
//!     /// Biquad filter
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Biquad<T: Transcendental> {
//!         params: {
//!             cutoff: T = T::from_f32(1000.0),
//!             q: T = T::from_f32(0.707),
//!         },
//!         coeffs: {
//!             b0: T = T::ZERO,
//!             b1: T = T::ZERO,
//!             b2: T = T::ZERO,
//!             a1: T = T::ZERO,
//!             a2: T = T::ZERO,
//!         },
//!         state: {
//!             x1: T = T::ZERO,
//!             x2: T = T::ZERO,
//!             y1: T = T::ZERO,
//!             y2: T = T::ZERO,
//!         },
//!         update_coeffs: |this| {
//!             // Calculate coefficients from parameters
//!         },
//!         process: |this, input| {
//!             // Apply filter
//!             input
//!         }
//!     }
//! }
//! ```

/// Macro for creating a filter with coefficients
///
/// # Example
/// ```
/// use rill_core_dsp::filter_algorithm;
/// use rill_core::math::Transcendental;
///
/// filter_algorithm! {
///     /// Biquad filter
///     #[derive(Debug, Clone, Copy)]
///     pub struct Biquad<T: Transcendental> {
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
///             // Calculate coefficients from parameters
///         },
///         process: |this, input| {
///             // Apply filter
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

            /// Sample rate
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Create a new filter instance
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($coeff_name: $coeff_default),*,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                }
            }

            /// Update filter coefficients
            pub fn update_coeffs(&mut self) {
                let update_fn: fn(&mut Self) = $update;
                update_fn(self);
            }
        }

        impl<$($generic: $bound),+> $crate::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: rill_core::math::Transcendental,
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

            fn process(
                &mut self,
                input: Option<&[T]>,
                output: &mut [T],
                _ctx: &$crate::algorithm::ActionContext,
            ) -> $crate::algorithm::ProcessResult<()> {
                let input = input.unwrap_or(&[]);
                let len = input.len().min(output.len());
                let process_fn: fn(&mut Self, T) -> T = $process;
                for i in 0..len {
                    output[i] = process_fn(self, input[i]);
                }
                Ok(())
            }

            fn metadata(&self) -> $crate::algorithm::AlgorithmMetadata {
                $crate::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::algorithm::AlgorithmCategory::Filter,
                    description: stringify!($name),
                    author: "Rill",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}
