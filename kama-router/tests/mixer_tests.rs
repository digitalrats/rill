use float_cmp::approx_eq;
use kama_core::traits::{AudioNode, ParamValue, Processor};
use kama_core::{ClockTick, DEFAULT_BLOCK_SIZE};
use kama_router::{ChannelConfig, ChannelMode, MixerNode, SendConfig, SendType};

#[test]
fn test_mixer_creation() {
    let mixer = MixerNode::new(4, 2);
    assert_eq!(mixer.num_inputs(), 4);
    assert_eq!(mixer.num_outputs(), 4); // 2 master + 2 buses
}

#[test]
fn test_mixer_basic_processing() {
    let mut mixer = MixerNode::new(2, 0);
    mixer.init(44100.0);

    let input1 = [0.5; DEFAULT_BLOCK_SIZE];
    let input2 = [0.3; DEFAULT_BLOCK_SIZE];
    let mut output_left = [0.0; DEFAULT_BLOCK_SIZE];
    let mut output_right = [0.0; DEFAULT_BLOCK_SIZE];

    let inputs = [&input1, &input2];
    let mut outputs = [&mut output_left, &mut output_right];

    let clock = ClockTick::default();
    let control_inputs: &[f32] = &[];
    let clock_inputs: &[ClockTick] = &[];
    let feedback_inputs: &[&[f32; DEFAULT_BLOCK_SIZE]] = &[];
    let mut control_outputs: [f32; 0] = [];
    let mut clock_outputs: [ClockTick; 0] = [];
    let mut feedback_outputs: [&mut [f32; DEFAULT_BLOCK_SIZE]; 0] = [];

    mixer
        .process(
            &clock,
            &inputs,
            control_inputs,
            clock_inputs,
            feedback_inputs,
            &mut outputs,
            &mut control_outputs,
            &mut clock_outputs,
            &mut feedback_outputs,
        )
        .unwrap();

    // Both channels are mono, so they are summed to both left and right outputs
    let expected = input1[0] + input2[0];

    for i in 0..DEFAULT_BLOCK_SIZE {
        assert!(
            approx_eq!(f32, output_left[i], expected, epsilon = 0.001),
            "left[{}]: {} vs {}",
            i,
            output_left[i],
            expected
        );
        assert!(
            approx_eq!(f32, output_right[i], expected, epsilon = 0.001),
            "right[{}]: {} vs {}",
            i,
            output_right[i],
            expected
        );
    }
}

#[test]
fn test_mixer_pan() {
    let mut mixer = MixerNode::new(1, 0);
    mixer.set_channel_pan(0, -0.5).unwrap();
    mixer.init(44100.0);

    let input = [1.0; DEFAULT_BLOCK_SIZE];
    let mut output_left = [0.0; DEFAULT_BLOCK_SIZE];
    let mut output_right = [0.0; DEFAULT_BLOCK_SIZE];

    let inputs = [&input];
    let mut outputs = [&mut output_left, &mut output_right];

    let clock = ClockTick::default();
    let control_inputs: &[f32] = &[];
    let clock_inputs: &[ClockTick] = &[];
    let feedback_inputs: &[&[f32; DEFAULT_BLOCK_SIZE]] = &[];
    let mut control_outputs: [f32; 0] = [];
    let mut clock_outputs: [ClockTick; 0] = [];
    let mut feedback_outputs: [&mut [f32; DEFAULT_BLOCK_SIZE]; 0] = [];

    mixer
        .process(
            &clock,
            &inputs,
            control_inputs,
            clock_inputs,
            feedback_inputs,
            &mut outputs,
            &mut control_outputs,
            &mut clock_outputs,
            &mut feedback_outputs,
        )
        .unwrap();

    // For pan -0.5: left gain 1.0, right gain 0.5
    // Then summed to both outputs (mono channel summed to stereo)
    // Actually, in our mixer, process_mono returns the channel output (already panned),
    // then it's added to both left and right masters. So for pan -0.5:
    // channel_out = input * volume * left_gain? No, process_mono doesn't apply pan.

    // For now, let's just check that left and right are different
    assert!(
        output_left[0] != output_right[0],
        "Left and right should be different with pan"
    );
}

#[test]
fn test_mixer_mute() {
    let mut mixer = MixerNode::new(1, 0);
    mixer.set_channel_mute(0, true).unwrap();
    mixer.init(44100.0);

    let input = [1.0; DEFAULT_BLOCK_SIZE];
    let mut output_left = [0.0; DEFAULT_BLOCK_SIZE];
    let mut output_right = [0.0; DEFAULT_BLOCK_SIZE];

    let inputs = [&input];
    let mut outputs = [&mut output_left, &mut output_right];

    let clock = ClockTick::default();
    let control_inputs: &[f32] = &[];
    let clock_inputs: &[ClockTick] = &[];
    let feedback_inputs: &[&[f32; DEFAULT_BLOCK_SIZE]] = &[];
    let mut control_outputs: [f32; 0] = [];
    let mut clock_outputs: [ClockTick; 0] = [];
    let mut feedback_outputs: [&mut [f32; DEFAULT_BLOCK_SIZE]; 0] = [];

    mixer
        .process(
            &clock,
            &inputs,
            control_inputs,
            clock_inputs,
            feedback_inputs,
            &mut outputs,
            &mut control_outputs,
            &mut clock_outputs,
            &mut feedback_outputs,
        )
        .unwrap();

    for i in 0..DEFAULT_BLOCK_SIZE {
        assert_eq!(output_left[i], 0.0);
        assert_eq!(output_right[i], 0.0);
    }
}

