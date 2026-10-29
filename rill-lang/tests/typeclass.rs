//! Typeclass tests: `typeclass`/`instance` declare polymorphic methods that
//! resolve **at compile time** to the concrete instance's body. The method
//! argument's static type selects the instance; the body is β-substituted at
//! compile time (zero runtime dispatch — typeclasses and instances produce no
//! arena values).
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::types::ty::ValueTy;

/// Compile and run `src`, returning the concatenated stdout.
fn run_out(src: &str) -> String {
    let mut prog =
        rill_lang::compile::<f32, 256>(src, ForeignRegistry::new()).expect("compile failed");
    prog.run().expect("run failed");
    prog.stdout().concat()
}

/// Compile `src` (which must bind `main` to the evaluated expression) and RUN
/// it once on the single-run value surface, returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn typeclass_method_resolves_at_compile_time() {
    // `show: a` declares the method over the class variable. `show f = f` is
    // the Double instance's identity body: `show 1.0` resolves to it, binds
    // `f := 1.0`, and the value output is Double(1.0). If resolution did not
    // happen (or the param binding were lost), the call could not compile.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = f; }
        main = show 1.0;
    "#;
    let prog = run(src);
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Double(1.0));
}

#[test]
fn typeclass_selects_instance_by_argument_type() {
    // Two instances of the same class with different bodies: the argument's
    // static type must pick the right one. `show 1` (an Int literal) must
    // resolve to the Int instance's body (2.5), not the Double instance's (1.5).
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = 1.5; }
        instance Show Int where { show f = 2.5; }
        main = show 1;
    "#;
    let prog = run(src);
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Double(2.5));
}

