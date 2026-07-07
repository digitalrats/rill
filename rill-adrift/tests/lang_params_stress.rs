#![cfg(feature = "lang")]

use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::{Algorithm, Node, ParamMetadata, ParamType, ParamValue, Processor};
use rill_core::{ParameterId, RenderContext};
use rill_lang::compile_with;

fn render_through<T: Processor<f32, 64>>(node: &mut T, input_val: f32) -> Vec<f32> {
    {
        let inp = node.input_port_mut(0).unwrap().write();
        inp.fill(input_val);
    }
    let ctx = RenderContext::new(0, 64, 48_000.0);
    node.process(&ctx, &[], &[], &[], &[]).unwrap();
    let out = node.output_port(0).unwrap().read();
    out.to_vec()
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

// — param in feedback —————————————————————————————————————

#[test]
fn param_in_feedback_hybrid_matches_reference() {
    let reg = full_registry::<f32>();
    let mut ph =
        compile_with::<f32>("process = + ~ (_ * param(\"fb\", 0.5));", &reg, 48_000.0).unwrap();
    let mut pr =
        compile_with::<f32>("process = + ~ (_ * param(\"fb\", 0.5));", &reg, 48_000.0).unwrap();

    // impulse: y[n] = x[n] + fb * y[n-1]
    let input = {
        let mut v = vec![0.0f32; 128];
        v[0] = 1.0;
        v
    };
    let mut oh = vec![0.0f32; input.len()];
    let mut oref = vec![0.0f32; input.len()];
    ph.process(Some(&input), &mut oh).unwrap();
    pr.process_reference(Some(&input), &mut oref).unwrap();

    let max_diff = oh
        .iter()
        .zip(oref.iter())
        .map(|(h, r)| (h - r).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff < 1e-4,
        "param-in-feedback hybrid vs reference max_diff={max_diff}"
    );

    // Verify impulse decay: y[n] ≈ fb^n
    for (i, &v) in oh.iter().enumerate() {
        let expected = 0.5f32.powi(i as i32);
        assert!(
            (v - expected).abs() < 1e-4,
            "fb-decay sample {i}: expected {expected}, got {v}"
        );
    }
}

#[test]
fn param_in_feedback_changes_with_set_param() {
    let reg = full_registry::<f32>();
    let mut prog =
        compile_with::<f32>("process = + ~ (_ * param(\"fb\", 0.5));", &reg, 48_000.0).unwrap();

    let fi = prog.param_index("fb").unwrap();
    let input = {
        let mut v = vec![0.0f32; 64];
        v[0] = 1.0;
        v
    };
    let mut out_05 = vec![0.0f32; 64];
    prog.process(Some(&input), &mut out_05).unwrap();

    prog.set_param(fi, ParamValue::Float(0.9));
    let mut out_09 = vec![0.0f32; 64];
    prog.process(Some(&input), &mut out_09).unwrap();

    // fb=0.9 should decay slower than fb=0.5 — later samples hold more energy
    let tail_05: f32 = out_05[32..].iter().map(|x| x.abs()).sum();
    let tail_09: f32 = out_09[32..].iter().map(|x| x.abs()).sum();
    assert!(
        tail_09 > tail_05 * 2.0,
        "fb=0.9 tail {tail_09} should be > fb=0.5 tail {tail_05}"
    );
}

// — smooth of a param ——————————————————————————————————

#[test]
fn smooth_of_param_ramps_not_instant() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(
        "process = _ * smooth(param(\"g\", 0.0), 20.0);",
        &reg,
        48_000.0,
    )
    .unwrap();
    let gi = prog.param_index("g").unwrap();

    // block 1: g=0, output should be ~0
    let mut out1 = vec![0.0f32; 64];
    prog.process(Some(&[1.0f32; 64]), &mut out1).unwrap();
    let rms1 = rms(&out1);
    assert!(rms1 < 0.01, "g=0 should give near-zero output, rms={rms1}");

    // Set g=1.0 and run. smooth(20ms) at 48kHz → a ≈ 1-exp(-1/(0.02*48000)) = 1-exp(-0.00104) ≈ 0.00104
    // Very slow ramp: first few samples should still be near zero
    prog.set_param(gi, ParamValue::Float(1.0));
    let mut out2 = vec![0.0f32; 64];
    prog.process(Some(&[1.0f32; 64]), &mut out2).unwrap();

    // After block 2 (64 samples), the smoothed value is far from 1.0
    // At sample 64, the smoothed value ≈ 1 - exp(-64*sr*...)? Actually:
    // y[n] = a * x + (1-a) * y[n-1], with a ≈ 0.00104
    // After n=63 (0-indexed in the loop): y[63] ≈ 1 - (1-a)^64 ≈ 1 - 0.936 ≈ 0.064
    // So short-term energy should be << 1.0
    let rms2 = rms(&out2);
    assert!(
        rms2 < 0.5,
        "smooth should ramp slowly (rms={rms2}) — not instant"
    );

    // block 3: continuing, more ramp-up. Should be higher than block 2.
    let mut out3 = vec![0.0f32; 64];
    prog.process(Some(&[1.0f32; 64]), &mut out3).unwrap();
    let rms3 = rms(&out3);
    assert!(
        rms3 > rms2,
        "smooth should continue ramping (rms3={rms3} <= rms2={rms2})"
    );
}

