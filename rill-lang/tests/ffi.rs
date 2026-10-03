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
    let sig = ffi_sig_from_typeexpr("biquad", &te).expect("FFI sig");
    assert_eq!(sig.params.len(), 4);
    assert!(matches!(sig.params[0], FfiParam::Signal));
    assert!(matches!(sig.params[1], FfiParam::Scalar));
    assert_eq!(sig.signal_outs, 1);
    assert_eq!(sig.param_names, vec!["type", "cutoff", "q"]);
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
    let ffi = ffi_sig_from_typeexpr("biquad", sig).expect("FFI sig from parser output");
    assert_eq!(ffi.params.len(), 4);
    assert!(matches!(ffi.params[0], FfiParam::Signal));
    assert!(matches!(ffi.params[1], FfiParam::Scalar));
    assert_eq!(ffi.signal_outs, 1);
}

#[test]
fn foreign_decl_registers_into_type_env() {
    // A foreign fn declaration registers its signature into TypeEnv::foreign_sigs
    // via the shared `register_decls` path (the same path inference phase 1 uses).
    // This is the compiler state Task 4 (infer foreign names) will consume.
    let src = r#"
        foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
        main = 1.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let mut env = TypeEnv::with_builtins();
    env.register_decls(&prog.defs);
    let sig = env
        .foreign_sigs
        .get("biquad")
        .expect("biquad foreign sig registered");
    assert!(matches!(sig, rill_lang::ast::TypeExpr::TFunc(..)));
}

#[test]
fn foreign_fn_types_as_signal_arrow() {
    // `gain : FixedBuffer f32 -> Float -> FixedBuffer f32` applied to a wire and
    // a constant — types as a 1→1 signal arrow.
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    // `process_ty` is the diagram type of the whole program (main).
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn foreign_fn_wrong_arity_is_error() {
    // `gain` takes 1 signal + 1 scalar = 2 args; a 1-arg call is a type error.
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog) {
        Err(e) => assert!(
            e.to_string().contains("expects at least"),
            "expected an arity error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for wrong arity"),
    }
}

#[test]
fn foreign_fn_lowers_to_callblock() {
    // Lower `gain _ 0.5` and assert the IR contains a CallBlock referencing the
    // `gain` builtin with one signal input and the folded param 0.5.
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = lower_with_cafs(&typed, &rill_lang::builtin::NoSigs, 44100.0, &typed.cafs).unwrap();
    assert!(
        ir.builtins
            .iter()
            .any(|b| b.name == "gain" && b.signal_ins == 1 && b.params == vec![0.5]),
        "expected gain CallBlock with 1 signal in and folded param 0.5, got {:?}",
        ir.builtins
    );
}

#[test]
fn foreign_fn_signal_slot_rejects_value() {
    // A VALUE-channel expression in a Signal slot is a type error (rate check).
    // Note: a bare Float literal is itself a Signal-rate constant signal
    // (0-in generator), so the rejection needs a genuine Value expression —
    // here a String literal.
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain "oops" 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog) {
        Err(e) => assert!(
            e.to_string().contains("must be a signal"),
            "expected a signal-slot rate error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a value in a Signal slot"),
    }
}

#[test]
fn foreign_fn_multi_signal_wires_distinct_inputs() {
    // `cross _ _ 0.5` — two distinct signal inputs must wire to two registers,
    // not both to the first (the k-th Wire in a Signal slot binds the k-th
    // wiring register).
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        foreign fn cross : FixedBuffer f32 -> FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = cross _ _ 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = lower_with_cafs(&typed, &rill_lang::builtin::NoSigs, 44100.0, &typed.cafs).unwrap();
    let cross_idx = ir
        .builtins
        .iter()
        .position(|b| b.name == "cross")
        .expect("cross builtin");
    let bi = &ir.builtins[cross_idx];
    assert_eq!(bi.signal_ins, 2);
    // The CallBlock for cross must reference two distinct src registers.
    let calls: Vec<_> = ir
        .instrs
        .iter()
        .filter(|i| {
            matches!(
                i,
                rill_lang::ir::Instr::CallBlock { instance, .. } if *instance == cross_idx
            )
        })
        .collect();
    assert_eq!(calls.len(), 1);
    let rill_lang::ir::Instr::CallBlock { srcs, .. } = calls[0] else {
        unreachable!()
    };
    assert_eq!(srcs.len(), 2);
    assert_ne!(
        srcs[0], srcs[1],
        "two signal inputs must wire to distinct registers"
    );
}

