//! SP-3a: FFI layer — foreign fn declarations + FixedBuffer/Buffer types.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::types::ty::{BufferRole, TypeEnv, ValueTy};

#[test]
fn signal_prelude_registers_fixedbuffer_and_buffer() {
    // `FixedBuffer elem` is a builtin signal-channel ctor (kind `* -> *`), and
    // `Buffer` a builtin typeclass over buffer types. The prelude's instance
    // head is MONOMORPHIC in the element (`FixedBuffer Double`), so it registers
    // under the full applied type — the element is part of the instance, and
    // there is no element-free `FixedBuffer` key.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.type_ctor_arity("FixedBuffer"), Some(1));
    assert_eq!(env.buffer_role("FixedBuffer"), Some(BufferRole::Channel));
    assert!(env.typeclasses.contains_key("Buffer"));
    assert!(env
        .instances
        .get("Buffer")
        .unwrap()
        .contains_key("FixedBuffer Double"));
    assert!(!env
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
    // Compile + single run must succeed: the SIGNAL_PRELUDE parses, registers,
    // and the instance kind-checks, and `main = 1.0` runs to a Double(1.0).
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    assert_eq!(
        prog.arena().get(prog.result().unwrap()).unwrap(),
        &rill_lang::arena::Value::Double(1.0)
    );
}

#[test]
fn foreign_fn_in_brace_where_block_parses() {
    // Regression: `parse_foreign_def` must leave the trailing `;` for
    // `parse_where_block` (brace style) to consume — double-eating used to
    // fail with "expected Semi, found Ident(x)".
    let src = r"
        main = x where { foreign biquad : FixedBuffer Double -> FixedBuffer Double; x = 1.0; }
    ";
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
    let src = r"
        foreign biquad : Double -> Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
        main = 1.0;
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let foreign = prog
        .defs
        .iter()
        .find_map(|d| match d {
            rill_lang::ast::Def::Foreign { name, sig, .. } if name == "biquad" => Some(sig),
            _ => None,
        })
        .expect(r"foreign biquad not found");
    // Pin the flat-curried signature shape: `Double -> Double -> Double -> Double
    // -> FixedBuffer Double -> FixedBuffer Double` (buffer-LAST: signal trails).
    assert_eq!(
        foreign,
        &rill_lang::ast::TypeExpr::TFunc(
            vec![
                rill_lang::ast::TypeExpr::TName("Double".into()),
                rill_lang::ast::TypeExpr::TName("Double".into()),
                rill_lang::ast::TypeExpr::TName("Double".into()),
                rill_lang::ast::TypeExpr::TName("Double".into()),
                rill_lang::ast::TypeExpr::TApp(
                    "FixedBuffer".into(),
                    vec![rill_lang::ast::TypeExpr::TName("Double".into())]
                ),
            ],
            Box::new(rill_lang::ast::TypeExpr::TApp(
                "FixedBuffer".into(),
                vec![rill_lang::ast::TypeExpr::TName("Double".into())]
            ))
        )
    );
}

#[test]
fn foreign_sig_describes_signal_and_scalar_params() {
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    // `Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double` — the
    // signal channel trails the scalar params (buffer-LAST).
    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TName("Double".into()),
            TypeExpr::TName("Double".into()),
            TypeExpr::TName("Double".into()),
            TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Double".into())]),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("Double".into())],
        )),
    );
    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(&env, "biquad", &te).expect("FFI sig");
    assert_eq!(sig.params.len(), 4);
    assert!(matches!(sig.params[0], FfiParam::Scalar));
    assert!(matches!(sig.params[1], FfiParam::Scalar));
    assert!(matches!(sig.params[2], FfiParam::Scalar));
    assert!(matches!(sig.params[3], FfiParam::Signal(ValueTy::Double)));
    assert_eq!(sig.signal_outs, 1);
    assert_eq!(sig.param_names, vec!["type", "cutoff", "q"]);
}