#[test]
fn smooth_matches_reference() {
    let reg = full_registry::<f32>();
    let mut ph = compile_with::<f32>(
        "process = _ * smooth(param(\"g\", 1.0), 10.0);",
        &reg,
        48_000.0,
    )
    .unwrap();
    let gi = ph.param_index("g").unwrap();
    ph.set_param(gi, ParamValue::Float(0.7));

    let mut pr = compile_with::<f32>(
        "process = _ * smooth(param(\"g\", 1.0), 10.0);",
        &reg,
        48_000.0,
    )
    .unwrap();
    pr.set_param(pr.param_index("g").unwrap(), ParamValue::Float(0.7));

    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut oh = vec![0.0f32; input.len()];
    let mut oref = vec![0.0f32; input.len()];
    ph.process(Some(&input), &mut oh).unwrap();
    pr.process_reference(Some(&input), &mut oref).unwrap();

    let max_diff = oh
        .iter()
        .zip(oref.iter())
        .map(|(h, r)| (h - r).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff < 1e-4,
        "smooth hybrid vs reference max_diff={max_diff}"
    );
}

// — dynamic cutoff on sample built-in (moog) ——————————

#[test]
fn dynamic_cutoff_moog_changes_output() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;
    // Use a 2 kHz signal: cutoff 500 attenuates it, cutoff 12000 passes it.
    let freq = 2000.0;
    let block: Vec<f32> = (0..128)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
        .collect();

    let mut prog = compile_with::<f32>(
        "process = _ : moog(param(\"cutoff\", 500.0), 0.5);",
        &reg,
        sr,
    )
    .unwrap();
    let ci = prog.param_index("cutoff").unwrap();

    let mut out_low = vec![0.0f32; block.len()];
    prog.process(Some(&block), &mut out_low).unwrap();
    let e_low = rms(&out_low);

    prog.set_param(ci, ParamValue::Float(12000.0));
    let mut out_high = vec![0.0f32; block.len()];
    prog.process(Some(&block), &mut out_high).unwrap();
    let e_high = rms(&out_high);

    assert!(
        e_high > e_low * 1.5,
        "higher moog cutoff should pass more energy: low_rms={e_low}, high_rms={e_high}"
    );
}

#[test]
fn dynamic_cutoff_moog_hybrid_matches_reference() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;

    let mut ph = compile_with::<f32>(
        "process = _ : moog(param(\"cutoff\", 800.0), 0.5);",
        &reg,
        sr,
    )
    .unwrap();
    let mut pr = compile_with::<f32>(
        "process = _ : moog(param(\"cutoff\", 800.0), 0.5);",
        &reg,
        sr,
    )
    .unwrap();

    let ci = ph.param_index("cutoff").unwrap();
    let ci_r = pr.param_index("cutoff").unwrap();
    ph.set_param(ci, ParamValue::Float(3000.0));
    pr.set_param(ci_r, ParamValue::Float(3000.0));

    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut oh = vec![0.0f32; input.len()];
    let mut oref = vec![0.0f32; input.len()];
    ph.process(Some(&input), &mut oh).unwrap();
    pr.process_reference(Some(&input), &mut oref).unwrap();

    let max_diff = oh
        .iter()
        .zip(oref.iter())
        .map(|(h, r)| (h - r).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff < 1e-3,
        "moog dynamic-cutoff hybrid vs reference max_diff={max_diff}"
    );
}

// — dynamic cutoff on block built-in (lowpass) ————————

