//! Integration tests for macros

use kama_core::traits::{Source, Processor, Sink};

#[test]
fn test_source_macro() {
    kama_core::source_node_f32! {
        /// Test source with control input
        #[derive(Debug)]
        struct TestSource {
            params: {
                frequency: f32 = 440.0,
                amplitude: f32 = 0.5
            },
            control_inputs: {
                mod_amount: f32 = 0.0
            },
            state: {
                phase: f32 = 0.0
            },
            outputs: 1,
            generate: |this, _channel, output, control| {
                let mod_amt = control[0];
                let phase_inc = (this.frequency + mod_amt * 100.0) / this.sample_rate;
                for i in 0..kama_core::DEFAULT_BLOCK_SIZE {
                    output[i] = (this.phase * 2.0 * std::f32::consts::PI).sin() * this.amplitude;
                    this.phase = (this.phase + phase_inc) % 1.0;
                }
            }
        }
    }

    let mut source = TestSource::new(440.0, 0.5, 0.0);
    source.init(44100.0);
    
    assert_eq!(source.num_audio_outputs(), 1);
    assert_eq!(source.num_control_inputs(), 1);
    
    let mut output = [[0.0; kama_core::DEFAULT_BLOCK_SIZE]];
    let mut outputs = [&mut output[0]];
    let control = [0.0];
    let result = source.generate(&mut outputs, &control);
    assert!(result.is_ok());
}

#[test]
fn test_processor_macro() {
    kama_core::processor_node_f32! {
        /// Test processor with control input
        #[derive(Debug)]
        struct TestProcessor {
            params: {
                gain: f32 = 1.0
            },
            control_inputs: {
                mod_gain: f32 = 0.0
            },
            state: {
                last_sample: f32 = 0.0
            },
            inputs: 1,
            outputs: 1,
            process: |this, _channel, input, output, control| {
                let total_gain = this.gain + control[0];
                for i in 0..kama_core::DEFAULT_BLOCK_SIZE {
                    output[i] = input[i] * total_gain;
                    this.last_sample = output[i];
                }
            }
        }
    }

    let mut proc = TestProcessor::new(1.0, 0.0);
    proc.init(44100.0);
    
    assert_eq!(proc.num_audio_inputs(), 1);
    assert_eq!(proc.num_audio_outputs(), 1);
    assert_eq!(proc.num_control_inputs(), 1);
    
    let input = [[1.0; kama_core::DEFAULT_BLOCK_SIZE]];
    let mut output = [[0.0; kama_core::DEFAULT_BLOCK_SIZE]];
    let inputs = [&input[0]];
    let mut outputs = [&mut output[0]];
    let control = [0.5];
    let result = proc.process(&inputs, &mut outputs, &control);
    assert!(result.is_ok());
    assert_eq!(output[0][0], 1.5);
}

#[test]
fn test_sink_macro() {
    kama_core::sink_node_f32! {
        /// Test sink with control input
        #[derive(Debug)]
        struct TestSink {
            params: {
                gain: f32 = 1.0
            },
            control_inputs: {
                volume: f32 = 1.0
            },
            state: {
                processed: u64 = 0
            },
            inputs: 1,
            sink: |this, _channel, input, control| {
                let vol = control[0];
                for &sample in input {
                    let _ = sample * this.gain * vol;
                    this.processed += 1;
                }
            }
        }
    }

    let mut sink = TestSink::new(1.0, 1.0);
    sink.init(44100.0);
    
    assert_eq!(sink.num_audio_inputs(), 1);
    assert_eq!(sink.num_control_inputs(), 1);
    
    let input = [[1.0; kama_core::DEFAULT_BLOCK_SIZE]];
    let inputs = [&input[0]];
    let control = [0.5];
    let result = sink.process(&inputs, &control);
    assert!(result.is_ok());
    assert_eq!(sink.processed, kama_core::DEFAULT_BLOCK_SIZE as u64);
}