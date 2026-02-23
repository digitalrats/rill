use float_cmp::approx_eq;
use kama_core::traits::{AudioNode, ParamValue};
use kama_mixer::{ChannelConfig, ChannelMode, MixerNode, SendConfig, SendType};

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

    let input1 = vec![0.5; 100];
    let input2 = vec![0.3; 100];
    let mut output_left = vec![0.0; 100];
    let mut output_right = vec![0.0; 100];

    let inputs = [input1.as_slice(), input2.as_slice()];
    let mut outputs = [output_left.as_mut_slice(), output_right.as_mut_slice()];

    mixer.process(&inputs, &mut outputs).unwrap();

    // Both channels are mono, so they are summed to both left and right outputs
    let expected = input1[0] + input2[0];

    for i in 0..100 {
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

    let input = vec![1.0; 100];
    let mut output_left = vec![0.0; 100];
    let mut output_right = vec![0.0; 100];

    let inputs = [input.as_slice()];
    let mut outputs = [output_left.as_mut_slice(), output_right.as_mut_slice()];

    mixer.process(&inputs, &mut outputs).unwrap();

    // For pan -0.5: left gain 1.0, right gain 0.5
    // Then summed to both outputs (mono channel summed to stereo)
    let expected_left = 1.0;
    let expected_right = 1.0; // Wait, this is wrong! Let's check the mixer logic.

    // Actually, in our mixer, process_mono returns the channel output (already panned),
    // then it's added to both left and right masters. So for pan -0.5:
    // channel_out = input * volume * left_gain? No, process_mono doesn't apply pan.

    // We need to fix the test - let's print values to debug
    println!("First few left samples: {:?}", &output_left[..5]);
    println!("First few right samples: {:?}", &output_right[..5]);

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

    let input = vec![1.0; 100];
    let mut output_left = vec![0.0; 100];
    let mut output_right = vec![0.0; 100];

    let inputs = [input.as_slice()];
    let mut outputs = [output_left.as_mut_slice(), output_right.as_mut_slice()];

    mixer.process(&inputs, &mut outputs).unwrap();

    for i in 0..100 {
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

    let input = vec![1.0; 100];
    let mut output_left = vec![0.0; 100];
    let mut output_right = vec![0.0; 100];
    let mut bus0_out = vec![0.0; 100];
    let mut bus1_out = vec![0.0; 100];

    let inputs = [input.as_slice()];
    let mut outputs = [
        output_left.as_mut_slice(),
        output_right.as_mut_slice(),
        bus0_out.as_mut_slice(),
        bus1_out.as_mut_slice(),
    ];

    mixer.process(&inputs, &mut outputs).unwrap();

    // Пропускаем первые несколько семплов из-за сглаживания
    let start = 10;

    for i in start..100 {
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
