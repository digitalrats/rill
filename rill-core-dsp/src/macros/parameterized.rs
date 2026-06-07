//! Macro for creating a parameterized algorithm
//!
//! # Example
//! ```
//! use rill_core_dsp::parameterized_algorithm;
//! use rill_core::math::Transcendental;
//!
//! parameterized_algorithm! {
//!     /// Filter with variable cutoff frequency
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct LowPass<T: Transcendental> {
//!         params: {
//!             /// Cutoff frequency in Hz
//!             cutoff: T = T::from_f32(1000.0),
//!             /// Quality factor
//!             q: T = T::from_f32(0.707),
//!         },
//!         state: {
//!             /// Internal filter state
//!             y1: T = T::ZERO,
//!             y2: T = T::ZERO,
//!         },
//!         update: |this| {
//!             // Update coefficients when parameters change
//!         },
//!         process: |this, input| {
//!             // Process with current parameters
//!             input
//!         }
//!     }
//! }
//! ```

/// Macro for creating a parameterized algorithm
///
/// # Example
/// ```
/// use rill_core_dsp::parameterized_algorithm;
/// use rill_core::math::Transcendental;
///
/// parameterized_algorithm! {
///     /// Filter with variable cutoff frequency
///     #[derive(Debug, Clone, Copy)]
///     pub struct LowPass<T: Transcendental> {
///         params: {
///             /// Cutoff frequency in Hz
///             cutoff: T = T::from_f32(1000.0),
///             /// Quality factor
///             q: T = T::from_f32(0.707),
///         },
///         state: {
///             /// Internal filter state
///             y1: T = T::ZERO,
///             y2: T = T::ZERO,
///         },
///         update: |this| {
///             // Update coefficients when parameters change
///         },
///         process: |this, input| {
///             // Process with current parameters
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

            /// Sample rate
            pub sample_rate: f32,
        }

        impl<$($generic: $bound),+> $name<$($generic),+> {
            /// Create a new algorithm instance
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($state_name: $state_default),*,
                    sample_rate: 44100.0,
                }
            }

            /// Update internal coefficients
            pub fn update_coeffs(&mut self) {
                let update_fn: fn(&mut Self) = $update;
                update_fn(self);
            }
        }

        impl<$($generic: $bound),+> rill_core::traits::algorithm::Algorithm<T> for $name<$($generic),+>
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