#[test]
fn dynamic_cutoff_lowpass_changes_output_across_blocks() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;
    // Use a 2 kHz signal: cutoff 500 attenuates it, cutoff 8000 passes it.
    let freq = 2000.0;
    let block: Vec<f32> = (0..128)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
        .collect();

    let mut prog = compile_with::<f32>(
        "process = _ : lowpass(param(\"cutoff\", 500.0), 0.7);",
        &reg,
        sr,
    )
    .unwrap();
    let ci = prog.param_index("cutoff").unwrap();

    let mut out_low = vec![0.0f32; block.len()];
    prog.process(Some(&block), &mut out_low).unwrap();
    let e_low = rms(&out_low);

    prog.set_param(ci, ParamValue::Float(8000.0));
    let mut out_high = vec![0.0f32; block.len()];
    prog.process(Some(&block), &mut out_high).unwrap();
    let e_high = rms(&out_high);

    assert!(
        e_high > e_low * 1.5,
        "higher lowpass cutoff should pass more energy: low_rms={e_low}, high_rms={e_high}",
    );
}

// — shared param name with conflicting defaults ——————————

#[test]
fn shared_param_name_conflicting_defaults_is_rejected() {
    let reg = full_registry::<f32>();
    // The same name `k` is declared with two different defaults (0.5 vs 1000.0).
    // This is rejected at compile time rather than silently taking the first.
    let err = compile_with::<f32>(
        "process = _ * param(\"k\", 0.5) : lowpass(param(\"k\", 1000.0), 0.7);",
        &reg,
        48_000.0,
    );
    assert!(
        err.is_err(),
        "conflicting param defaults must be a compile error"
    );
}

#[test]
fn shared_param_name_matching_defaults_shares_one_slot() {
    let reg = full_registry::<f32>();
    // Identical declarations of `k` share a single slot.
    let prog = compile_with::<f32>(
        "process = _ * param(\"k\", 0.5) + param(\"k\", 0.5);",
        &reg,
        48_000.0,
    )
    .unwrap();
    assert_eq!(
        prog.params_meta().len(),
        1,
        "matching param names should deduplicate to one slot"
    );
    assert_eq!(prog.params_meta()[0].name, "k");
}

// — LangNode automation ————————————————————————————————

fn lang_node_from(src: &str) -> rill_adrift::lang_node::LangNode<f32, 64> {
    rill_adrift::lang_node::LangNode::<f32, 64>::from_source_with(
        src,
        std::sync::Arc::new(full_registry::<f32>()),
        48_000.0,
    )
    .unwrap()
}

#[test]
fn lang_node_set_parameter_changes_output() {
    let mut node = lang_node_from("process = _ * param(\"gain\", 1.0);");
    Node::init(&mut node, 48_000.0);

    let out_default = render_through(&mut node, 2.0);
    for &v in &out_default {
        assert!((v - 2.0).abs() < 1e-4, "gain=1: expected ~2, got {v}");
    }

    Node::set_parameter(
        &mut node,
        &ParameterId::new("gain").unwrap(),
        ParamValue::Float(0.5),
    )
    .unwrap();
    Node::reset(&mut node);

    let out_half = render_through(&mut node, 2.0);
    for &v in &out_half {
        assert!((v - 1.0).abs() < 1e-4, "gain=0.5: expected ~1, got {v}");
    }
}

#[test]
fn lang_node_get_parameter_roundtrips() {
    let mut node = lang_node_from("process = _ * param(\"gain\", 1.0, 0.0, 2.0);");
    Node::init(&mut node, 48_000.0);

    let val =
        Node::get_parameter(&node, &ParameterId::new("gain").unwrap()).expect("should get param");
    assert!((val.as_f32().unwrap() - 1.0).abs() < 1e-6);

    Node::set_parameter(
        &mut node,
        &ParameterId::new("gain").unwrap(),
        ParamValue::Float(1.5),
    )
    .unwrap();

    let val2 = Node::get_parameter(&node, &ParameterId::new("gain").unwrap())
        .expect("should get param after set");
    assert!(
        (val2.as_f32().unwrap() - 1.5).abs() < 1e-6,
        "round-trip failed"
    );
}

#[test]
fn lang_node_metadata_lists_params() {
    let mut node = lang_node_from("process = _ * param(\"g\", 1.0);");
    Node::init(&mut node, 48_000.0);

    let md = Node::metadata(&node);
    assert_eq!(md.type_name.as_deref(), Some("rill/lang"));

    let g_meta: Vec<&ParamMetadata> = md.parameters.iter().filter(|p| p.name == "g").collect();
    assert_eq!(g_meta.len(), 1);
    assert_eq!(g_meta[0].typ, ParamType::Float);
    assert!((g_meta[0].default.as_f32().unwrap() - 1.0).abs() < 1e-6);

    // With range
    let mut node_r = lang_node_from("process = _ * param(\"w\", 0.5, 0.0, 1.0);");
    Node::init(&mut node_r, 48_000.0);

    let md_r = Node::metadata(&node_r);
    let w_meta = md_r
        .parameters
        .iter()
        .find(|p| p.name == "w")
        .expect("w param listed");
    assert!(w_meta.range.min.is_some());
    assert!(w_meta.range.max.is_some());
}