#[test]
fn foreign_sig_from_parser_flat_curried_form() {
    // The parser emits a FLAT `TFunc(args: Vec, ret)` for carried arrows
    // (`a -> b -> c -> r` → `TFunc([a, b, c], r)`, not nested one-arg-at-a-time).
    // Feed the real parser output into the descriptor to pin that compatibility.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let src = r"foreign biquad : Double -> Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double; main = 1.0;";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let sig = prog
        .defs
        .iter()
        .find_map(|d| match d {
            rill_lang::ast::Def::Foreign { sig, .. } => Some(sig),
            _ => None,
        })
        .expect(r"foreign biquad not found");
    let env = TypeEnv::with_builtins();
    let ffi = ffi_sig_from_typeexpr(&env, "biquad", sig).expect("FFI sig from parser output");
    assert_eq!(ffi.params.len(), 5);
    assert!(matches!(ffi.params[0], FfiParam::Scalar));
    assert!(matches!(ffi.params[1], FfiParam::Scalar));
    assert!(matches!(ffi.params[2], FfiParam::Scalar));
    assert!(matches!(ffi.params[3], FfiParam::Scalar));
    assert!(matches!(ffi.params[4], FfiParam::Signal(ValueTy::Double)));
    assert_eq!(ffi.signal_outs, 1);
}

#[test]
fn foreign_decl_registers_into_type_env() {
    // A foreign fn declaration registers its signature into TypeEnv::foreign_sigs
    // via the shared `register_decls` path (the same path inference phase 1 uses).
    // This is the compiler state Task 4 (infer foreign names) will consume.
    let src = r"
        foreign biquad : Double -> Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
        main = 1.0;
    ";
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
    // Task 13b: `gain : Double -> FixedBuffer Double -> FixedBuffer Double` with its
    // scalar params and the signal over the program input — in the unary value
    // model the arrow typing is gone, but the foreign fn over the input still
    // types as a value program (arity_in is always 0; arity_out is 1).
    let src = r"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    // `def_schemes["main"].ty` is the value type of the whole program (main).
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

#[test]
fn foreign_fn_wrong_arity_is_error() {
    // `onepole` takes 2 scalars + 1 signal = 3 args; a 1-arg call is a type
    // error (omitting a scalar is an arity error — only a TRAILING run of
    // omitted signals may be combinator-fed).
    let src = r"
        foreign onepole : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
        main = onepole 200.0;
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog, prog.mode) {
        Err(e) => assert!(
            e.to_string().contains("expects at least"),
            "expected an arity error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for wrong arity"),
    }
}

#[test]
fn foreign_fn_lowers_to_callblock() {
    // Lower `gain 0.5 _` and assert the IR contains a CallBlock referencing the
    // `gain` builtin with one signal input; the scalar param 0.5 is an ordinary
    // value-track argument (not a folded param).
    use rill_lang::lower::lower_with_cafs;

    let src = r"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = lower_with_cafs(&typed, &typed.cafs).unwrap();
    let gain = ir
        .builtins
        .iter()
        .find(|b| b.name == "gain")
        .expect("gain builtin");
    assert_eq!(gain.signal_ins, 1, "one signal input");
    assert_eq!(
        gain.params,
        vec![0.5],
        "scalar param folds into the builtin at build (currying)"
    );
    // The FFI arrow is a Callable fragment holding one ValueCallForeign with
    // the Buffer input as its src; scalar params are folded into the builtin.
    let gain_idx = ir
        .builtins
        .iter()
        .position(|b| b.name == "gain")
        .expect("gain builtin index");
    let call = ir
        .fragments
        .iter()
        .flat_map(|f| f.value_blocks.iter().flat_map(|b| &b.instrs))
        .find(|i| matches!(i, rill_lang::ir::ValueInstr::ValueCallForeign { instance, .. } if *instance == gain_idx))
        .expect("gain ValueCallForeign in its fragment");
    let rill_lang::ir::ValueInstr::ValueCallForeign { srcs, .. } = call else {
        unreachable!()
    };
    assert_eq!(srcs.len(), 1, "one Buffer input; scalar params are folded");
}

