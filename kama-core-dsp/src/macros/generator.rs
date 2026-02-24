//! Макросы для создания генераторов.

#[macro_export]
macro_rules! generator {
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
            audio_out: $audio_out:expr,
        }
        
        generate_fn = $generate:expr
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $($param_name: $param_type),*,
            $($($state_name: $state_type),*)?
            sample_rate: f32,
            phase: f32,
        }

        impl $name {
            pub fn new($($param_name: $param_type),*) -> Self {
                Self {
                    $($param_name),*,
                    $($($state_name: $state_init),*)?
                    sample_rate: 44100.0,
                    phase: 0.0,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.phase = 0.0;
            }
            
            fn process_sample(&mut self, _input: f32) -> f32 {
                let sample = ($generate)(self);
                sample
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Generator,
                    description: "Generated oscillator",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! lfo {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                frequency: f32 = $freq_default:expr,
                amplitude: f32 = $amp_default:expr,
                offset: f32 = $offset_default:expr,
            }
            
            waveform: $waveform:expr
        }
        
        ports {
            audio_out: 1,
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            frequency: f32,
            amplitude: f32,
            offset: f32,
            phase: f32,
            sample_rate: f32,
        }

        impl $name {
            pub fn new(frequency: f32, amplitude: f32, offset: f32) -> Self {
                Self {
                    frequency,
                    amplitude,
                    offset,
                    phase: 0.0,
                    sample_rate: 44100.0,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.phase = 0.0;
            }
            
            fn process_sample(&mut self, _input: f32) -> f32 {
                use std::f32::consts::PI;
                
                let phase_inc = self.frequency / self.sample_rate;
                let raw = match $waveform {
                    $crate::generators::Waveform::Sine => (self.phase * 2.0 * PI).sin(),
                    $crate::generators::Waveform::Triangle => {
                        if self.phase < 0.5 {
                            4.0 * self.phase - 1.0
                        } else {
                            3.0 - 4.0 * self.phase
                        }
                    }
                    $crate::generators::Waveform::Saw => 2.0 * self.phase - 1.0,
                    $crate::generators::Waveform::Square => {
                        if self.phase < 0.5 { 1.0 } else { -1.0 }
                    }
                    _ => (self.phase * 2.0 * PI).sin(),
                };
                
                self.phase += phase_inc;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                
                raw * self.amplitude + self.offset
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Generator,
                    description: "LFO generator",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! noise_generator {
    (
        $(#[$struct_meta:meta])*
        $vis:vis $name:ident {
            params {
                amplitude: f32 = $amp_default:expr,
            }
            
            noise_type: $noise_type:expr
        }
        
        ports {
            audio_out: 1,
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            amplitude: f32,
            sample_rate: f32,
            state: u32,
        }

        impl $name {
            pub fn new(amplitude: f32) -> Self {
                Self {
                    amplitude,
                    sample_rate: 44100.0,
                    state: 123456789,
                }
            }
        }

        impl $crate::Algorithm<f32> for $name {
            fn init(&mut self, sample_rate: f32) {
                self.sample_rate = sample_rate;
            }
            
            fn reset(&mut self) {
                self.state = 123456789;
            }
            
            fn process_sample(&mut self, _input: f32) -> f32 {
                // Xorshift RNG
                let mut x = self.state;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                self.state = x;
                
                let white = (x as f32 / 2147483648.0) - 1.0;
                white * self.amplitude
            }
            
            fn metadata(&self) -> $crate::AlgorithmMetadata {
                $crate::AlgorithmMetadata {
                    name: stringify!($name),
                    category: $crate::AlgorithmCategory::Generator,
                    description: "Noise generator",
                    author: "Kama Audio",
                    version: env!("CARGO_PKG_VERSION"),
                }
            }
        }
    };
}