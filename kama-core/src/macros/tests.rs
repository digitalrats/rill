//! # Тесты для макросов

#[cfg(test)]
mod tests {
    use crate::math::AudioNum;
    use crate::traits::*;
    use std::f32::consts::PI;
    
    // Тестовый источник
    source_node! {
        /// Тестовый синус
        pub TestSine<T: AudioNum, const BUF_SIZE: usize>
        {
            params {
                frequency: f32 = 440.0,
                amplitude: T = T::from_f32(0.5),
            }
            
            ports {
                audio_out: 1,
            }
            
            generate: |this, output| {
                let phase_inc = this.frequency / this.sample_rate();
                
                for i in 0..output.len() {
                    let sample = (this.state().phase * 2.0 * PI).sin();
                    output[i] = T::from_f32(sample) * this.amplitude;
                    
                    this.state_mut().phase += phase_inc;
                    if this.state().phase >= 1.0 {
                        this.state_mut().phase -= 1.0;
                    }
                }
                
                Ok(())
            }
        }
    }
    
    // Тестовый процессор
    processor_node! {
        /// Тестовый усилитель
        pub TestGain<T: AudioNum, const BUF_SIZE: usize>
        {
            params {
                gain: T = T::from_f32(1.0),
            }
            
            ports {
                audio_in: 1,
                audio_out: 1,
            }
            
            process: |this, inputs, outputs| {
                if let (Some(input), Some(output)) = (inputs.first(), outputs.first_mut()) {
                    let input_slice = input.read()?;
                    let output_slice = output.write()?;
                    
                    for i in 0..input_slice.len().min(output_slice.len()) {
                        output_slice[i] = input_slice[i] * this.gain;
                    }
                }
                Ok(())
            }
        }
    }
    
    // Тестовый приёмник
    sink_node! {
        /// Тестовый приёмник
        pub TestSink<T: AudioNum, const BUF_SIZE: usize>
        {
            params {
                volume: T = T::from_f32(1.0),
            }
            
            ports {
                audio_in: 1,
            }
            
            consume: |_this, _inputs| {
                Ok(())
            }
        }
    }
    
    #[test]
    fn test_source_node() {
        let mut node = TestSine::<f32, 64>::new(44100.0);
        node.set_id(NodeId(1));
        
        assert_eq!(node.frequency, 440.0);
        assert!((node.amplitude.to_f32() - 0.5).abs() < 1e-6);
        assert_eq!(node.id(), NodeId(1));
        
        node.set_parameter("frequency", 880.0).unwrap();
        assert_eq!(node.frequency, 880.0);
        
        let freq = node.get_parameter("frequency").unwrap();
        assert_eq!(freq, 880.0);
    }
    
    #[test]
    fn test_processor_node() {
        let mut node = TestGain::<f32, 64>::new(44100.0);
        
        node.set_parameter("gain", 2.0).unwrap();
        assert_eq!(node.gain.to_f32(), 2.0);
        
        let names = node.parameter_names();
        assert!(names.contains(&"gain"));
    }
    
    #[test]
    fn test_sink_node() {
        let mut node = TestSink::<f32, 64>::new(44100.0);
        
        node.set_parameter("volume", 0.5).unwrap();
        assert_eq!(node.volume.to_f32(), 0.5);
    }
    
    #[test]
    fn test_with_parameters() {
        use crate::with_parameters;
        
        let node = TestGain::<f32, 64>::new(44100.0);
        let node = with_parameters!(node, gain: 3.0.into());
        
        assert_eq!(node.gain.to_f32(), 3.0);
    }
}