#[test]
fn foreign_fn_signal_slot_rejects_value() {
    // A VALUE-channel expression in a Signal slot is a type error (rate check).
    // Note: a bare Double literal is itself a Signal-rate constant signal
    // (0-in generator), so the rejection needs a genuine Value expression —
    // here a String literal in the trailing signal slot.
    let src = r#"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 "oops";
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog, prog.mode) {
        Err(e) => assert!(
            e.to_string().contains("must be a signal"),
            "expected a signal-slot rate error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a value in a Signal slot"),
    }
}

#[test]
fn foreign_sig_rejects_non_terminal_variadic_signal() {
    // A VariadicSignal (`List (FixedBuffer Double)`) that is not the LAST param
    // is ambiguous: lowering would fold any trailing scalar as a signal, while
    // inference counts a different arity. The descriptor must reject it.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    // `List (FixedBuffer Double) -> Double -> FixedBuffer Double` — variadic mid-signature.
    let te = rill_lang::ast::TypeExpr::TFunc(
        vec![
            rill_lang::ast::TypeExpr::TApp(
                "List".into(),
                vec![rill_lang::ast::TypeExpr::TApp(
                    "FixedBuffer".into(),
                    vec![rill_lang::ast::TypeExpr::TName("Double".into())],
                )],
            ),
            rill_lang::ast::TypeExpr::TName("Double".into()),
        ],
        Box::new(rill_lang::ast::TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![rill_lang::ast::TypeExpr::TName("Double".into())],
        )),
    );
    let env = TypeEnv::with_builtins();
    assert_eq!(ffi_sig_from_typeexpr(&env, "bad", &te), None);

    // A terminal VariadicSignal is still accepted (`List (FixedBuffer Double) ->
    // FixedBuffer Double`).
    let ok = rill_lang::ast::TypeExpr::TFunc(
        vec![rill_lang::ast::TypeExpr::TApp(
            "List".into(),
            vec![rill_lang::ast::TypeExpr::TApp(
                "FixedBuffer".into(),
                vec![rill_lang::ast::TypeExpr::TName("Double".into())],
            )],
        )],
        Box::new(rill_lang::ast::TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![rill_lang::ast::TypeExpr::TName("Double".into())],
        )),
    );
    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(&env, "sum_all", &ok).expect("terminal variadic is accepted");
    assert!(matches!(
        sig.params[0],
        FfiParam::VariadicSignal(ValueTy::Double)
    ));
}

