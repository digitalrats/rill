#![cfg(feature = "lang")]

use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::{Algorithm, Node, Processor};
use rill_core::traits::{ParamMetadata, ParamType, ParamValue};
use rill_core::{ParameterId, RenderContext};
use rill_lang::compile_with;

#[test]
fn param_controls_gain() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>("process = _ * param(\"g\", 1.0);", &reg, 48_000.0).unwrap();
    let input = vec![1.0f32; 4];
    let mut out = vec![0.0f32; 4];

    let g_idx = prog.param_index("g").expect("param 'g' should exist");
    assert!((prog.param(g_idx) - 1.0).abs() < 1e-6);

    prog.process(Some(&input), &mut out).unwrap();
    for &v in &out {
        assert!((v - 1.0).abs() < 1e-5, "expected ~1.0, got {v}");
    }

    prog.set_param(g_idx, 0.25);
    prog.process(Some(&input), &mut out).unwrap();
    for &v in &out {
        assert!((v - 0.25).abs() < 1e-5, "expected ~0.25, got {v}");
    }
}

#[test]
fn dynamic_cutoff_changes_filter() {
    let reg = full_registry::<f32>();
    let sr = 48_000.0;
    let freq = 4000.0;
    let block = {
        let mut v = vec![0.0f32; 128];
        let omega = 2.0 * std::f32::consts::PI * freq / sr;
        for (i, s) in v.iter_mut().enumerate() {
            *s = (omega * i as f32).sin();
        }
        v
    };

    let mut prog_low = compile_with::<f32>(
        "process = _ : lowpass(param(\"cutoff\", 500.0), 0.7);",
        &reg,
        sr,
    )
    .unwrap();
    let mut out_low = vec![0.0f32; block.len()];
    prog_low.process(Some(&block), &mut out_low).unwrap();

    let mut prog_high = compile_with::<f32>(
        "process = _ : lowpass(param(\"cutoff\", 500.0), 0.7);",
        &reg,
        sr,
    )
    .unwrap();
    let ci = prog_high.param_index("cutoff").unwrap();
    prog_high.set_param(ci, 8000.0);
    let mut out_high = vec![0.0f32; block.len()];
    prog_high.process(Some(&block), &mut out_high).unwrap();

    let energy = |v: &[f32]| -> f32 { v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32 };
    let e_low = energy(&out_low);
    let e_high = energy(&out_high);
    assert!(
        e_high > e_low,
        "higher cutoff should pass more energy: low={e_low}, high={e_high}"
    );
}

fn render_through<T: Processor<f32, 64>>(node: &mut T) -> Vec<f32> {
    {
        let inp = node.input_port_mut(0).unwrap().write();
        inp.fill(2.0);
    }
    let ctx = RenderContext::new(0, 64, 48_000.0);
    node.process(&ctx, &[], &[], &[], &[]).unwrap();
    let out = node.output_port(0).unwrap().read();
    out.to_vec()
}

#[test]
fn lang_node_advertises_and_sets_params() {
    use rill_adrift::lang_node::LangNode;

    let mut node = LangNode::<f32, 64>::from_source_with(
        "process = _ * param(\"g\", 1.0);",
        std::sync::Arc::new(full_registry::<f32>()),
        48_000.0,
    )
    .unwrap();
    Node::init(&mut node, 48_000.0);

    let md = Node::metadata(&node);
    let g_param = md
        .parameters
        .iter()
        .find(|p: &&ParamMetadata| p.name == "g")
        .expect("metadata should contain param 'g'");
    assert_eq!(g_param.typ, ParamType::Float);
    assert!((g_param.default.as_f32().unwrap() - 1.0).abs() < 1e-6);

    let out_default = render_through(&mut node);
    for &v in &out_default {
        assert!(
            (v - 2.0).abs() < 1e-4,
            "default gain=1: input 2 → output ~2, got {v}"
        );
    }

    Node::set_parameter(
        &mut node,
        &ParameterId::new("g").unwrap(),
        ParamValue::Float(0.5),
    )
    .unwrap();
    Node::reset(&mut node);

    let out_half = render_through(&mut node);
    for &v in &out_half {
        assert!(
            (v - 1.0).abs() < 1e-4,
            "gain=0.5: input 2 → output ~1, got {v}"
        );
    }
}

#[test]
fn lang_node_set_source_preserves_params_then_recompile() {
    use rill_adrift::lang_node::LangNode;

    let mut node = LangNode::<f32, 64>::from_source_with(
        "process = _ * param(\"g\", 1.0);",
        std::sync::Arc::new(full_registry::<f32>()),
        48_000.0,
    )
    .unwrap();
    Node::init(&mut node, 48_000.0);

    Node::set_parameter(
        &mut node,
        &ParameterId::new("g").unwrap(),
        ParamValue::Float(0.25),
    )
    .unwrap();

    let out = render_through(&mut node);
    for &v in &out {
        assert!(
            (v - 0.5).abs() < 1e-4,
            "gain=0.25: input 2 → output ~0.5, got {v}"
        );
    }

    Node::set_parameter(
        &mut node,
        &ParameterId::new("source").unwrap(),
        ParamValue::String("process = _ * 3.0;".to_string()),
    )
    .unwrap();

    let out2 = render_through(&mut node);
    for &v in &out2 {
        assert!(
            (v - 6.0).abs() < 1e-4,
            "recompile to *3: input 2 → output ~6, got {v}"
        );
    }
}

#[test]
fn lang_node_rejects_unknown_param() {
    use rill_adrift::lang_node::LangNode;

    let mut node = LangNode::<f32, 64>::from_source_with(
        "process = _ * param(\"g\", 1.0);",
        std::sync::Arc::new(full_registry::<f32>()),
        48_000.0,
    )
    .unwrap();

    assert!(Node::set_parameter(
        &mut node,
        &ParameterId::new("nonexistent").unwrap(),
        ParamValue::Float(0.5),
    )
    .is_err());
}