#[test]
fn foreign_sig_rejects_non_terminal_variadic_signal() {
    // A VariadicSignal (`List (FixedBuffer f32)`) that is not the LAST param
    // is ambiguous: lowering would fold any trailing scalar as a signal, while
    // inference counts a different arity. The descriptor must reject it.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    // `List (FixedBuffer f32) -> Float -> FixedBuffer f32` — variadic mid-signature.
    let te = rill_lang::ast::TypeExpr::TFunc(
        vec![
            rill_lang::ast::TypeExpr::TApp(
                "List".into(),
                vec![rill_lang::ast::TypeExpr::TApp(
                    "FixedBuffer".into(),
                    vec![rill_lang::ast::TypeExpr::TName("Float".into())],
                )],
            ),
            rill_lang::ast::TypeExpr::TName("Float".into()),
        ],
        Box::new(rill_lang::ast::TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![rill_lang::ast::TypeExpr::TName("Float".into())],
        )),
    );
    assert_eq!(ffi_sig_from_typeexpr("bad", &te), None);

    // A terminal VariadicSignal is still accepted (`List (FixedBuffer f32) ->
    // FixedBuffer f32`).
    let ok = rill_lang::ast::TypeExpr::TFunc(
        vec![rill_lang::ast::TypeExpr::TApp(
            "List".into(),
            vec![rill_lang::ast::TypeExpr::TApp(
                "FixedBuffer".into(),
                vec![rill_lang::ast::TypeExpr::TName("Float".into())],
            )],
        )],
        Box::new(rill_lang::ast::TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![rill_lang::ast::TypeExpr::TName("Float".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr("sum_all", &ok).expect("terminal variadic is accepted");
    assert!(matches!(sig.params[0], FfiParam::VariadicSignal));
}

#[test]
fn foreign_fn_non_terminal_variadic_fails_inference() {
    // `foreign fn bad : List (FixedBuffer f32) -> Float -> FixedBuffer f32;`
    // — a non-terminal VariadicSignal — must be rejected: the descriptor
    // returns None and the name does not resolve as a foreign builtin.
    let src = r#"
        foreign fn bad : List (FixedBuffer f32) -> Float -> FixedBuffer f32;
        main = bad _ _ 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    assert!(
        rill_lang::types::infer::infer_program(&prog).is_err(),
        "non-terminal variadic signal must be rejected at inference"
    );
}

#[test]
fn foreign_fn_runs_end_to_end() {
    use rill_core::traits::Algorithm;
    use rill_lang::builtin::BlockBuiltin;
    use rill_lang::ffi::ForeignRegistry;

    // A trivial gain builtin implemented in the test.
    struct Gain(f64);
    impl<T: rill_core::math::Transcendental> Algorithm<T> for Gain {
        fn process(
            &mut self,
            input: Option<&[T]>,
            output: &mut [T],
        ) -> rill_core::ProcessResult<()> {
            let x = input.unwrap_or(&[]);
            for (o, &i) in output.iter_mut().zip(x.iter()) {
                *o = i * T::from_f64(self.0);
            }
            Ok(())
        }
        fn reset(&mut self) {}
    }
    impl<T: rill_core::math::Transcendental> BlockBuiltin<T> for Gain {}

    let mut ffi = ForeignRegistry::<f32>::new();
    ffi.register_block("gain", |params: &[f64], _sr: f32| {
        Box::new(Gain(params.first().copied().unwrap_or(1.0)))
    });

    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [0.5, 1.0, 1.5, 2.0]);
}

#[test]
fn foreign_fn_multichannel_runs_end_to_end() {
    use rill_core::traits::MultichannelAlgorithm;
    use rill_lang::builtin::MultichannelBlockBuiltin;
    use rill_lang::ffi::ForeignRegistry;

    // A 2→1 foreign builtin: sums two input channels.
    struct Add;
    impl<T: rill_core::math::Transcendental> MultichannelAlgorithm<T> for Add {
        fn num_inputs(&self) -> usize {
            2
        }
        fn num_outputs(&self) -> usize {
            1
        }
        fn process(
            &mut self,
            inputs: &[&[T]],
            outputs: &mut [&mut [T]],
        ) -> rill_core::ProcessResult<()> {
            for i in 0..outputs[0].len() {
                outputs[0][i] = inputs[0][i] + inputs[1][i];
            }
            Ok(())
        }
        fn reset(&mut self) {}
    }
    impl<T: rill_core::math::Transcendental> MultichannelBlockBuiltin<T> for Add {}

    let mut ffi = ForeignRegistry::<f32>::new();
    ffi.register_multichannel_block("add", |_ins: usize, _p: &[f64], _sr: f32| Box::new(Add));

    let src = r#"
        foreign fn add : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
        main = add _ _;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]],
        &mut [&mut out],
    )
    .unwrap();
    assert_eq!(out, [11.0, 22.0, 33.0, 44.0]);
}

#[test]
fn complex_ops_build_with_block_factory() {
    // `conj` is a 2→2 builtin registered as a Block factory. Variant selection
    // must follow FACTORY KIND, not signal arity — otherwise
    // `build_multichannel_block` finds no matching variant, `compile_with_ffi`
    // returns CompileError::Unsupported("foreign built-in 'conj' is not
    // registered"), and the `.unwrap()` below panics at build.
    use rill_core::traits::Algorithm;
    use rill_lang::builtin::BlockBuiltin;
    use rill_lang::ffi::ForeignRegistry;

    struct Conj;
    impl<T: rill_core::math::Transcendental> Algorithm<T> for Conj {
        fn process(
            &mut self,
            input: Option<&[T]>,
            output: &mut [T],
        ) -> rill_core::ProcessResult<()> {
            let x = input.unwrap_or(&[]);
            output[..x.len()].copy_from_slice(x);
            for o in output[x.len()..].iter_mut() {
                *o = T::ZERO;
            }
            Ok(())
        }
        fn reset(&mut self) {}
    }
    impl<T: rill_core::math::Transcendental> BlockBuiltin<T> for Conj {}

    let mut ffi = ForeignRegistry::<f32>::new();
    ffi.register_block("conj", |_p: &[f64], _sr: f32| Box::new(Conj));

    let src = r#"
        foreign fn conj : FixedBuffer f32 -> FixedBuffer f32 -> Pair (FixedBuffer f32) (FixedBuffer f32);
        main = conj _ _;
    "#;
    // Must compile and run, not panic at build.
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0, 2.0, 3.0, 4.0], &[5.0, 6.0, 7.0, 8.0]],
        &mut [&mut out],
    )
    .unwrap();
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn foreign_fn_not_registered_is_compile_error() {
    use rill_lang::ffi::ForeignRegistry;

    let ffi = ForeignRegistry::<f32>::new();
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    let err = match rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0) {
        Err(e) => e,
        Ok(_) => panic!("expected a compile error for an unregistered foreign builtin"),
    };
    assert!(
        err.to_string().contains("not registered"),
        "expected a 'not registered' error, got: {err}"
    );
}