#[test]
fn foreign_fn_non_terminal_variadic_fails_inference() {
    // `foreign fn bad : List (FixedBuffer Double) -> Double -> FixedBuffer Double;`
    // — a non-terminal VariadicSignal — must be rejected: the descriptor
    // returns None, so `bad` does not resolve as a foreign builtin and any
    // call to it fails inference (value-model call form).
    let src = r"
        foreign bad : List (FixedBuffer Double) -> Double -> FixedBuffer Double;
        main = bad 0.5 (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    assert!(
        rill_lang::types::infer::infer_program(&prog, prog.mode).is_err(),
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
    ffi.register_block("gain", |params: &[f64], _sr: f32, _alloc| {
        Box::new(Gain(params.first().copied().unwrap_or(1.0)))
    });
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 (square 0.0 1.0 0.0);
    ";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    assert_eq!(out, [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn foreign_fn_not_registered_is_compile_error() {
    use rill_lang::ffi::ForeignRegistry;

    let ffi = ForeignRegistry::<f32>::new();
    let src = r"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 (square 0.0 1.0 0.0);
    ";
    let err = match eval_at(src, "main", ffi, 44_100.0) {
        Err(e) => e,
        Ok(_) => panic!("expected an error for an unregistered foreign builtin"),
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
    // `TypeEnv::foreign_sigs` (SP-3b Task 3). The generator list is a
    // 0-input IO program.
    let src = r#"
        main = sine 440.0 1.0 0.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

#[test]
fn signal_input_builtin_legacy_call_style_sugar() {
    // Task 13b: the legacy `_ : onepole 200.0 0.7` combinator style is removed;
    // the same signal-input builtin now applies as an ordinary value function
    // over the program input. This pins the value-model typing.
    let src = r"
        foreign onepole : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
        main = onepole 200.0 0.7 (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

// Task 13b: the following tests were removed with the signal surface syntax —
// each exercised a construct that no longer exists in the unary value model, so
// there is no value-model equivalent to preserve:
//
//   combinator_sugar_binds_delay_lhs           — the `@` delay combinator
//   combinator_sugar_binds_loop_operand        — the `~` loop/feedback combinator
//   combinator_sugar_binds_merge_operand       — the `:` / `:>` channel merge
//   combinator_sugar_binds_split_operand       — the `<:` / `:` channel split
//   combinator_sugar_binds_par_operand_with_two_signals
//                                              — parallel two-signal wiring
//   combinator_sugar_binds_gain_scalar_only_call
//                                              — the wire-counting sugar pass
//   combinator_sugar_does_not_double_bind_positional_call
//                                              — the positional wire sugar
//   foreign_fn_multi_signal_wires_distinct_inputs
//                                              — multi-channel signal routing
//   foreign_variadic_signal_merge_accepts_channels
//                                              — variadic channel merge
//
// The surviving `combinator_sugar_*` tests below cover the value-model
// equivalents (partial application applied to a constant buffer).

#[test]
fn combinator_sugar_binds_both_seq_operands() {
    // Task 13b: the `:` sequential combinator is removed; two signal-input
    // builtins in sequence (`onepole 200.0 0.7` then `onepole 1000.0 0.9`)
    // now compose as nested `map`s over the program input. The program types
    // as a value program (arity_in 0, arity_out 1).
    let src = r#"
        main = onepole 1000.0 0.9 (onepole 200.0 0.7 (square 0.0 1.0 0.0));
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_rewrites_where_block_defs() {
    // Task 13b: the combinator desugar pass is gone; a where-block def is now
    // inferred as its own def group in the value model. The def carries the
    // whole value program (FFI effect over the input), and the program types
    // as a value program.
    let src = r#"
        main = x where { x = onepole 200.0 0.7 (square 0.0 1.0 0.0); }
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

#[test]
fn combinator_sugar_lowers_positionally() {
    // Task 13b: the `_ :` combinator desugar is gone; the FFI lowering path is
    // now driven by the canonical value form. `onepole 200.0 0.7` mapped over
    // the input folds the scalars and wires one signal input through lowering.
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        main = onepole 200.0 0.7 (square 0.0 1.0 0.0);
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = lower_with_cafs(&typed, &typed.cafs).unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "onepole").unwrap();
    assert_eq!(bi.signal_ins, 1);
    assert_eq!(bi.params, vec![200.0, 0.7]);
}

#[test]
fn builtin_catalog_registers_record_data_types() {
    // The record types the catalog's record-param builtin (`eq_parametric`)
    // needs register as `data` declarations — Task 4's record-param lowering
    // reads them from `TypeEnv::data_types`.
    let env = TypeEnv::with_builtins();
    for name in ["EqBand", "EqConfig"] {
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
                rill_lang::types::ty::ValueTy::Con(head, args)
                    if head == "List" && args.len() == 1
            ));
        }
        other => panic!("EqConfig must be a record, got {other:?}"),
    }
}

#[test]
fn ffi_sig_param_names_filled_from_catalog_table() {
    // The name table (mirroring legacy `BuiltinSig::param_names`) supplies
    // display names for scalar params in declaration order.
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::ffi_sig_from_typeexpr;

    let out = TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Double".into())]);
    let f = |n: &str| TypeExpr::TName(n.into());
    let fb = |n: &str| TypeExpr::TApp(n.into(), vec![TypeExpr::TName("Double".into())]);

    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(
        &env,
        "sine",
        &TypeExpr::TFunc(
            vec![f("Double"), f("Double"), f("Double")],
            Box::new(out.clone()),
        ),
    )
    .expect("sine FFI sig");
    assert_eq!(sig.param_names, vec!["freq", "amp", "phase"]);

    let sig = ffi_sig_from_typeexpr(
        &env,
        "biquad",
        &TypeExpr::TFunc(
            vec![
                f("Double"),
                f("Double"),
                f("Double"),
                f("Double"),
                fb("FixedBuffer"),
            ],
            Box::new(out.clone()),
        ),
    )
    .expect("biquad FFI sig");
    assert_eq!(sig.param_names, vec!["type", "cutoff", "q", "gain_db"]);

    let sig = ffi_sig_from_typeexpr(
        &env,
        "eq_parametric",
        &TypeExpr::TFunc(vec![f("EqConfig"), fb("FixedBuffer")], Box::new(out)),
    )
    .expect("eq_parametric FFI sig");
    // eq_parametric's record config has no scalar display names — the record
    // flattens through its schema at lowering.
    assert!(sig.param_names.is_empty());
}

#[test]
fn ffi_sig_param_names_fallback_to_index_for_unknown() {
    // A builtin not in the name table gets index-based names (param0, param1…)
    // so host/tooling inspection sees stable index-based scalar names (neither
    // lowering nor seat-time binding reads these names — both are positional).
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::ffi_sig_from_typeexpr;

    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TName("Double".into()),
            TypeExpr::TName("Double".into()),
            TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Double".into())]),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("Double".into())],
        )),
    );
    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(&env, "not_a_catalog_builtin", &te).unwrap();
    assert_eq!(sig.param_names, vec!["param0", "param1"]);
}

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn tape_param_maps_to_resource() {
    // `Tape a` — a shared-buffer handle — is a Resource param (the tape arg is
    // a symbolic `Ref`, wired to the shared buffer at build, not a signal).
    use rill_lang::ast::TypeExpr;
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp("Tape".into(), vec![TypeExpr::TName("Double".into())]),
            TypeExpr::TName("Double".into()),
        ],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("Double".into())],
        )),
    );
    let env = TypeEnv::with_builtins();
    let sig = ffi_sig_from_typeexpr(&env, "read_head", &te).unwrap();
    assert!(matches!(sig.params[0], FfiParam::Resource(ValueTy::Double)));
    assert!(matches!(sig.params[1], FfiParam::Scalar));
}

