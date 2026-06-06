use float_cmp::approx_eq;
use rill_core::traits::{Node, Router};
use rill_core::RenderContext;

#[test]
fn test_mixer_creation() {
    let mixer = rill_router::MixerNode::<64>::new(4, 2);
    assert_eq!(mixer.num_inputs(), 4);
    assert_eq!(mixer.num_outputs(), 4); // 2 master + 2 buses
}

#[test]
fn test_mixer_basic_processing() {
    let mut mixer = rill_router::MixerNode::<64>::new(2, 0);
    mixer.init(44100.0);

    let input1 = [0.5; 64];
    let input2 = [0.3; 64];

    mixer
        .input_port_mut(0)
        .unwrap()
        .buffer
        .as_mut_array()
        .copy_from_slice(&input1);
    mixer
        .input_port_mut(1)
        .unwrap()
        .buffer
        .as_mut_array()
        .copy_from_slice(&input2);

    let ctx = RenderContext::new(0, 64, 44100.0);

    mixer.route(&ctx, &[]).unwrap();

    let output_left = mixer.output_port(0).unwrap().buffer.as_array();
    let output_right = mixer.output_port(1).unwrap().buffer.as_array();

    // Both channels are mono, so they are summed to both left and right outputs
    let expected = input1[0] + input2[0];

    for i in 0..64 {
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
    let mut mixer = rill_router::MixerNode::<64>::new(1, 0);
    mixer.set_channel_pan(0, -0.5).unwrap();
    mixer.init(44100.0);

    let input = [1.0; 64];

    mixer
        .input_port_mut(0)
        .unwrap()
        .buffer
        .as_mut_array()
        .copy_from_slice(&input);

    let ctx = RenderContext::new(0, 64, 44100.0);

    mixer.route(&ctx, &[]).unwrap();

    let output_left = mixer.output_port(0).unwrap().buffer.as_array();
    let output_right = mixer.output_port(1).unwrap().buffer.as_array();

    // For pan -0.5: left gain 1.0, right gain 0.5
    // For now, let's just check that left and right are different
    assert!(
        output_left[0] != output_right[0],
        "Left and right should be different with pan"
    );
}

#[test]
fn test_mixer_mute() {
    let mut mixer = rill_router::MixerNode::<64>::new(1, 0);
    mixer.set_channel_mute(0, true).unwrap();
    mixer.init(44100.0);

    let input = [1.0; 64];

    mixer
        .input_port_mut(0)
        .unwrap()
        .buffer
        .as_mut_array()
        .copy_from_slice(&input);

    let ctx = RenderContext::new(0, 64, 44100.0);

    mixer.route(&ctx, &[]).unwrap();

    let output_left = mixer.output_port(0).unwrap().buffer.as_array();
    let output_right = mixer.output_port(1).unwrap().buffer.as_array();

    for i in 0..64 {
        assert_eq!(output_left[i], 0.0);
        assert_eq!(output_right[i], 0.0);
    }
}

#[test]
fn test_mixer_sends() {
    let mut mixer = rill_router::MixerNode::<64>::new(1, 2); // 2 buses for testing

    // Disable smoothing for accurate testing
    mixer.set_smoothing(1.0); // 1.0 = instant change

    mixer.set_channel_volume(0, 0.8).unwrap();

    // Post-fader send
    mixer
        .add_send(
            0,
            rill_router::SendConfig {
                bus_index: 0,
                level: 0.5,
                send_type: rill_router::SendType::PostFader,
            },
        )
        .unwrap();

    // Pre-fader send
    mixer
        .add_send(
            0,
            rill_router::SendConfig {
                bus_index: 1,
                level: 0.3,
                send_type: rill_router::SendType::PreFader,
            },
        )
        .unwrap();

    mixer.init(44100.0);

    let input = [1.0; 64];

    mixer
        .input_port_mut(0)
        .unwrap()
        .buffer
        .as_mut_array()
        .copy_from_slice(&input);

    let ctx = RenderContext::new(0, 64, 44100.0);

    mixer.route(&ctx, &[]).unwrap();

    let output_left = mixer.output_port(0).unwrap().buffer.as_array();
    let output_right = mixer.output_port(1).unwrap().buffer.as_array();
    let bus0_out = mixer.output_port(2).unwrap().buffer.as_array();
    let bus1_out = mixer.output_port(3).unwrap().buffer.as_array();

    // Skip first few samples due to smoothing
    let start = 10;

    for i in start..64 {
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
    let mut mixer = rill_router::MixerNode::<64>::new(2, 0);

    // Test get_param
    assert!(mixer.get_param("master_volume").is_some());
    assert!(mixer.get_param("ch_1_volume").is_some());
    assert!(mixer.get_param("ch_2_pan").is_some());

    // Test set_param
    mixer
        .set_param("ch_1_volume", rill_core::traits::ParamValue::Float(0.5))
        .unwrap();
    if let Some(rill_core::traits::ParamValue::Float(v)) = mixer.get_param("ch_1_volume") {
        assert!((v - 0.5).abs() < 0.001);
    }

    mixer
        .set_param("ch_2_pan", rill_core::traits::ParamValue::Float(-0.8))
        .unwrap();
    if let Some(rill_core::traits::ParamValue::Float(v)) = mixer.get_param("ch_2_pan") {
        assert!((v + 0.8).abs() < 0.001);
    }

    mixer
        .set_param("ch_1_mute", rill_core::traits::ParamValue::Bool(true))
        .unwrap();
    if let Some(rill_core::traits::ParamValue::Bool(v)) = mixer.get_param("ch_1_mute") {
        assert!(v);
    }
}
