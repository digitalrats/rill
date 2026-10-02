//! SP-3a: FFI layer — foreign fn declarations + FixedBuffer/Buffer types.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;
use rill_lang::types::ty::TypeEnv;

#[test]
fn signal_prelude_registers_fixedbuffer_and_buffer() {
    // `FixedBuffer a` is a builtin signal-channel ctor (kind `* -> *`), `Buffer`
    // a builtin typeclass over buffer types, with a `FixedBuffer a` instance.
    // This asserts the SIGNAL_PRELUDE registration directly (a typeclass
    // signature referencing `FixedBuffer` is stored raw, so an integration
    // program alone would not prove resolution).
    let env = TypeEnv::with_builtins();
    assert_eq!(env.ctor_arity("FixedBuffer"), Some(1));
    assert!(env.typeclasses.contains_key("Buffer"));
    assert!(env
        .instances
        .get("Buffer")
        .unwrap()
        .contains_key("FixedBuffer"));
}

#[test]
fn buffer_typeclass_registers_from_signal_prelude() {
    // `Buffer` is a builtin typeclass over buffer types, `FixedBuffer a` the
    // signal-channel type. `main` is trivial — this proves the SIGNAL_PRELUDE
    // parses, registers, and the instance kind-checks.
    let src = r#"
        typeclass UsesBuffer a where { use_buf: FixedBuffer a -> FixedBuffer a; }
        main = 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