#[test]
fn ffi_record_schema_reads_data_types() {
    // `ffi_record_schema` mirrors the legacy RecordSchema from the catalog's
    // `data` declarations: field name + scalar type in declaration order, with
    // the legacy `RecordField::default` values (SP-3b Task 7, follow-up A).
    use rill_lang::types::ffi::{ffi_record_schema, FfiScalar};

    let env = TypeEnv::with_builtins();
    let schema = ffi_record_schema(&env, "EqConfig").expect("EqConfig schema");
    assert_eq!(
        schema.fields,
        vec![(
            "bands".to_string(),
            FfiScalar::BandList("EqBand".to_string()),
            None
        )]
    );
    assert!(ffi_record_schema(&env, "NoSuchType").is_none());
}

#[test]
fn record_param_infer_accepts_record_literal() {
    // The FFI infer arm validates a Record param's arg is a record literal of
    // the named data type (previously an unconditional SP-3b error).
    let src = r"
        foreign eq_parametric : EqConfig -> FixedBuffer Double -> FixedBuffer Double;
        main = eq_parametric { bands: [] } (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    assert_eq!(typed.def_schemes["main"].ty.arity_in(), 0);
    assert_eq!(typed.def_schemes["main"].ty.arity_out(), 1);
}

#[test]
fn record_param_flattens_schema_fields() {
    // `eq_parametric` takes an EqConfig record whose `bands: List EqBand`
    // flattens each band's schema fields (4 per band) into the folded param
    // list — two bands → 8 f64s.
    let src = r"
        foreign eq_parametric : EqConfig -> FixedBuffer Double -> FixedBuffer Double;
        main = eq_parametric { bands: [
            { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 },
            { freq: 3000.0, q: 1.0, gain_db: -3.0, band_type: 4.0 }
        ] } (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
    let bi = ir
        .builtins
        .iter()
        .find(|b| b.name == "eq_parametric")
        .unwrap();
    assert_eq!(
        bi.params,
        vec![1000.0, 1.0, 0.0, 0.0, 3000.0, 1.0, -3.0, 4.0]
    );
}

#[test]
fn record_param_missing_fields_use_zero_defaults() {
    // A partial record fills the omitted schema field with its default (0.0 in
    // the FFI schema — the catalog `data` declaration carries no defaults).
    let src = r"
        data TestCfg = { a: Double, b: Double };
        foreign cfg : TestCfg -> FixedBuffer Double;
        main = cfg { a: 1.5 };
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "cfg").unwrap();
    assert_eq!(bi.params, vec![1.5, 0.0]);
}

#[test]
fn record_param_rejects_non_record_arg() {
    let src = r"
        foreign eq_parametric : EqConfig -> FixedBuffer Double -> FixedBuffer Double;
        main = eq_parametric 0.5 (square 0.0 1.0 0.0);
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog, prog.mode) {
        Err(e) => assert!(
            e.to_string().contains("record literal"),
            "expected a record-literal error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a non-record record param"),
    }
}

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn resource_param_wires_tape_ref() {
    // A `Tape Double` param is a Resource: lowering sets the builtin's resource to
    // the symbolic `Ref` name (the build path resolves it in the registry).
    let src = r"
        foreign read_head : Tape Double -> Double -> FixedBuffer Double;
        main = read_head tape_0 0.1;
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
    let bi = ir.builtins.iter().find(|b| b.name == "read_head").unwrap();
    assert_eq!(bi.resource.as_deref(), Some("tape_0"));
    assert_eq!(bi.params, vec![0.1]);
}

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn resource_param_rejects_non_ref_arg() {
    let src = r"
        foreign read_head : Tape Double -> Double -> FixedBuffer Double;
        main = read_head 0.5 0.1;
    ";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    match rill_lang::types::infer::infer_program(&prog, prog.mode) {
        Err(e) => assert!(
            e.to_string().contains("symbolic reference"),
            "expected a symbolic-reference error, got: {e}"
        ),
        Ok(_) => panic!("expected a type error for a non-Ref resource param"),
    }
}