#[test]
fn builtin_catalog_is_auto_registered() {
    // `sine` resolves WITHOUT a user-written `foreign fn` declaration — the
    // inline builtin catalog auto-registers the declaration into
    // `TypeEnv::foreign_sigs` (SP-3b Task 3).
    let src = r#"
        main = sine 440.0 1.0 0.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 0);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn builtin_catalog_complex_ops_resolve() {
    // `complex` and `norm` are catalog entries: the combinator style
    // `complex 3.0 4.0 : norm` must infer (complex 0→2, norm bare-ref 2→1).
    let src = r#"
        main = complex 3.0 4.0 : norm;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 0);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn signal_input_builtin_legacy_call_style_sugar() {
    // `_ : onepole 200.0 0.7` — the legacy combinator style must desugar to
    // `onepole _ 200.0 0.7` (positional signal arg), so the FFI declaration
    // types and lowers without rewriting the program.
    let src = r#"
        foreign fn onepole : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
        main = _ : onepole 200.0 0.7;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_binds_both_seq_operands() {
    // `onepole 200.0 0.7 : onepole 1000.0 0.9` — the LEFT operand of `:` is a
    // signal-input builtin in combinator style too; both sides get a Wire.
    let src = r#"
        main = onepole 200.0 0.7 : onepole 1000.0 0.9;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_binds_loop_operand() {
    // `+ ~ onepole 500.0 0.5` — the feedback right operand is an arrow; the
    // sugar binds its missing signal wire.
    let src = r#"
        main = + ~ onepole 500.0 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_binds_par_operand_with_two_signals() {
    // `_, _ : crossfade 0.5` — a 2-signal-in builtin (user-declared FFI) binds
    // BOTH leading wires; the trailing scalar stays a call arg.
    let src = r#"
        foreign fn crossfade : FixedBuffer f32 -> FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = _, _ : crossfade 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 2);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_binds_split_operand() {
    // `onepole 200.0 0.7 <: _ , _` — fan-out: the 1-out builtin distributes
    // over the 2-in Par.
    let src = r#"
        main = onepole 200.0 0.7 <: _ , _;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 2);
}

#[test]
fn combinator_sugar_binds_merge_operand() {
    // `_ , _ :> onepole 200.0 0.7` — fan-in: the 2 outputs sum into the
    // 1-in builtin.
    let src = r#"
        main = _ , _ :> onepole 200.0 0.7;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 2);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_does_not_double_bind_positional_call() {
    // `_ : onepole _ 200.0 0.7` — the signal arg is already supplied
    // positionally; the sugar must leave the call alone.
    let src = r#"
        main = _ : onepole _ 200.0 0.7;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_binds_gain_scalar_only_call() {
    // `_ : gain 0.5` — a foreign with ONE leading signal param and ONE scalar.
    // The supplied arg is exactly the trailing scalar; the sugar prepends the
    // single wire (`gain _ 0.5`), not zero (the supplied arg is NOT a signal).
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = _ : gain 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_rewrites_where_block_defs() {
    // A combinator call inside a `where` definition desugars too (where defs
    // are inferred as their own def group, so the pass must descend).
    let src = r#"
        main = x where { x = _ : onepole 200.0 0.7; }
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_lowers_positionally() {
    // End-to-end: the desugared `onepole _ 200.0 0.7` folds the scalars and
    // wires one signal input through the FFI lowering path.
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        main = _ : onepole 200.0 0.7;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = lower_with_cafs(&typed, &rill_lang::builtin::NoSigs, 44100.0, &typed.cafs).unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "onepole").unwrap();
    assert_eq!(bi.signal_ins, 1);
    assert_eq!(bi.params, vec![200.0, 0.7]);
}

#[test]
fn combinator_sugar_binds_delay_lhs() {
    // `onepole 200.0 0.7 @ 3` — the Delay (`@`) combinator's lhs is a foreign
    // builtin used as an arrow; the sugar must bind its missing signal wire.
    let src = r#"
        foreign fn onepole : FixedBuffer f32 -> Float -> Float -> FixedBuffer f32;
        main = _ : onepole 200.0 0.7 @ 3;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

#[test]
fn builtin_catalog_registers_record_data_types() {
    // The record types the catalog's record-param builtins need (mixer/eq/
    // dry_wet) register as `data` declarations — Task 4's record-param
    // lowering reads them from `TypeEnv::data_types`.
    let env = TypeEnv::with_builtins();
    for name in ["MixerConfig", "EqBand", "EqConfig", "DryWetConfig"] {
        assert!(
            env.data_types.contains_key(name),
            "catalog data type `{name}` must be registered"
        );
    }
    // `EqConfig = { bands: List EqBand }` — the `List EqBand` field must survive
    // the phase-2 value-type conversion as an App of the nested record name.
    match env.data_types.get("EqConfig") {
        Some(rill_lang::types::ty::DataInfo::Record(fields)) => {
            assert_eq!(fields.len(), 1);
            assert!(matches!(
                &fields[0].1,
                rill_lang::types::ty::ValueTy::App(head, args)
                    if head == "List" && args.len() == 1
            ));
        }
        other => panic!("EqConfig must be a record, got {other:?}"),
    }
}

#[test]
fn ffi_sig_param_names_filled_from_catalog_table() {
    // The name table (mirroring legacy `BuiltinSig::param_names`) supplies
    // display names for scalar params in declaration order — reconstruct
    // consumes them to order GraphSpec node params.
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::ffi_sig_from_typeexpr;

    let out = TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("f32".into())]);
    let f = |n: &str| TypeExpr::TName(n.into());
    let fb = |n: &str| TypeExpr::TApp(n.into(), vec![TypeExpr::TName("f32".into())]);

    let sig = ffi_sig_from_typeexpr(
        "sine",
        &TypeExpr::TFunc(
            vec![f("Float"), f("Float"), f("Float")],
            Box::new(out.clone()),
        ),
    )
    .expect("sine FFI sig");
    assert_eq!(sig.param_names, vec!["freq", "amp", "phase"]);

    let sig = ffi_sig_from_typeexpr(
        "biquad",
        &TypeExpr::TFunc(
            vec![
                fb("FixedBuffer"),
                f("Float"),
                f("Float"),
                f("Float"),
                f("Float"),
            ],
            Box::new(out.clone()),
        ),
    )
    .expect("biquad FFI sig");
    assert_eq!(sig.param_names, vec!["type", "cutoff", "q", "gain_db"]);

    let sig = ffi_sig_from_typeexpr(
        "dry_wet",
        &TypeExpr::TFunc(
            vec![fb("FixedBuffer"), fb("FixedBuffer"), f("DryWetConfig")],
            Box::new(out),
        ),
    )
    .expect("dry_wet FFI sig");
    // dry_wet's record config has no scalar display names (legacy param_names
    // was empty) — the record flattens through its schema at lowering.
    assert!(sig.param_names.is_empty());
}

#[test]
fn ffi_sig_param_names_fallback_to_index_for_unknown() {
    // A builtin not in the name table gets index-based names (param0, param1…)
    // so graph reconstruction still orders its scalar params.
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::ffi_sig_from_typeexpr;

    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("f32".into())]),
            TypeExpr::TName("Float".into()),
            TypeExpr::TName("Float".into()),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("f32".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr("not_a_catalog_builtin", &te).unwrap();
    assert_eq!(sig.param_names, vec!["param0", "param1"]);
}

#[test]
fn tape_param_maps_to_resource() {
    // `Tape a` — a shared-buffer handle — is a Resource param (the tape arg is
    // a symbolic `Ref`, wired to the shared buffer at build, not a signal).
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp("Tape".into(), vec![TypeExpr::TName("f32".into())]),
            TypeExpr::TName("Float".into()),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("f32".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr("read_head", &te).unwrap();
    assert!(matches!(sig.params[0], FfiParam::Resource));
    assert!(matches!(sig.params[1], FfiParam::Scalar));
}

#[test]
fn ffi_record_schema_reads_data_types() {
    // `ffi_record_schema` mirrors the legacy RecordSchema from the catalog's
    // `data` declarations: field name + scalar type in declaration order, with
    // the legacy `RecordField::default` values (SP-3b Task 7, follow-up A).
    use rill_lang::types::ffi::{ffi_record_schema, FfiScalar};

    let env = TypeEnv::with_builtins();
    let schema = ffi_record_schema(&env, "MixerConfig").expect("MixerConfig schema");
    assert_eq!(
        schema.fields,
        vec![
            ("buses".to_string(), FfiScalar::Int, Some(0.0)),
            ("master_vol".to_string(), FfiScalar::Float, Some(1.0)),
        ]
    );
    assert!(ffi_record_schema(&env, "NoSuchType").is_none());
}

#[test]
fn record_param_infer_accepts_record_literal() {
    // The FFI infer arm validates a Record param's arg is a record literal of
    // the named data type (previously an unconditional SP-3b error).
    let src = r#"
        foreign fn dry_wet : FixedBuffer f32 -> FixedBuffer f32 -> DryWetConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
        main = dry_wet _ _ { mix: 0.5 };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 2);
    assert_eq!(typed.process_ty.arity_out(), 2);
}

#[test]
fn record_param_flattens_schema_fields() {
    // `dry_wet` takes a DryWetConfig record `{ mix: 0.5 }` → one f64 param.
    let src = r#"
        foreign fn dry_wet : FixedBuffer f32 -> FixedBuffer f32 -> DryWetConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
        main = dry_wet _ _ { mix: 0.5 };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(
        &typed,
        &rill_lang::builtin::NoSigs,
        44100.0,
        &typed.cafs,
    )
    .unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "dry_wet").unwrap();
    assert_eq!(bi.params, vec![0.5]);
}

#[test]
fn record_param_missing_fields_use_zero_defaults() {
    // A partial record fills the omitted schema field with its default (0.0 in
    // the FFI schema — the catalog `data` declaration carries no defaults).
    let src = r#"
        data TestCfg = { a: Float, b: Float };
        foreign fn cfg : TestCfg -> FixedBuffer f32;
        main = cfg { a: 1.5 };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(
        &typed,
        &rill_lang::builtin::NoSigs,
        44100.0,
        &typed.cafs,
    )
    .unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "cfg").unwrap();
    assert_eq!(bi.params, vec![1.5, 0.0]);
}

