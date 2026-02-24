//! Макросы для создания эффектов.

#[macro_export]
macro_rules! effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            $(buffers {
                $($buf_name:ident: $buf_type:ty = $buf_init:expr),* $(,)?
            })?
            
            $(state {
                $($state_name:ident: $state_type:ty = $state_init:expr),* $(,)?
            })?
        }
        
        ports {
            audio_in: $audio_in:expr,
            audio_out: $audio_out:expr,
        }
        
        process_fn: $process:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $($param_name: $param_type),*,
            $($($buf_name: $buf_type),*)?
            $($($state_name: $state_type),*)?
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($($buf_name: $buf_init),*)?
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
                    category: $crate::AlgorithmCategory::Effect,
                    description: "Audio effect",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! dry_wet_effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
            
            wet: f32 = $wet_default:expr,
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        effect_fn: $effect:expr,
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $($param_name: $param_type),*,
            wet: f32,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($param_name: $param_type),*, wet: f32) -> Self {
                Self {
                    $($param_name),*,
                    wet,
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {}
            
            fn process_sample(&mut self, input: f32) -> f32 {
                let wet_signal = ($effect)(self, input);
                input * (1.0 - self.wet) + wet_signal * self.wet
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Effect,
                    description: "Effect with dry/wet control",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! stereo_effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                $(
                    $(#[$param_meta:meta])*
                    $param_name:ident: $param_type:ty = $param_default:expr
                ),* $(,)?
            }
        }
        
        process_stereo_fn: $process:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $($param_name: $param_type),*,
            sample_rate: f32,
        }

        impl $name {
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {}
            
            fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
                ($process)(self, left, right)
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Effect,
                    description: "Stereo effect",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! delay_effect {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                delay_time: f32 = $delay_default:expr,
                feedback: f32 = $fb_default:expr,
                mix: f32 = $mix_default:expr,
            }
        }
        
        ports {
            audio_in: 1,
            audio_out: 1,
        }
        
        max_delay_ms: $max_delay:expr,
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            delay_time: f32,
            feedback: f32,
            mix: f32,
            buffer: Vec<f32>,
            write_pos: usize,
            sample_rate: f32,
        }

        impl $name {
            pub fn new(delay_time: f32, feedback: f32, mix: f32) -> Self {
                let max_delay_samples = ($max_delay * 44100.0 / 1000.0) as usize;
                Self {
                    delay_time,
                    feedback,
                    mix,
                    buffer: vec![0.0; max_delay_samples],
                    write_pos: 0,
                    sample_rate: 44100.0,
                }
            }
            
            fn delay_samples(&self) -> usize {
                (self.delay_time * self.sample_rate) as usize
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
                self.buffer.resize((self.delay_time * sample_rate) as usize * 2, 0.0);
                self.write_pos = 0;
            }
            
            fn reset(&mut self) {
                self.buffer.fill(0.0);
                self.write_pos = 0;
            }
            
            fn process_sample(&mut self, input: f32) -> f32 {
                let delay = self.delay_samples();
                let read_pos = (self.write_pos + self.buffer.len() - delay) % self.buffer.len();
                let delayed = self.buffer[read_pos];
                
                let output = input * (1.0 - self.mix) + delayed * self.mix;
                
                self.buffer[self.write_pos] = input + delayed * self.feedback;
                self.write_pos = (self.write_pos + 1) % self.buffer.len();
                
                output
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Effect,
                    description: "Delay effect",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}