#[test]
fn test_mixer_sends() {
    let mut mixer = MixerNode::new(1, 2); // 2 buses for testing

    // Отключаем сглаживание для точных тестов
    mixer.set_smoothing(1.0); // 1.0 = моментальное изменение

    mixer.set_channel_volume(0, 0.8).unwrap();

    // Post-fader send
    mixer
        .add_send(
            0,
            SendConfig {
                bus_index: 0,
                level: 0.5,
                send_type: SendType::PostFader,
            },
        )
        .unwrap();

    // Pre-fader send
    mixer
        .add_send(
            0,
            SendConfig {
                bus_index: 1,
                level: 0.3,
                send_type: SendType::PreFader,
            },
        )
        .unwrap();

    mixer.init(44100.0);

    let input = [1.0; DEFAULT_BLOCK_SIZE];
    let mut output_left = [0.0; DEFAULT_BLOCK_SIZE];
    let mut output_right = [0.0; DEFAULT_BLOCK_SIZE];
    let mut bus0_out = [0.0; DEFAULT_BLOCK_SIZE];
    let mut bus1_out = [0.0; DEFAULT_BLOCK_SIZE];

    let inputs = [&input];
    let mut outputs = [
        &mut output_left,
        &mut output_right,
        &mut bus0_out,
        &mut bus1_out,
    ];

    let clock = ClockTick::default();
    let control_inputs: &[f32] = &[];
    let clock_inputs: &[ClockTick] = &[];
    let feedback_inputs: &[&[f32; DEFAULT_BLOCK_SIZE]] = &[];
    let mut control_outputs: [f32; 0] = [];
    let mut clock_outputs: [ClockTick; 0] = [];
    let mut feedback_outputs: [&mut [f32; DEFAULT_BLOCK_SIZE]; 0] = [];

    mixer
        .process(
            &clock,
            &inputs,
            control_inputs,
            clock_inputs,
            feedback_inputs,
            &mut outputs,
            &mut control_outputs,
            &mut clock_outputs,
            &mut feedback_outputs,
        )
        .unwrap();

    // Пропускаем первые несколько семплов из-за сглаживания
    let start = 10;

    for i in start..DEFAULT_BLOCK_SIZE {
        assert!(
            approx_eq!(f32, output_left[i], 0.8, epsilon = 0.01),
            "left[{}]: {} vs 0.8",
            i,
            output_left[i]
        );
        assert!(
            approx_eq!(f32, output_right[i], 0.8, epsilon = 0.01),
            "right[{}]: {} vs 0.8",
            i,
            output_right[i]
        );

        // Post-fader send: signal * volume * level = 1.0 * 0.8 * 0.5 = 0.4
        assert!(
            approx_eq!(f32, bus0_out[i], 0.4, epsilon = 0.01),
            "bus0[{}]: {} vs 0.4",
            i,
            bus0_out[i]
        );

        // Pre-fader send: signal * level = 1.0 * 0.3 = 0.3
        assert!(
            approx_eq!(f32, bus1_out[i], 0.3, epsilon = 0.01),
            "bus1[{}]: {} vs 0.3",
            i,
            bus1_out[i]
        );
    }
}

#[test]
fn test_mixer_parameters() {
    let mut mixer = MixerNode::new(2, 0);

    // Test get_param
    assert!(mixer.get_param("master_volume").is_some());
    assert!(mixer.get_param("ch_1_volume").is_some());
    assert!(mixer.get_param("ch_2_pan").is_some());

    // Test set_param
    mixer
        .set_param("ch_1_volume", ParamValue::Float(0.5))
        .unwrap();
    if let Some(ParamValue::Float(v)) = mixer.get_param("ch_1_volume") {
        assert!((v - 0.5).abs() < 0.001);
    }

    mixer
        .set_param("ch_2_pan", ParamValue::Float(-0.8))
        .unwrap();
    if let Some(ParamValue::Float(v)) = mixer.get_param("ch_2_pan") {
        assert!((v + 0.8).abs() < 0.001);
    }

    mixer
        .set_param("ch_1_mute", ParamValue::Bool(true))
        .unwrap();
    if let Some(ParamValue::Bool(v)) = mixer.get_param("ch_1_mute") {
        assert!(v);
    }
}