#[test]
fn record_param_rejects_non_record_arg() {
    let src = r#"
        foreign fn dry_wet : FixedBuffer f32 -> FixedBuffer f32 -> DryWetConfig -> Pair (FixedBuffer f32) (FixedBuffer f32);
        main = dry_wet _ _ 0.5;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog) {
        Err(e) => assert!(
            e.to_string().contains("record literal"),
            "expected a record-literal error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a non-record record param"),
    }
}

#[test]
fn resource_param_wires_tape_ref() {
    // A `Tape f32` param is a Resource: lowering sets the builtin's resource to
    // the symbolic `Ref` name (the build path resolves it in the registry).
    let src = r#"
        foreign fn read_head : Tape f32 -> Float -> FixedBuffer f32;
        main = read_head tape_0 0.1;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(
        &typed,
        &rill_lang::builtin::NoSigs,
        44100.0,
        &typed.cafs,
    )
    .unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "read_head").unwrap();
    assert_eq!(bi.resource.as_deref(), Some("tape_0"));
    assert_eq!(bi.params, vec![0.1]);
}

#[test]
fn resource_param_rejects_non_ref_arg() {
    let src = r#"
        foreign fn read_head : Tape f32 -> Float -> FixedBuffer f32;
        main = read_head 0.5 0.1;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog) {
        Err(e) => assert!(
            e.to_string().contains("symbolic reference"),
            "expected a symbolic-reference error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a non-Ref resource param"),
    }
}