#[test]
fn recursive_typeclass_method_is_compile_error() {
    // A method body that inlines itself must be a compile error, not a stack
    // overflow / SIGABRT of the compiler.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = show f; }
        main = show 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.expect_err("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("recursive typeclass method"),
                "expected recursive-method message, got: {msg}",
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
}

#[test]
fn transitive_recursive_typeclass_methods_are_compile_error() {
    // `a` inlines `b` which inlines `a` — the recursion guard must catch the
    // cycle through the chain, not just a method inlining itself.
    let src = r#"
        typeclass C a where { a: a; b: a; }
        instance C Double where { a f = b f; b f = a f; }
        main = a 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.expect_err("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("recursive typeclass method"),
                "expected recursive-method message, got: {msg}",
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
}

#[test]
fn recursive_constructor_class_method_is_compile_error() {
    // Constructor-class branch of the recursion guard: an INSTANCE body that
    // calls its own method (`f k = f k`) must be a compile error. The guard
    // arms around the inlined BODY only (not the arguments — composed
    // defaults legitimately re-enter other methods' bodies through arguments),
    // so genuine body self-re-entry must still trip it.
    let src = r"
        typeclass A a where { f: a b c -> a b c; }
        data K m a b = { unK: a -> m b };
        instance (Monad m) => A (K m) where { f k = f k; }
        apply k x = let u = k.unK in u x;
        main = case (apply (f (K { unK: \x -> Just x })) 3.0) of { Just v -> v; Nothing -> 0.0; };
    ";
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.expect_err("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("recursive typeclass method"),
                "expected recursive-method message, got: {msg}",
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
}

#[test]
fn buffer_value_does_not_select_scalar_instance() {
    // A Buffer computation is an ordinary value, but it cannot select a
    // `Show Double` instance: the operand's type is `Buffer`, never `Double`.
    //
    // The rejection is reported at the ARGUMENT, because `sin 1.0` is itself
    // ill-typed (`sin` takes a `Buffer`; a bare literal is not one), and a
    // type checker that reported the outer method first would hide the real
    // cause. What must never happen is the other reading: a silent selection of
    // `Show Double` for a Buffer, or a successful compile.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = f; }
        main = show (sin 1.0);
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.expect_err("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("`sin`") || msg.contains("cannot resolve method `show`"),
                "expected a diagnostic naming the ill-typed argument or the \
                 unresolved method, got: {msg}",
            );
            assert!(
                !msg.contains("no instance of `Show`"),
                "a Buffer must not select `Show Double`: {msg}"
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
    // The same program with a literal operand DOES select the instance, so the
    // rejection above is about the Buffer, not about `show` being unusable.
    let ok = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = f; }
        main = show 1.0;
    "#;
    let ir = compile_eval::<f32, 256>(ok, "main", Mode::Strict).expect("`show 1.0` must compile");
    assert!(
        ir.dicts
            .iter()
            .any(|d| d.class == "Show" && d.ty_key == "Double"),
        "`show 1.0` selects `Show Double`; table = {:?}",
        ir.dicts
            .iter()
            .map(|d| (d.class.clone(), d.ty_key.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn instance_body_accepts_value_expression() {
    // A Buffer computation (`sin 1.0 : Buffer`) is an ordinary value: a
    // typeclass method body may now be a buffer-value expression (the block
    // track is gone; buffers are values). The body is validated as a value
    // channel at compile time even when the instance is never called.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Double where { show f = sin 1.0; }
        main = 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_ok(), "expected success");
}

#[test]
fn user_instance_redefining_a_prelude_class_is_validated() {
    // A user-declared instance whose `(class, type)` key matches a PRELUDE one
    // (`Monad IO`) replaces the prelude entry in the instance table. It must
    // then be validated exactly like any other user instance — otherwise a
    // program could silently redefine how every `do`-block sequences effects,
    // with neither the kind/method-signature check nor the self-inlining
    // recursion guard running. The body below is the case only the validator
    // rejects (main never calls `return`, so no call-site check fires).
    let src = r#"
        instance Monad IO where { return x = return x; bind m f = bind m f; }
        main = 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(
        res.is_err(),
        "a user instance overwriting the prelude's must be validated, got success"
    );
    match res.expect_err("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("recursive typeclass method"),
                "expected recursive-method message, got: {msg}",
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
}

#[test]
fn user_def_shadows_builtin_eq_ord_method_names() {
    // The builtin `Eq`/`Ord` typeclasses declare `eq`/`lt` methods, but a user
    // definition with the same name is a plain function and must compile.
    let src = r"
        lt = \a -> \b -> a * b;
        main = lt 2.0 3.0;
    ";
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(6.0)
    );
}

/// Infer the program and return the value type of the binding `x`. Panics when
/// the program is rejected — use [`infer_err`] for a program that must fail.
fn infer_x(src: &str) -> ValueTy {
    let typed = infer_typed(src);
    typed.def_schemes["x"].ty.vty.clone()
}

/// Infer `src` at `Mode::Complete`, panicking with the diagnostic if rejected.
fn infer_typed(src: &str) -> rill_lang::types::infer::TypedProgram {
    let toks = rill_lang::lexer::tokenize(src).expect("lex");
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
    rill_lang::types::infer::infer_program(&prog, Mode::Complete)
        .unwrap_or_else(|e| panic!("`{src}` must typecheck: {e}"))
}

/// The diagnostic for a program inference REJECTS, or `None` when it accepts it.
fn infer_err(src: &str) -> Option<rill_lang::CompileError> {
    let toks = rill_lang::lexer::tokenize(src).expect("lex");
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
    rill_lang::types::infer::infer_program(&prog, Mode::Complete).err()
}

#[test]
fn an_applied_primitive_method_has_its_result_type() {
    // A FULLY APPLIED primitive method's type is the signature's RESULT, not the
    // whole unapplied arrow. `fromInteger : Int -> a` applied to one argument is
    // an `a` solved by the instance — `Integer` at `Num Integer`. Before the fix
    // `x`'s scheme was `Thunk([Int], [Integer])` (the whole arrow), so the
    // annotation could not unify: "cannot unify value type Thunk([Int],
    // [Integer]) with Integer".
    //
    // This is asserted at the INFERENCE layer because it is an inference defect:
    // `fromInteger`'s VALUE needs the `FromInteger` IR instruction (design §3.9,
    // Task 5), so running the program would report an unrelated lowering gap.
    // Two targets of the SAME polymorphic method, so the answer comes from the
    // instance and not from a defaulted variable.
    assert_eq!(
        infer_x("x :: Integer = fromInteger 1;\nmain = 0.0;"),
        ValueTy::Integer,
        "`(fromInteger 1) :: Integer` is the applied RESULT, not the method arrow"
    );
    assert_eq!(
        infer_x("x :: Double = fromInteger 1;\nmain = 0.0;"),
        ValueTy::Double,
        "`(fromInteger 1) :: Double` is the applied RESULT, not the method arrow"
    );
    // CONTROL, not evidence: `(+)` is a fully applied primitive method whose
    // result type was already right (it takes the operator path), so it must
    // keep RUNNING. This guards the fix from having traded a result type for a
    // value; it passes before the fix too, and is labelled as a control rather
    // than counted as a demonstration of it.
    assert_eq!(
        run_out(
            "x :: Double = (+) 1.0 2.0;\nmain = do { write (Just (show_double x)) stdout; return () };"
        ),
        "3.0"
    );
}

#[test]
fn a_result_directed_primitive_method_checks_its_arity() {
    // GHC 9.10.3: `fromInteger 1 2` is "No instance for `Num (t0 -> t1)`" —
    // a method applied to MORE arguments than its signature takes is an error.
    // The result-directed primitive branch had no arity check, so returning the
    // applied result (the previous fix) made these typecheck as `Integer`.
    assert!(
        infer_err("x :: Integer = fromInteger 1 2;\nmain = 0.0;").is_some(),
        "`fromInteger 1 2` must be an arity error, not an `Integer`"
    );
    assert!(
        infer_err("x :: Integer = fromInteger 1 2 3;\nmain = 0.0;").is_some(),
        "`fromInteger 1 2 3` must be an arity error"
    );
    // Controls: correct arity still works.
    assert_eq!(
        infer_x("x :: Integer = fromInteger 1;\nmain = 0.0;"),
        ValueTy::Integer
    );
    assert_eq!(
        infer_x("x :: Double = fromInteger 1;\nmain = 0.0;"),
        ValueTy::Double
    );
}

#[test]
fn every_call_site_applies_the_same_curried_rule() {
    use rill_lang::lexer::tokenize;
    use rill_lang::parser::parse;
    let ok = |src: &str| {
        let toks = tokenize(src).expect("lex");
        let prog = parse(&toks, src.as_bytes()).expect("parse");
        rill_lang::types::infer::infer_program(&prog, Mode::Complete).is_ok()
    };
    // Under-application yields a function value; over-application is an error.
    // GHC 9.10.3: `fromInteger 1 2` is "No instance for `Num (t0 -> t1)`".
    assert!(ok("x :: Integer = fromInteger 1;\nmain = 0.0;"));
    assert!(!ok("x :: Integer = fromInteger 1 2;\nmain = 0.0;"));
    assert!(!ok("x :: Integer = fromInteger 1 2 3;\nmain = 0.0;"));
    // A partial application of a TWO-parameter method is a function value, and
    // applying it once more completes it — the property the copies disagreed on.
    assert!(ok("h :: Double -> Double;\nh = (+) 1.0;\nmain = 0.0;"));
    assert!(ok("x :: Double = (+) 1.0 2.0;\nmain = 0.0;"));
    assert!(!ok("x :: Double = (+) 1.0 2.0 3.0;\nmain = 0.0;"));
}

#[test]
fn a_routed_site_keeps_its_arity_wording() {
    // Routing a site through the one application rule must not change what a
    // user reads. Each message below is the wording the site had before the
    // refactor: the shared function supplies only the subject (`what`).
    let msg = |src: &str| match infer_err(src).expect("the program must be rejected") {
        rill_lang::CompileError::Type { msg, .. } => msg,
        other => panic!("expected a type error, got {other:?}"),
    };
    assert_eq!(
        msg("x :: Integer = fromInteger 1 2;\nmain = 0.0;"),
        "method `fromInteger` of `Num` expects 1 argument(s), got 2"
    );
    // The runtime-application path (`(\y -> y) 1.0 2.0`) names the EXPRESSION.
    assert_eq!(
        msg("x :: Double = (\\y -> y) 1.0 2.0;\nmain = 0.0;"),
        "expression expects 1 argument(s), got 2"
    );
    // A source-instance method: the operator form's subject is the method.
    assert_eq!(
        msg("x :: Double = (<) 1.0 2.0 3.0;\nmain = 0.0;"),
        "method `<` of `Ord` expects 2 argument(s), got 3"
    );
    // A source-instance method under its own name, over-applied.
    assert_eq!(
        msg("typeclass Add a where { add: a -> a -> a; }\n\
             instance Add Double where { add x y = x + y; }\n\
             x :: Double = add 1.0 2.0 3.0;\nmain = 0.0;"),
        "method `add` of `Add` expects 2 argument(s), got 3"
    );
    // And a two-parameter source method is curried: a partial application is a
    // function value, not an error (the property the drifted copies lost).
    let hs = "\
        typeclass Add a where { add: a -> a -> a; }\n\
        instance Add Double where { add x y = x + y; }\n";
    assert!(
        infer_err(&format!(
            "{hs}h :: Double -> Double;\nh = add 1.0;\nmain = 0.0;"
        ))
        .is_none(),
        "a partial application of a source method must be a function value"
    );
    assert!(
        infer_err(&format!("{hs}x :: Double = add 1.0 2.0;\nmain = 0.0;")).is_none(),
        "a full application of a source method must yield its result"
    );
}

#[test]
fn a_forall_signature_reports_its_real_arity() {
    // `sig_arg_count` looks through `forall`, so the parameter list the
    // application rule compares against must too. Before the rule was shared,
    // `signature_param_and_result_tys_conv` did not strip the quantifier, and
    // the two disagreed: the arity message said 1 for a 2-argument method.
    let src = "\
        typeclass C a where { f: forall b. a -> b -> a; }\n\
        instance C Int where { f x y = x; }\n\
        x :: Int = f 1 2 3;\n\
        main = 0.0;";
    match infer_err(src).expect("over-applying a 2-argument method must be rejected") {
        rill_lang::CompileError::Type { msg, .. } => assert_eq!(
            msg, "method `f` of `C` expects 2 argument(s), got 3",
            "the reported arity must be the signature's, through the quantifier"
        ),
        other => panic!("expected a type error, got {other:?}"),
    }
    // A partial application of the same quantified method is a function value.
    let partial = "\
        typeclass C a where { f: forall b. a -> b -> a; }\n\
        instance C Int where { f x y = x; }\n\
        h :: Int -> Int;\n\
        h = f 1;\n\
        main = 0.0;";
    assert!(
        infer_err(partial).is_none(),
        "`f 1` at `Int -> Int` must type as the curried remainder"
    );
}
