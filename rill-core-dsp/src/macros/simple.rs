//! Macro for creating a simple algorithm without parameters
//!
//! # Example
//! ```
//! use rill_core_dsp::simple_algorithm;
//! use rill_core::math::Transcendental;
//!
//! simple_algorithm! {
//!     /// Simple gain
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Gain<T: Transcendental> {
//!         params: {
//!     /// Gain coefficient
//!             gain: T = T::from_f32(1.0),
//!         },
//!         state: {
//!     /// Last output value (for statistics)
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

/// Macro for creating a simple algorithm without parameters
///
/// # Example
/// ```
/// use rill_core_dsp::simple_algorithm;
/// use rill_core::math::Transcendental;
///
/// simple_algorithm! {
///     /// Simple gain
///     #[derive(Debug, Clone, Copy)]
///     pub struct Gain<T: Transcendental> {
///         params: {
///     /// Gain coefficient
///             gain: T = T::from_f32(1.0),
///         },
///         state: {
///     /// Last output value (for statistics)
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
            /// Create a new algorithm instance
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                }
            }
        }

        impl<$($generic: $bound),+> rill_core::traits::algorithm::Algorithm<T> for $name<$($generic),+>
        where
            T: rill_core::math::Transcendental,
        {
            fn init(&mut self, _sample_rate: f32) {}

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
                for i in 0..len {
                    output[i] = process_fn(self, input[i]);
                }
                Ok(())
            }

            fn metadata(&self) -> rill_core::traits::algorithm::AlgorithmMetadata {
                rill_core::traits::algorithm::AlgorithmMetadata {
                    name: stringify!($name),
                    category: rill_core::traits::algorithm::AlgorithmCategory::Utility,
                    description: stringify!($name),
                    author: "Rill",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}