#[test]
fn foreign_variadic_signal_merge_accepts_channels() {
    // `expr_has_variadic_signal` must recognize a foreign sig's VariadicSignal:
    // `_, _ :> sum_all _ _` merges both channels into the variadic input (the
    // rhs is an Apply, the shape reconstruct emits for a variadic builtin).
    let src = r#"
        foreign fn sum_all : List (FixedBuffer f32) -> FixedBuffer f32;
        main = _ , _ :> sum_all _ _;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    assert_eq!(typed.process_ty.arity_in(), 2);
    assert_eq!(typed.process_ty.arity_out(), 1);
}

// --- SP-3b Task 7: record defaults + List EqBand band-list (router migration) ---

#[test]
fn ffi_record_schema_carries_legacy_defaults() {
    // Follow-up A (Task 4 review): the FFI record schema must carry the legacy
    // RecordField defaults (dry_wet mix 0.5, mixer buses 0 / master_vol 1.0) —
    // otherwise a migrated omitted-field default silently becomes 0.0.
    use rill_lang::types::ffi::ffi_record_schema;

    let env = TypeEnv::with_builtins();
    let mixer = ffi_record_schema(&env, "MixerConfig").expect("MixerConfig schema");
    assert_eq!(
        mixer.fields,
        vec![
            (
                "buses".to_string(),
                rill_lang::types::ffi::FfiScalar::Int,
                Some(0.0)
            ),
            (
                "master_vol".to_string(),
                rill_lang::types::ffi::FfiScalar::Float,
                Some(1.0)
            ),
        ]
    );
    let dry_wet = ffi_record_schema(&env, "DryWetConfig").expect("DryWetConfig schema");
    assert_eq!(
        dry_wet.fields,
        vec![(
            "mix".to_string(),
            rill_lang::types::ffi::FfiScalar::Float,
            Some(0.5)
        )]
    );
}

#[test]
fn ffi_record_schema_bandlist_field() {
    // Follow-up B (Task 4 review): `EqConfig = { bands: List EqBand }` — the
    // list-of-record field must be representable in the schema (a `BandList`
    // field carrying the band record type name), not silently dropped.
    use rill_lang::types::ffi::{ffi_record_schema, FfiScalar};

    let env = TypeEnv::with_builtins();
    let eq = ffi_record_schema(&env, "EqConfig").expect("EqConfig schema");
    assert_eq!(eq.fields.len(), 1);
    let (fname, fscalar, fdefault) = &eq.fields[0];
    assert_eq!(*fname, "bands");
    let band_ty: &str = match fscalar {
        rill_lang::types::ffi::FfiScalar::BandList(bt) => bt.as_str(),
        other => panic!("bands field must be a BandList, got {other:?}"),
    };
    assert_eq!(band_ty, "EqBand");
    assert!(fdefault.is_none());
    // The band schema itself flattens with its legacy defaults.
    let band = ffi_record_schema(&env, "EqBand").expect("EqBand schema");
    assert_eq!(
        band.fields,
        vec![
            ("freq".to_string(), FfiScalar::Float, Some(1000.0)),
            ("q".to_string(), FfiScalar::Float, Some(1.0)),
            ("gain_db".to_string(), FfiScalar::Float, Some(0.0)),
            ("band_type".to_string(), FfiScalar::Int, Some(0.0)),
        ]
    );
}

#[test]
fn ffi_sig_accepts_mixer_variadic_record() {
    // Step 2 (option a): `List (FixedBuffer f32) -> MixerConfig -> Pair ...` —
    // a LEADING VariadicSignal followed only by a record param is valid (the
    // variadic is the mixer's signal channels, the record the config scalar).
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp(
                "List".into(),
                vec![TypeExpr::TApp(
                    "FixedBuffer".into(),
                    vec![TypeExpr::TName("f32".into())],
                )],
            ),
            TypeExpr::TName("MixerConfig".into()),
        ],
        Box::new(TypeExpr::TApp(
            "Pair".into(),
            vec![
                TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("f32".into())]),
                TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("f32".into())]),
            ],
        )),
    );
    let sig = ffi_sig_from_typeexpr("mixer", &te).expect("mixer FFI sig");
    assert!(matches!(&sig.params[0], FfiParam::VariadicSignal));
    assert!(matches!(&sig.params[1], FfiParam::Record(ty) if ty == "MixerConfig"));
    assert_eq!(sig.signal_outs, 2);
}

#[test]
fn record_default_ffi_applies() {
    // Follow-up A end-to-end: `dry_wet { }` (empty record) must apply the
    // legacy `mix: 0.5` default — not silently 0.0.
    let src = r#"
        main = _ , _ : dry_wet { };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(
        &typed,
        &rill_lang::builtin::NoSigs,
        44100.0,
        &typed.cafs,
    )
    .unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "dry_wet").unwrap();
    assert_eq!(bi.params, vec![0.5]);
}