// --- SP-3b Task 7: record defaults + List EqBand band-list (router migration) ---

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
            ("freq".to_string(), FfiScalar::Double, Some(1000.0)),
            ("q".to_string(), FfiScalar::Double, Some(1.0)),
            ("gain_db".to_string(), FfiScalar::Double, Some(0.0)),
            ("band_type".to_string(), FfiScalar::Int, Some(0.0)),
        ]
    );
}

#[test]
fn record_default_ffi_applies() {
    // Follow-up A end-to-end: omitted BAND fields in a `bands` list must apply
    // the legacy `EqBand` defaults (`freq: 1000.0`) — not silently 0.0.
    let src = r#"
        main = eq_parametric { bands: [ { q: 2.0 } ] } (square 0.0 1.0 0.0);
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
    let bi = ir
        .builtins
        .iter()
        .find(|b| b.name == "eq_parametric")
        .unwrap();
    assert_eq!(bi.params, vec![1000.0, 2.0, 0.0, 0.0]);
}

#[test]
fn band_list_flattens_band_fields() {
    // Follow-up B end-to-end: `eq_parametric { bands: [ { freq: 1000.0, q: 1.0,
    // gain_db: 0.0, band_type: 0.0 } ] }` flattens each band's fields in schema
    // order into the folded param list.
    let src = r#"
        main = eq_parametric { bands: [ { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 } ] } (square 0.0 1.0 0.0);
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
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

    let src = r#"main = analog_moog 500.0 0.7 (square 0.0 1.0 0.0)"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    assert!(
        out.iter().all(|v| v.is_finite()),
        "analog_moog output must be finite, got {out:?}"
    );
}

