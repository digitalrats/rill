// rill-adrift/tests/fft_integration.rs
//! Integration tests for rill-fft graph nodes via rill-adrift.

#[cfg(feature = "fft")]
use rill_adrift::fft::nodes::convolver_node::ConvolverNode;
#[cfg(feature = "fft")]
use rill_adrift::registration;
#[cfg(feature = "fft")]
use rill_adrift::rill_core::traits::{Node, Processor};
#[cfg(feature = "fft")]
use rill_adrift::rill_core::{ParamValue, ParameterId, RenderContext};

const BUF: usize = 64;
const SR: f32 = 44100.0;

#[cfg(feature = "fft")]
#[test]
fn test_convolver_node_via_factory() {
    use rill_adrift::rill_core::traits::{NodeVariant, Params};
    use rill_adrift::rill_graph::NodeFactory;

    let mut factory = NodeFactory::<f32, BUF>::new();
    registration::register_all_nodes::<BUF>(&mut factory);

    let result = factory.construct(
        "rill/convolver",
        rill_adrift::rill_core::NodeId(0),
        &Params::new(SR).with("ir_len", ParamValue::Float(1024.0)),
    );
    assert!(result.is_ok(), "Failed to construct convolver node");

    let variant = result.unwrap();
    assert!(matches!(variant, NodeVariant::Processor(_)));
}

#[cfg(feature = "fft")]
#[test]
fn test_convolver_node_passthrough() {
    let mut node = ConvolverNode::<f32, BUF>::new(64, SR);
    Node::init(&mut node, SR);

    // Feed a unit impulse
    let mut signal = [0.0f32; BUF];
    signal[0] = 1.0;
    node.input_port_mut(0)
        .unwrap()
        .write()
        .copy_from_slice(&signal);

    let ctx = RenderContext::new(0, 0, SR);
    node.process(&ctx, &[], &[], &[], &[]).unwrap();

    let output = node.output_port(0).unwrap().read();
    // No IR loaded → output should match input (passthrough)
    for (i, o) in signal.iter().zip(output.iter()) {
        assert!((i - o).abs() < 1e-5, "expected {i}, got {o}");
    }
}

#[cfg(feature = "fft")]
#[test]
fn test_convolver_node_with_ir() {
    let ir = [1.0f32, 0.5, 0.25, 0.125];

    let mut node = ConvolverNode::<f32, BUF>::new(ir.len(), SR);
    Node::init(&mut node, SR);
    node.set_ir(&ir);
    node.set_mix(1.0);

    let mut signal = [0.0f32; BUF];
    signal[0] = 1.0; // unit impulse
    node.input_port_mut(0)
        .unwrap()
        .write()
        .copy_from_slice(&signal);

    let ctx = RenderContext::new(0, 0, SR);
    node.process(&ctx, &[], &[], &[], &[]).unwrap();

    let output = node.output_port(0).unwrap().read();
    // Output of unit impulse should match IR (within FFT precision)
    for (i, expected) in ir.iter().enumerate() {
        assert!(
            (output[i] - expected).abs() < 0.01,
            "IR tap {i}: expected {expected}, got {}",
            output[i]
        );
    }
}

#[cfg(feature = "fft")]
#[test]
fn test_convolver_node_parameters() {
    use rill_adrift::rill_core::traits::Node;

    let mut node = ConvolverNode::<f32, BUF>::new(1024, SR);
    Node::init(&mut node, SR);

    assert!(!node.ir_loaded());

    let ir_gain = ParameterId::new("ir_gain").unwrap();
    let mix = ParameterId::new("mix").unwrap();

    assert_eq!(node.get_parameter(&ir_gain), Some(ParamValue::Float(1.0)));
    assert_eq!(node.get_parameter(&mix), Some(ParamValue::Float(1.0)));

    node.set_parameter(&ir_gain, ParamValue::Float(2.0))
        .unwrap();
    assert_eq!(node.get_parameter(&ir_gain), Some(ParamValue::Float(2.0)));

    node.set_parameter(&mix, ParamValue::Float(0.5)).unwrap();
    assert_eq!(node.get_parameter(&mix), Some(ParamValue::Float(0.5)));
}

#[cfg(all(feature = "fft", feature = "lang"))]
#[test]
fn test_spectralgate_builtin_in_registry() {
    use rill_adrift::lang_builtins::full_registry;

    let reg = full_registry::<f32>();
    let entry = reg.get("spectralgate");
    assert!(entry.is_some(), "spectralgate builtin not registered");
    let sig = &entry.unwrap().sig;
    assert_eq!(sig.signal_ins(), 1);
    assert_eq!(sig.signal_outs, 1);
    assert_eq!(sig.params.len(), 3);
}