#[test]
fn band_list_flattens_band_fields() {
    // Follow-up B end-to-end: `eq_parametric { bands: [ { freq: 1000.0, q: 1.0,
    // gain_db: 0.0, band_type: 0.0 } ] }` flattens each band's fields in schema
    // order into the folded param list.
    let src = r#"
        main = _ : eq_parametric { bands: [ { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 } ] };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(
        &typed,
        &rill_lang::builtin::NoSigs,
        44100.0,
        &typed.cafs,
    )
    .unwrap();
    let bi = ir
        .builtins
        .iter()
        .find(|b| b.name == "eq_parametric")
        .unwrap();
    assert_eq!(bi.params, vec![1000.0, 1.0, 0.0, 0.0]);
}

// --- SP-3b Task 9: rill-core-model FFI E2E (analog_moog) ---

#[cfg(feature = "model")]
#[test]
fn analog_moog_ffi_end_to_end() {
    // `analog_moog` is in the catalog (Task 3b); `register_foreign_model`
    // registers the WDF MoogLadder factory (moved from rill-core-model,
    // SP-3b Task 9), so the combinator call compiles and runs through the
    // FFI path.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_model;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_model(&mut ffi);

    let src = r#"main = _ : analog_moog 500.0 0.7;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 1.0, 1.0, 1.0]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "analog_moog output must be finite, got {out:?}"
    );
}

// --- SP-3b Task 7: rill-router FFI E2E (mixer/eq_parametric/dry_wet) ---

#[test]
fn dry_wet_ffi_end_to_end() {
    // `dry_wet` is in the catalog; `register_foreign_router` registers its
    // factory, so `_ , _ : dry_wet { mix: 0.5 }` compiles and runs through the
    // FFI path. mix 0.5 → output = dry·0.5 + wet·0.5 (both L and R).
    use rill_lang::ffi::ForeignRegistry;

    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);

    let src = r#"
        main = _ , _ : dry_wet { mix: 0.5 };
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let dry = [2.0f32; 4];
    let wet = [4.0f32; 4];
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let inputs: [&[f32]; 2] = [&dry, &wet];
    let mut outputs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut prog, &inputs, &mut outputs).unwrap();
    assert!(
        (l[0] - 3.0).abs() < 1e-5,
        "mix=0.5 must blend dry(2) and wet(4) to 3.0, got l[0]={}",
        l[0]
    );
    assert!(
        (r[0] - 3.0).abs() < 1e-5,
        "both outputs receive the blend, got r[0]={}",
        r[0]
    );
}

#[test]
fn mixer_ffi_end_to_end() {
    // `mixer` (variadic signal + `MixerConfig` record) is in the catalog; the
    // factory is registered via `register_foreign_router`. The 2-in stereo sum
    // runs and yields 2 outs (master_vol 1.0, default channel vol 0.8).
    use rill_lang::ffi::ForeignRegistry;

    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);

    let src = r#"
        main = (_, _) :> mixer { buses: 0, master_vol: 1.0 };
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let ch0 = [1.0f32; 4];
    let ch1 = [2.0f32; 4];
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let inputs: [&[f32]; 2] = [&ch0, &ch1];
    let mut outputs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut prog, &inputs, &mut outputs).unwrap();
    // channel_vols default 0.8 · master_vol 1.0 → (1 + 2)·0.8 = 2.4.
    assert!(
        (l[0] - 2.4).abs() < 1e-5,
        "mixer stereo sum: l[0]={}, expected ~2.4",
        l[0]
    );
    assert!(r.iter().all(|v| v.is_finite()));
}

#[test]
fn mixer_ffi_explicit_wires_not_doubled() {
    // `mixer _ _ { ... }` — the FFI positional style supplies the signal
    // channels as explicit wire call args. The variadic lowering must consume
    // each explicit wire once and then stop (a regression: the combinator-fed
    // while-loop re-pushed already-consumed wires, doubling signal_ins → a
    // 2-channel mix computed as 4 channels, 4.8 instead of 2.4).
    use rill_lang::ffi::ForeignRegistry;

    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);

    let src = r#"
        main = mixer _ _ { buses: 0, master_vol: 1.0 };
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let ch0 = [1.0f32; 4];
    let ch1 = [2.0f32; 4];
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let inputs: [&[f32]; 2] = [&ch0, &ch1];
    let mut outputs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut prog, &inputs, &mut outputs).unwrap();
    assert!(
        (l[0] - 2.4).abs() < 1e-5,
        "mixer explicit-wire stereo sum must be ~2.4, got l[0]={}",
        l[0]
    );
}

#[test]
fn eq_parametric_ffi_end_to_end() {
    // `eq_parametric` with a `bands` list — the BandList flattening (Follow-up
    // B) feeds the factory's per-band params. A unity-gain peak band passes
    // the first sample unchanged (b0 = 1); the run proves the band config
    // reached the factory.
    use rill_lang::ffi::ForeignRegistry;

    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);

    let src = r#"
        main = _ : eq_parametric { bands: [ { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 } ] };
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 4]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "eq_parametric output must be finite, got {out:?}"
    );
    assert!(
        (out[0] - 1.0).abs() < 1e-5,
        "unity-gain peak band: first sample must pass unchanged, got out[0]={}",
        out[0]
    );
}

