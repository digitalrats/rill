//! Макросы для создания фильтров.

#[macro_export]
macro_rules! filter {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            $(state {
                $($state_name:ident: $state_type:ty = $state_init:expr),* $(,)?
            })?
        }
        
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
        }
        
        filter_type: $filter_type:expr,
        process_fn: $process:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $($param_name: $param_type),*,
            $($($state_name: $state_type),*)?
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($($state_name: $state_init),*)?
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                $($(self.$state_name = $state_init),*)?
            }
            
            fn process_sample(&mut self, input: f32) -> f32 {
                ($process)(self, input)
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Filter,
                    description: "Digital filter",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! butterworth {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
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
        $(#[$struct_meta])*
        $vis struct $name {
            cutoff: f32,
            sample_rate: f32,
            state: [f32; 4],
        }

        impl $name {
            pub fn new(cutoff: f32) -> Self {
                Self {
                    cutoff,
                    sample_rate: 44100.0,
                    state: [0.0; 4],
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.state = [0.0; 4];
            }
            
            fn process_sample(&mut self, input: f32) -> f32 {
                // Упрощенная реализация
                let alpha = self.cutoff / self.sample_rate;
                let output = alpha * input + (1.0 - alpha) * self.state[0];
                self.state[0] = output;
                output
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Filter,
                    description: "Butterworth filter",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! chebyshev {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                cutoff: f32 = $cutoff_default:expr,
                ripple: f32 = $ripple_default:expr,
            }
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        order: $order:expr,
        filter_type: $filter_type:expr,
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            cutoff: f32,
            ripple: f32,
            sample_rate: f32,
            state: [f32; 4],
        }

        impl $name {
            pub fn new(cutoff: f32, ripple: f32) -> Self {
                Self {
                    cutoff,
                    ripple,
                    sample_rate: 44100.0,
                    state: [0.0; 4],
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.state = [0.0; 4];
            }
            
            fn process_sample(&mut self, input: f32) -> f32 {
                // Упрощенная реализация
                let alpha = self.cutoff / self.sample_rate;
                let output = alpha * input + (1.0 - alpha) * self.state[0];
                self.state[0] = output;
                output
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Filter,
                    description: "Chebyshev filter",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}