// --- SP-3b Task 7: EQ FFI E2E (graphic_eq / eq_parametric) ---

#[cfg(feature = "dsp")]
#[test]
fn eq_parametric_ffi_end_to_end() {
    // `eq_parametric` with a `bands` list — the BandList flattening (Follow-up
    // B) feeds the factory's per-band params. A unity-gain peak band passes
    // the first sample unchanged (b0 = 1); the run proves the band config
    // reached the factory.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_eq;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_eq(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"
        main = eq_parametric { bands: [ { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 } ] } (square 0.0 1.0 0.0);
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    // back to param0/param1 and corrupt the param ordering.
    use rill_lang::types::ffi::{ffi_sig_from_typeexpr, FfiParam};

    let env = TypeEnv::with_builtins();
    let mut scalar_builtins = 0usize;
    for (name, te) in &env.foreign_sigs {
        let Some(sig) = ffi_sig_from_typeexpr(&env, name, te) else {
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
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"
        main = delay 0.1 0.3 0.5 (square 0.0 1.0 0.0);
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"
        main = distortion 2.0 1.0 (square 0.0 1.0 0.0);
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    // Soft-clip: tanh(2.0) with output gain 1.0.
    let expected = 2.0f32.tanh();
    assert!(
        (out[0] - expected).abs() < 1e-4,
        "distortion soft-clip: out[0]={} expected ~{expected}",
        out[0]
    );
}

// NOTE — DELETED (Class 2, re-run surface removed):
//   digital_effects_delay_live_param_reaches_algorithm — a regression pinning
//   that the FFI wrapper's `set_param` routes a live SetParameter
//   (`delay ?t 0.0 1.0`) into the algorithm across 3+ ticks. `test_eval` runs
//   exactly once and cannot inject a parameter, so both the default-delay tap
//   and the SetParameter(t, 0.3) re-tick assertions are unrepresentable.

#[cfg(feature = "dsp")]
#[test]
fn digital_effects_limiter_runs() {
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_digital_effects;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_digital_effects(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"
        main = limiter (-6.0) 0.1 (square 0.0 1.0 0.0);
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    // Lookahead warm-up: the limiter passes the first `lookahead_samples` (220)
    // unchanged, so the 4-sample block is a passthrough.
    assert_eq!(out, [1.0, 1.0, 1.0, 1.0]);
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
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

    let src = r#"main = leaky_integrator 0.5 (square 0.0 1.0 0.0);"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
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
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"main = lowpass 1000.0 0.7 (square 0.0 1.0 0.0);"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
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
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"main = biquad 0.0 1000.0 0.7 0.0 (square 0.0 1.0 0.0);"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
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

    let src = r#"main = integrator (square 0.0 1.0 0.0);"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
}

// --- SP-3b Task 11: tape_loop constructor + Tape as a Buffer member ---

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn tape_loop_read_head_compiles_and_runs() {
    // `tape_loop <capacity>` is a foreign constructor producing a `Tape Double`
    // (a `Buffer` family member); `read_head` takes it as its resource param.
    // The plan's Task 11 smoke: `main = read_head (tape_loop 1024) 0.1;`
    // compiles, builds a shared tape cell, and runs finite.
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = read_head (tape_loop 1024) 0.1;"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
    assert!(
        out.iter().all(|v| v.is_finite()),
        "read_head on a fresh tape must be finite, got {out:?}"
    );
}

// NOTE — DELETED (Class 2, multi-channel):
//   inline_tape_loop_calls_are_distinct_tapes — `main = read_head (tape_loop
//   8) 0.1, read_head (tape_loop 8) 0.1;` is a two-output program read through
//   `copy_output_samples(1, …)`. The at-most-one-output model has no
//   equivalent; the distinct-tape invariant it pinned is no longer observable
//   through the single-run surface.

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn tape_loop_float_capacity_is_rejected() {
    // `tape_loop` is declared `Int -> Tape Double`; a Double capacity (`1024.5`)
    // must be a compile error, not silently truncated to 1024.
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = read_head (tape_loop 1024.5) 0.1;"#;
    match rill_lang::compile::<f32, 256>(src, ffi) {
        Err(e) => assert!(
            e.to_string().contains("capacity"),
            "expected a capacity error, got: {e}"
        ),
        Ok(_) => panic!("expected a compile error for a Double tape_loop capacity"),
    }
}

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn tape_loop_zero_capacity_is_rejected() {
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = read_head (tape_loop 0) 0.1;"#;
    assert!(
        rill_lang::compile::<f32, 256>(src, ffi).is_err(),
        "zero-capacity tape_loop must be rejected"
    );
}

#[test]
#[ignore = "resource/tape deferred: architectural issues"]
fn undeclared_resource_ref_is_reported_on_source_path() {
    // A `Ref` to a tape that was never declared (`missing`) must produce the
    // actionable "references undeclared resource 'missing' (declare it with
    // `tape = tape_loop <capacity>`…)" error on the source-level compile path —
    // not the confusing "carries no tape index" fallthrough.
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = read_head missing 0.1;"#;
    match rill_lang::compile::<f32, 256>(src, ffi) {
        Err(e) => assert!(
            e.to_string().contains("undeclared resource 'missing'"),
            "expected the undeclared-resource diagnostic, got: {e}"
        ),
        Ok(_) => panic!("expected a compile error for an undeclared tape ref"),
    }
}

#[test]
fn main_is_not_extracted_as_tape_declaration() {
    // `main = tape_loop 1024` is a plausible typo; `extract_resources` must not
    // eat `main` as a resource declaration (which would surface as a confusing
    // "program must contain a main definition" error).
    use rill_lang::ffi::ForeignRegistry;
    use rill_sampler::tape::lang::register_tape_ffi;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_tape_ffi(&mut ffi);

    let src = r#"main = tape_loop 1024;"#;
    match rill_lang::compile::<f32, 256>(src, ffi) {
        Err(e) => assert!(
            !e.to_string().contains("must contain a `main`"),
            "the typo must not be reported as a missing main, got: {e}"
        ),
        Ok(_) => panic!("expected a compile error for `main = tape_loop`"),
    }
}

// --- SP-3b Task 12 follow-up: eq 2+ bands ---

#[cfg(feature = "dsp")]
#[test]
fn eq_parametric_ffi_two_bands() {
    // `eq_parametric` with TWO bands: the BandList flattening must feed both
    // bands' params to the factory in order (2·4 = 8 f64s) and run finite.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_eq;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_eq(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);

    let src = r#"
        main = eq_parametric { bands: [
            { freq: 1000.0, q: 1.0, gain_db: 0.0, band_type: 0.0 },
            { freq: 3000.0, q: 1.0, gain_db: -3.0, band_type: 4.0 }
        ] } (square 0.0 1.0 0.0);
    "#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    assert!(
        out.iter().all(|v| v.is_finite()),
        "2-band eq_parametric output must be finite, got {out:?}"
    );
}