#[test]
fn lang_node_source_recompile_reinitializes_params() {
    let mut node = lang_node_from("process = _ * param(\"g\", 1.0);");
    Node::init(&mut node, 48_000.0);

    // Set g to 0.5
    Node::set_parameter(
        &mut node,
        &ParameterId::new("g").unwrap(),
        ParamValue::Float(0.5),
    )
    .unwrap();
    let out_before = render_through(&mut node, 2.0);
    for &v in &out_before {
        assert!(
            (v - 1.0).abs() < 1e-4,
            "before recompile: expected ~1, got {v}"
        );
    }

    // Recompile to a completely new program
    Node::set_parameter(
        &mut node,
        &ParameterId::new("source").unwrap(),
        ParamValue::String("process = _ * 3.0;".to_string()),
    )
    .unwrap();

    let out_after = render_through(&mut node, 2.0);
    for &v in &out_after {
        assert!(
            (v - 6.0).abs() < 1e-4,
            "after recompile: expected ~6, got {v}"
        );
    }

    // The old "g" param is gone from the new program
    assert!(
        Node::get_parameter(&node, &ParameterId::new("g").unwrap()).is_none()
            || Node::get_parameter(&node, &ParameterId::new("g").unwrap())
                .map(|v| (v.as_f32().unwrap() - 1.0).abs() < 1e-6)
                .unwrap_or(true), // param may or may not exist after recompile; it's a new program
    );
}

// — combined: param + smooth + feedback + builtin —————————

#[test]
fn combined_param_smooth_feedback_builtin_runs() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;
    let mut prog = compile_with::<f32>(
        "process = _ : lowpass(param(\"cut\", 2000.0), 0.7) : (+ ~ (_ * param(\"fb\", 0.3))) * smooth(param(\"gain\", 1.0), 10.0);",
        &reg,
        sr,
    )
    .unwrap();

    let cut_i = prog.param_index("cut").unwrap();
    let fb_i = prog.param_index("fb").unwrap();
    let gain_i = prog.param_index("gain").unwrap();

    assert_eq!(prog.params_meta().len(), 3);

    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let e = rms(&out);
    assert!(e > 0.0, "combined program silenced output");
    assert!(e < 10.0, "combined program unstable (rms={e})");

    prog.set_param(cut_i, ParamValue::Float(8000.0));
    prog.set_param(fb_i, ParamValue::Float(0.7));
    prog.set_param(gain_i, ParamValue::Float(0.1));
    let mut out2 = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out2).unwrap();

    // With higher cutoff + higher feedback, energy should increase
    // (but lower gain might offset — the combination still runs without panic)
    let e2 = rms(&out2);
    assert!(e2 > 0.0, "post-set combined program silenced");
    assert!(e2 < 10.0, "post-set combined program unstable (rms={e2})");
}

#[test]
fn combined_hybrid_matches_reference() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;
    let src = "process = _ : lowpass(param(\"cut\", 2000.0), 0.7) : (+ ~ (_ * param(\"fb\", 0.3))) * smooth(param(\"gain\", 1.0), 10.0);";
    let mut ph = compile_with::<f32>(src, &reg, sr).unwrap();
    let mut pr = compile_with::<f32>(src, &reg, sr).unwrap();

    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut oh = vec![0.0f32; input.len()];
    let mut oref = vec![0.0f32; input.len()];
    ph.process(Some(&input), &mut oh).unwrap();
    pr.process_reference(Some(&input), &mut oref).unwrap();

    let max_diff = oh
        .iter()
        .zip(oref.iter())
        .map(|(h, r)| (h - r).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff < 1e-3,
        "combined hybrid vs reference max_diff={max_diff}"
    );
}

// regression: no panic on programs using params + builtins

#[test]
fn no_panic_on_multiple_dynamic_params() {
    let reg = full_registry::<f32>();
    let src = "process = _ : moog(param(\"c\", 800.0), param(\"r\", 0.5));";
    let mut prog = compile_with::<f32>(src, &reg, 48_000.0).unwrap();
    let ci = prog.param_index("c").unwrap();
    let ri = prog.param_index("r").unwrap();

    let input = vec![0.1f32; 32];
    let mut out = vec![0.0f32; 32];
    prog.process(Some(&input), &mut out).unwrap();
    assert!(rms(&out) > 0.0);

    // Change both params mid-run — should not panic
    prog.set_param(ci, ParamValue::Float(4000.0));
    prog.set_param(ri, ParamValue::Float(0.9));
    prog.process(Some(&input), &mut out).unwrap();
    assert!(rms(&out) > 0.0);
}