#[cfg(all(feature = "fft", feature = "lang"))]
#[test]
fn test_spectraldelay_builtin_in_registry() {
    use rill_adrift::lang_builtins::full_registry;

    let reg = full_registry::<f32>();
    let entry = reg.get("spectraldelay");
    assert!(entry.is_some(), "spectraldelay builtin not registered");
    let sig = &entry.unwrap().sig;
    assert_eq!(sig.signal_ins(), 1);
    assert_eq!(sig.signal_outs, 1);
    assert_eq!(sig.params.len(), 3);
}

#[cfg(all(feature = "fft", feature = "lang"))]
#[test]
fn test_fft_builtins_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    // Compile a program using spectralgate
    let src = "main = _ : spectralgate 0.01 0.0";
    let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile should succeed");

    let input = [0.5f32; BUF];
    let mut output = [0.0f32; BUF];
    Algorithm::process(&mut prog, Some(&input), &mut output).unwrap();

    // Output should be finite
    for o in output.iter() {
        assert!(o.is_finite());
    }
}

// ============================================================================
// Complex built-in DSL tests
// ============================================================================

#[cfg(feature = "lang")]
#[test]
fn test_complex_builtins_in_registry() {
    use rill_adrift::lang_builtins::full_registry;

    let reg = full_registry::<f32>();

    for name in &["complex", "conj", "re", "im", "norm", "arg", "cmul", "cadd"] {
        let entry = reg.get(name);
        assert!(entry.is_some(), "complex builtin '{name}' not registered");
    }
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_gen_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    // complex → extract re to satisfy process arity 1 requirement
    let src = "main = complex 3.0 4.0 : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();

    assert!(
        (output[0] - 3.0).abs() < 1e-4,
        "re should be 3.0, got {}",
        output[0]
    );
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_conj_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = complex 3.0 4.0 : conj : im";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();

    assert!(
        (output[0] + 4.0).abs() < 1e-4,
        "conj.im should be -4.0, got {}",
        output[0]
    );
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_re_extract() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = complex 3.0 4.0 : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();
    assert!((output[0] - 3.0).abs() < 1e-4);
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_im_extract() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = complex 5.0 7.0 : im";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();
    assert!((output[0] - 7.0).abs() < 1e-4);
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_norm_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = complex 3.0 4.0 : norm";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();

    assert!(
        (output[0] - 5.0).abs() < 1e-2,
        "norm(3+4i) = 5, got {}",
        output[0]
    );
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_cmul_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    // cmul → re: (1+0i)*(2+3i) = 2+3i → extract re=2
    let src = "main = complex 1.0 0.0 , complex 2.0 3.0 : cmul : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();

    assert!(
        (output[0] - 2.0).abs() < 1e-3,
        "re should be 2.0, got {}",
        output[0]
    );
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_cadd_compile_and_run() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    // cadd → re: (1+2i)+(3+4i) = 4+6i → extract re=4
    let src = "main = complex 1.0 2.0 , complex 3.0 4.0 : cadd : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();

    assert!((output[0] - 4.0).abs() < 1e-3, "re should be 4.0");
}

// ============================================================================
// Imaginary literal syntax tests
// ============================================================================

#[cfg(feature = "lang")]
#[test]
fn test_imag_literal_standalone() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = 3i : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();
    assert!((output[0] - 0.0).abs() < 1e-4, "re(3i) should be 0");

    let src2 = "main = 3i : im";
    let mut prog2 = compile_with::<f32>(src2, &reg, 44100.0).expect("compile");
    let mut output2 = [0.0f32; 1];
    Algorithm::process(&mut prog2, None, &mut output2).unwrap();
    assert!((output2[0] - 3.0).abs() < 1e-4, "im(3i) should be 3");
}

#[cfg(feature = "lang")]
#[test]
fn test_complex_literal_syntax() {
    use rill_adrift::rill_core::traits::algorithm::Algorithm;
    use rill_lang::compile_with;

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();

    let src = "main = 1 + 2i : re";
    let mut prog = compile_with::<f32>(src, &reg, 44100.0).expect("compile");
    let mut output = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut output).unwrap();
    assert!((output[0] - 1.0).abs() < 1e-4, "re(1+2i) should be 1");

    let src2 = "main = 1 + 2i : im";
    let mut prog2 = compile_with::<f32>(src2, &reg, 44100.0).expect("compile");
    let mut output2 = [0.0f32; 1];
    Algorithm::process(&mut prog2, None, &mut output2).unwrap();
    assert!((output2[0] - 2.0).abs() < 1e-4, "im(1+2i) should be 2");
}
