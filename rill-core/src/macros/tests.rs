//! # Тесты для макросов

#[cfg(test)]
mod tests {
    use crate::math::Transcendental;
    use crate::prelude::*;
    use crate::traits::IntoParamValue;
    use std::f32::consts::PI;

    // Импортируем макросы
    use crate::{processor_node, sink_node, source_node, with_parameters};

    // Тестовый источник
    source_node! {
        /// Тестовый синус
        pub TestSine<T: Transcendental, const BUF_SIZE: usize>
        {
            params {
                frequency: f32 = 440.0,
                amplitude: T = T::from_f32(0.5),
            }

            ports {
                audio_out: 1,
            }

            generate: |this: &mut TestSine<T, BUF_SIZE>| -> crate::ProcessResult<()> {
                let freq = this.frequency;
                let amp = this.amplitude;
                let sr = this.sample_rate();
                let phase_inc = T::from_f32(freq) / T::from_f32(sr);

                let mut temp = [T::ZERO; BUF_SIZE];
                for i in 0..BUF_SIZE {
                    let phase_rad = this.state().phase * T::from_f32(2.0 * PI);
                    let sample = phase_rad.sin();
                    temp[i] = sample * amp;

                    let new_phase = this.state().phase + phase_inc;
                    if new_phase >= T::from_f32(1.0) {
                        this.state_mut().phase = new_phase - T::from_f32(1.0);
                    } else {
                        this.state_mut().phase = new_phase;
                    }
                }
                *this.outputs[0].buffer.as_mut_array() = temp;
                Ok(())
            }
        }
    }

    // Тестовый процессор
    processor_node! {
        /// Тестовый усилитель
        pub TestGain<T: Transcendental, const BUF_SIZE: usize>
        {
            params {
                gain: T = T::from_f32(1.0),
            }

            ports {
                audio_in: 1,
                audio_out: 1,
            }

            process: |this: &mut TestGain<T, BUF_SIZE>| -> crate::ProcessResult<()> {
                if let (Some(input), Some(output)) = (this.inputs.first_mut(), this.outputs.first_mut()) {
                    let input_buf = *input.buffer.as_array();
                    let mut output_buf = [T::ZERO; BUF_SIZE];
                    for i in 0..BUF_SIZE {
                        output_buf[i] = input_buf[i] * this.gain;
                    }
                    output.buffer.copy_from(&output_buf);
                }
                Ok(())
            }
        }
    }

    // Тестовый приёмник
    sink_node! {
        /// Тестовый приёмник
        pub TestSink<T: Transcendental, const BUF_SIZE: usize>
        {
            params {
                volume: T = T::from_f32(1.0),
            }

            ports {
                audio_in: 1,
            }

            consume: |_this: &mut TestSink<T, BUF_SIZE>| -> crate::ProcessResult<()> {
                // Здесь можно обращаться к полям через _this, если нужно
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

        let param_id = ParameterId::new("frequency").unwrap();
        node.set_parameter(&param_id, ParamValue::Float(880.0))
            .unwrap();
        assert_eq!(node.frequency, 880.0);

        let freq = node.get_parameter(&param_id).unwrap().as_f32().unwrap();
        assert_eq!(freq, 880.0);
    }

    #[test]
    fn test_processor_node() {
        let mut node = TestGain::<f32, 64>::new(44100.0);

        let param_id = ParameterId::new("gain").unwrap();
        node.set_parameter(&param_id, ParamValue::Float(2.0))
            .unwrap();
        assert_eq!(node.gain.to_f32(), 2.0);
    }

    #[test]
    fn test_sink_node() {
        let mut node = TestSink::<f32, 64>::new(44100.0);

        let param_id = ParameterId::new("volume").unwrap();
        node.set_parameter(&param_id, ParamValue::Float(0.5))
            .unwrap();
        assert_eq!(node.volume.to_f32(), 0.5);
    }

    #[test]
    fn test_with_parameters() -> Result<(), Box<dyn std::error::Error>> {
        let node = TestGain::<f32, 64>::new(44100.0);
        let node = with_parameters!(node, gain: 3.0f32);

        assert_eq!(node.gain.to_f32(), 3.0);
        Ok(())
    }
}
