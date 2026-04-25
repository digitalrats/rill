use rill_io::{
    processor::{GainProcessor, MonoMixerProcessor, PassThroughProcessor, SilenceProcessor},
    AudioProcessor,
};

#[cfg(feature = "examples")]
use rill_io::processor::SineProcessor;

mod mocks;
use mocks::test_config;

#[test]
fn test_gain_processor() {
    let mut proc = GainProcessor::new(2.0);
    let input = vec![0.1, 0.2, 0.3];
    let mut output = vec![0.0; 3];
    proc.process(&input, &mut output);
    assert_eq!(output, vec![0.2, 0.4, 0.6]);

    proc.set_gain(0.5);
    let mut output2 = vec![0.0; 3];
    proc.process(&input, &mut output2);
    assert_eq!(output2, vec![0.05, 0.1, 0.15]);
}

#[test]
fn test_passthrough_processor() {
    let mut proc = PassThroughProcessor;
    let input = vec![1.0, -1.0, 0.5];
    let mut output = vec![0.0; 3];
    proc.process(&input, &mut output);
    assert_eq!(output, input);
}

#[test]
fn test_silence_processor() {
    let mut proc = SilenceProcessor;
    let input = vec![1.0, 2.0, 3.0];
    let mut output = vec![1.0; 3];
    proc.process(&input, &mut output);
    assert_eq!(output, vec![0.0; 3]);
}

#[test]
fn test_mono_mixer_processor() {
    let mut proc = MonoMixerProcessor;
    let input = vec![0.8, 0.2, 0.5, 0.5, 1.0, 0.0];
    let mut output = vec![0.0; 3];
    proc.process(&input, &mut output);
    assert_eq!(output, vec![0.5, 0.5, 0.5]);
}

#[test]
#[cfg(feature = "examples")]
fn test_sine_processor() {
    let sample_rate = 44100.0;
    let mut proc = SineProcessor::new(440.0, sample_rate);

    let mut output = vec![0.0; 1024];
    proc.process(&[], &mut output);

    assert!(output.iter().any(|&x| x != 0.0));
    for &s in &output {
        assert!(s >= -1.0 && s <= 1.0);
    }

    proc.set_frequency(880.0);
    let mut output2 = vec![0.0; 1024];
    proc.process(&[], &mut output2);
}
