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

#[test]
fn foreign_fn_in_brace_where_block_parses() {
    // Regression: `parse_foreign_def` must leave the trailing `;` for
    // `parse_where_block` (brace style) to consume — double-eating used to
    // fail with "expected Semi, found Ident(x)".
    let src = r#"
        main = x where { foreign fn biquad : FixedBuffer f32 -> FixedBuffer f32; x = 1.0; }
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let main = prog.main_def().expect("main def");
    assert!(main.where_defs().iter().any(|d| matches!(
        d,
        rill_lang::ast::Def::Foreign { name, .. } if name == "biquad"
    )));
}

#[test]
fn foreign_fn_declaration_parses() {
    // `foreign fn name : TypeExpr;` — a carried signal signature. Parsing alone
    // is the bar here; resolution lands in Task 4.
    let src = r#"
        foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
        main = 1.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let foreign = prog
        .defs
        .iter()
        .find_map(|d| match d {
            rill_lang::ast::Def::Foreign { name, sig, .. } if name == "biquad" => Some(sig),
            _ => None,
        })
        .expect("foreign fn biquad not found");
    // Pin the flat-curried signature shape: `FixedBuffer f32 -> Float -> Float
    // -> Float -> FixedBuffer f32`.
    assert_eq!(
        foreign,
        &rill_lang::ast::TypeExpr::TFunc(
            vec![
                rill_lang::ast::TypeExpr::TApp(
                    "FixedBuffer".into(),
                    vec![rill_lang::ast::TypeExpr::TName("f32".into())]
                ),
                rill_lang::ast::TypeExpr::TName("Float".into()),
                rill_lang::ast::TypeExpr::TName("Float".into()),
                rill_lang::ast::TypeExpr::TName("Float".into()),
            ],
            Box::new(rill_lang::ast::TypeExpr::TApp(
                "FixedBuffer".into(),
                vec![rill_lang::ast::TypeExpr::TName("f32".into())]
            ))
        )
    );
}

#[test]
fn foreign_sig_describes_signal_and_scalar_params() {
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    // `FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32`
    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Float".into())]),
            TypeExpr::TName("Float".into()),
            TypeExpr::TName("Float".into()),
            TypeExpr::TName("Float".into()),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("Float".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr(&te).expect("FFI sig");
    assert_eq!(sig.params.len(), 4);
    assert!(matches!(sig.params[0], FfiParam::Signal));
    assert!(matches!(sig.params[1], FfiParam::Scalar));
    assert_eq!(sig.signal_outs, 1);
}

#[test]
fn foreign_sig_from_parser_flat_curried_form() {
    // The parser emits a FLAT `TFunc(args: Vec, ret)` for carried arrows
    // (`a -> b -> c -> r` → `TFunc([a, b, c], r)`, not nested one-arg-at-a-time).
    // Feed the real parser output into the descriptor to pin that compatibility.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let src = "foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32; main = 1.0;";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let sig = prog
        .defs
        .iter()
        .find_map(|d| match d {
            rill_lang::ast::Def::Foreign { sig, .. } => Some(sig),
            _ => None,
        })
        .expect("foreign fn biquad not found");
    let ffi = ffi_sig_from_typeexpr(sig).expect("FFI sig from parser output");
    assert_eq!(ffi.params.len(), 4);
    assert!(matches!(ffi.params[0], FfiParam::Signal));
    assert!(matches!(ffi.params[1], FfiParam::Scalar));
    assert_eq!(ffi.signal_outs, 1);
}