#[test]
fn foreign_param_names_never_fallback_to_index_for_catalog() {
    // Follow-up C: every catalog builtin with scalar params must have real
    // display names — a missed `foreign_param_names` entry would silently fall
    // back to param0/param1 and corrupt graph reconstruction's param ordering.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let env = TypeEnv::with_builtins();
    let mut scalar_builtins = 0usize;
    for (name, te) in &env.foreign_sigs {
        let Some(sig) = ffi_sig_from_typeexpr(name, te) else {
            continue;
        };
        let n_scalar = sig
            .params
            .iter()
            .filter(|p| matches!(p, FfiParam::Scalar))
            .count();
        if n_scalar == 0 {
            continue;
        }
        scalar_builtins += 1;
        let names = sig.param_names.clone();
        assert_eq!(
            names.len(),
            n_scalar,
            "catalog builtin `{name}` must name every scalar param"
        );
        assert!(
            names.iter().all(|n| !n.starts_with("param")),
            "catalog builtin `{name}` must have real param names, got {names:?}"
        );
    }
    assert!(
        scalar_builtins > 0,
        "catalog must contain scalar-param builtins to guard against drift"
    );
}

#[cfg(feature = "dsp")]
#[test]
fn digital_effects_ffi_end_to_end() {
    // `delay` is in the catalog (Task 3b); `register_foreign_digital_effects`
    // registers its factory, so `_ : delay …` compiles and runs through the FFI
    // path. The algorithm lives in rill-digital-effects (algorithms-only).
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_digital_effects;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_digital_effects(&mut ffi);

    let src = r#"
        main = _ : delay 0.1 0.3 0.5;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    // Fresh delay line: wet = 0, so the first block is the dry path at mix 0.5.
    assert!(
        (out[0] - 0.5).abs() < 1e-6,
        "delay dry path: out[0]={}",
        out[0]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn digital_effects_distortion_runs() {
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_digital_effects;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_digital_effects(&mut ffi);

    let src = r#"
        main = _ : distortion 2.0 1.0;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    // Soft-clip: tanh(2.0) with output gain 1.0.
    let expected = 2.0f32.tanh();
    assert!(
        (out[0] - expected).abs() < 1e-4,
        "distortion soft-clip: out[0]={} expected ~{expected}",
        out[0]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn digital_effects_delay_live_param_reaches_algorithm() {
    // Regression: the FFI wrapper's `set_param` must route a live SetParameter
    // into the algorithm. Empty `BlockBuiltin` impls silently dropped it, so
    // `main t = _ : delay t 0.0 1.0` ran at the clamped minimum forever.
    // `t` is a main λ-param → param_bindings (0, 0); mix = 1.0 makes the
    // output pure wet, so the output equals the delayed tap.
    use rill_core::traits::ParamValue;
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_digital_effects;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_digital_effects(&mut ffi);

    let src = r#"
        main t = _ : delay t 0.0 1.0;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();

    // Default t = 0.0 clamps to 0.01 s → 441 samples at 44.1 kHz.
    let t_idx = prog.param_index("t").expect("param `t`");
    assert_eq!(t_idx, 0);

    // Fill the delay line (3 blocks of 256 = 768 samples > 441).
    let mut out = [0.0f32; 256];
    let ones = [1.0f32; 256];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[&ones], &mut [&mut out]).unwrap();
    }
    // The wet tap 441 samples back reads a written region: pure wet output.
    assert!(
        (out[0] - 1.0).abs() < 1e-6,
        "wet tap at default delay: out[0]={}",
        out[0]
    );

    // Lengthen the delay via SetParameter → the tap moves to a never-written
    // region of the delay line → the pure-wet output drops to 0.0.
    prog.set_param(t_idx, ParamValue::Float(0.3));
    MultichannelAlgorithm::process(&mut prog, &[&ones], &mut [&mut out]).unwrap();
    assert!(
        out[0].abs() < 1e-6,
        "after SetParameter(t, 0.3) the tap must move: out[0]={}",
        out[0]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn digital_effects_limiter_runs() {
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_digital_effects;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_digital_effects(&mut ffi);

    let src = r#"
        main = _ : limiter (-6.0) 0.1;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    // Lookahead warm-up: the limiter passes the first `lookahead_samples` (220)
    // unchanged, so the 4-sample block is a passthrough.
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
}

#[cfg(feature = "dsp")]
#[test]
fn generators_ffi_end_to_end() {
    // `sine` is in the catalog; a registered factory makes it run. The FFI
    // registry holds rill-lang's own generator/integrator wrappers around the
    // rill-core-dsp algorithms (SP-3b Task 6) — the same factories the legacy
    // registry served.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = sine 440.0 1.0 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    // Sine at t=0, amp 1.0, phase 0.0: starts at 0.0 and rises.
    assert_eq!(out[0], 0.0);
    // Sample 1 must follow sin(2π·f·n/sr + phase) — the oscillator's actual
    // formula (BasicOscillator::scalar_sine: phase·2π → sin → ·amp). A silent
    // oscillator (or a phase-agnostic stub) would leave this 0.0.
    let expected = (std::f32::consts::PI * 2.0 * (440.0f32 / 44100.0f32)).sin();
    assert!(
        (out[1] - expected).abs() < 1e-3,
        "sine sample 1 must oscillate at the requested freq/amp: out[1]={}, expected ~{expected}",
        out[1]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn saw_ffi_end_to_end() {
    // `saw` is a 0-in catalog builtin; the registered factory must run it. A
    // band-limited saw starts at phase 0 → raw `2·0 - 1 = -1.0` (amp 1.0).
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = saw 440.0 1.0 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(
        (out[0] + 1.0).abs() < 1e-6,
        "saw at phase 0 must be -amp: out[0]={}",
        out[0]
    );
    assert!(out.iter().all(|s| s.is_finite()));
}

#[cfg(feature = "dsp")]
#[test]
fn square_ffi_end_to_end() {
    // `square` at phase 0 (phase < 0.5) outputs +amp = 1.0; the 440 Hz
    // frequency stays well inside the first half-period for a 4-sample block.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = square 440.0 1.0 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(
        (out[0] - 1.0).abs() < 1e-6,
        "square at phase 0 must be +amp: out[0]={}",
        out[0]
    );
    assert!(out.iter().all(|s| s.is_finite()));
}

#[cfg(feature = "dsp")]
#[test]
fn triangle_ffi_end_to_end() {
    // `triangle` at phase 0: |0 - 0.5|·4 - 1 = 1.0 (amp 1.0).
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = triangle 440.0 1.0 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(
        (out[0] - 1.0).abs() < 1e-6,
        "triangle at phase 0 must be +amp: out[0]={}",
        out[0]
    );
    assert!(out.iter().all(|s| s.is_finite()));
}

#[cfg(feature = "dsp")]
#[test]
fn noise_ffi_end_to_end() {
    // `noise 1.0 0.5` — pink noise, amp 0.5 (deterministic fixed seed). White
    // noise is essentially never all-zero for a 64-sample block; the assertion
    // only requires non-all-zero + finite + within the ±1 amplitude bound.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = noise 1.0 0.5;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(
        out.iter().any(|&s| s != 0.0),
        "noise must produce non-zero samples, got all zeros"
    );
    assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

#[cfg(feature = "dsp")]
#[test]
fn leaky_integrator_ffi_end_to_end() {
    // `leaky_integrator` is a 1→1 catalog builtin: out[n] = x[n] + coeff·out[n-1].
    // With coeff 0.5 and a constant-1 input, the running sum is exact.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = _ : leaky_integrator 0.5;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 1.0, 1.0, 1.0]], &mut [&mut out]).unwrap();
    let expected = [1.0, 1.5, 1.75, 1.875];
    assert!(
        out.iter()
            .zip(expected.iter())
            .all(|(&o, &e)| (o - e).abs() < 1e-6),
        "leaky_integrator(0.5) over ones: got {out:?}, expected {expected:?}"
    );
}

#[cfg(feature = "dsp")]
#[test]
fn lowpass_ffi_end_to_end() {
    // `register_foreign_filters` is the only FFI register fn with zero direct
    // coverage — this exercises its `lowpass` factory (cutoff, q) end-to-end.
    // A lowpass at 1000 Hz on a DC step (all-ones): the filter starts from a
    // zero state, so the first samples are far below the 1.0 input (transient
    // attenuation toward the unity DC gain). A passthrough/identity builtin
    // would emit 1.0 immediately.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_filters;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_filters(&mut ffi);

    let src = r#"main = _ : lowpass 1000.0 0.7;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|s| s.is_finite()),
        "lowpass output must be finite, got {out:?}"
    );
    assert!(
        (0.0..0.1).contains(&out[0]),
        "lowpass step transient must start near zero (b0 ≈ 0.0046), got out[0]={}",
        out[0]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn biquad_ffi_end_to_end() {
    // The general `biquad` factory (type/cutoff/q/gain_db). type 0 = LowPass at
    // 1000 Hz, q 0.7 — same step-transient behaviour as `lowpass`.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_filters;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_filters(&mut ffi);

    let src = r#"main = _ : biquad 0.0 1000.0 0.7 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|s| s.is_finite()),
        "biquad output must be finite, got {out:?}"
    );
    assert!(
        (0.0..0.1).contains(&out[0]),
        "biquad(lowpass 1000 Hz) step transient must start near zero, got out[0]={}",
        out[0]
    );
}

#[cfg(feature = "dsp")]
#[test]
fn integrator_ffi_end_to_end() {
    // `integrator` is a 1→1 running-sum builtin registered as an FFI Block
    // factory; the combinator-sugar `+ ~ _` desugars to it.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_generators;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_generators(&mut ffi);

    let src = r#"main = + ~ _;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 1.0, 1.0, 1.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
}

// --- SP-3b Task 11: tape_loop constructor + Tape as a Buffer member ---

#[test]
fn tape_loop_read_head_compiles_and_runs() {
    // `tape_loop <capacity>` is a foreign constructor producing a `Tape f32`
    // (a `Buffer` family member); `read_head` takes it as its resource param.
    // The plan's Task 11 smoke: `main = read_head (tape_loop 1024) 0.1;`
    // compiles, builds a shared tape cell, and runs finite.
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = read_head (tape_loop 1024) 0.1;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "read_head on a fresh tape must be finite, got {out:?}"
    );
}

#[test]
fn tape_loop_write_read_share_one_cell() {
    // Two `tape_loop 1024` calls dedupe to ONE shared cell (same capacity → same
    // index), so `write_head` and `read_head` in the same program reference the
    // same buffer. Writing constant samples must make the read head eventually
    // return non-zero taps (delay 0.1s at 44.1 kHz = 4410 samples ≈ 69 blocks).
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    // write_head (2-in: dry+fb) and read_head (0-in) run in parallel; both bind
    // the tape_loop 1024 cell (deduplicated by capacity).
    let src = r#"
        main = write_head _ _ (tape_loop 1024) 0.0 0.0, read_head (tape_loop 1024) 0.1;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let dry = [1.0f32; 64];
    let fb = [0.0f32; 64];
    let mut out = [0.0f32; 64];
    // Warm up past the 0.1s delay, then observe the read head's output channel.
    for _ in 0..120 {
        MultichannelAlgorithm::process(&mut prog, &[&dry, &fb], &mut [&mut [0.0f32; 64], &mut out])
            .unwrap();
    }
    assert!(
        out.iter().any(|v| v.abs() > 1e-3),
        "read_head must see the write head's samples through the shared cell, got {out:?}"
    );
}
