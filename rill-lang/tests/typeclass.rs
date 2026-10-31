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
    // static type must pick the right one.
    //
    // The argument is PINNED (`1 :: Int`). A bare `1` would not select anything:
    // a numeric literal is `fromInteger (n :: Integer)` (Haskell 2010 §6.4.1),
    // so the class variable is constrained only by `Num` and no instance is
    // determined. GHC 9.10.3 rejects the unpinned call with `Ambiguous type
    // variable 'a0'` — it does not default here, because `Probe` is not one of
    // the Report's standard classes (§4.3.4) — and rill must reject it too
    // (OD-47/OD-33). The pinned form is the legal Haskell module, and it is what
    // measures the claim this test is about.
    let src = r#"
        typeclass Probe a where { probe: a -> Double; }
        instance Probe Double where { probe _f = 1.5; }
        instance Probe Int where { probe _f = 2.5; }
        main = probe (1 :: Int);
    "#;
    let prog = run(src);
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Double(2.5));

    // The unpinned call is genuinely ambiguous and must NOT compile — the
    // divergence OD-47 recorded was this acceptance, not a wrong value.
    let ambiguous = r#"
        typeclass Probe a where { probe: a -> Double; }
        instance Probe Double where { probe _f = 1.5; }
        instance Probe Int where { probe _f = 2.5; }
        main = probe 1;
    "#;
    let (defs, expr) = ambiguous
        .rsplit_once("main = ")
        .expect("the source must bind `main`");
    match eval_ffi(
        defs,
        expr.trim().trim_end_matches(';'),
        ForeignRegistry::<f32>::new(),
    ) {
        Ok(_) => panic!("an open class variable must not be defaulted to an instance"),
        Err(e) => assert!(
            e.to_string().contains("Ambiguous type variable"),
            "expected an ambiguity diagnostic, got: {e}"
        ),
    }
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
        main = a (1.0 :: Double);
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
    let mut prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
    rill_lang::desugar::desugar_program(&mut prog).expect("desugar");
    rill_lang::types::infer::infer_program(&prog, Mode::Complete)
        .unwrap_or_else(|e| panic!("`{src}` must typecheck: {e}"))
}

/// The diagnostic for a program inference REJECTS, or `None` when it accepts it.
fn infer_err(src: &str) -> Option<rill_lang::CompileError> {
    let toks = rill_lang::lexer::tokenize(src).expect("lex");
    let mut prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
    rill_lang::desugar::desugar_program(&mut prog).expect("desugar");
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
    // the assertion is about the inferred SCHEME, and running the program would
    // measure the kernel instead.
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

#[test]
fn integral_division_satisfies_the_report_sign_laws() {
    // Report §6.4.2: `quot` truncates toward ZERO and `div` toward NEGATIVE
    // INFINITY, so `rem` takes the dividend's sign and `mod` the divisor's.
    // Every value below is GHC 9.10.3's, measured:
    //   (3, 1, 3, 1, -3, -1, -4, 1)
    let run_num = |src: &str| {
        let (defs, expr) = src.rsplit_once("main = ").expect("the source binds `main`");
        let world = eval_ffi(
            defs,
            expr.trim().trim_end_matches(';'),
            ForeignRegistry::<f32>::new(),
        )
        .unwrap_or_else(|e| panic!("`{src}` must compile: {e}"));
        match world.arena().get(world.result().unwrap()).unwrap() {
            rill_lang::arena::Value::Int(v) => *v,
            other => panic!("expected an Int, got {other:?}"),
        }
    };
    for (expr, want) in [
        ("quot (7 :: Int) 2", 3),
        ("rem (7 :: Int) 2", 1),
        ("div (7 :: Int) 2", 3),
        ("mod (7 :: Int) 2", 1),
        ("quot (-7 :: Int) 2", -3),
        ("rem (-7 :: Int) 2", -1),
        ("div (-7 :: Int) 2", -4),
        ("mod (-7 :: Int) 2", 1),
    ] {
        assert_eq!(run_num(&format!("main = {expr};")), want, "`{expr}`");
    }
    // The Report law holds on both pairs.
    for (a, b) in [(7i64, 2i64), (-7, 2), (7, -2), (-7, -2)] {
        let q = run_num(&format!("main = quot ({a} :: Int) {b};"));
        let r = run_num(&format!("main = rem ({a} :: Int) {b};"));
        assert_eq!(
            q * b + r,
            a,
            "`({a} quot {b}) * {b} + ({a} rem {b}) == {a}`"
        );
        let d = run_num(&format!("main = div ({a} :: Int) {b};"));
        let m = run_num(&format!("main = mod ({a} :: Int) {b};"));
        assert_eq!(d * b + m, a, "`({a} div {b}) * {b} + ({a} mod {b}) == {a}`");
    }
    // `Integral` is a real class, and `Double` is NOT in it (Report Table 6.1).
    let infer = |src: &str| {
        let toks = rill_lang::lexer::tokenize(src).expect("lex");
        let mut prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
        rill_lang::desugar::desugar_program(&mut prog).expect("desugar");
        rill_lang::types::infer::infer_program(&prog, Mode::Strict)
    };
    assert!(
        infer("f :: Integral a => a -> a -> a;\nf x y = quot x y;\nmain = 0.0;").is_ok(),
        "`Integral a =>` must supply `quot`"
    );
    // KNOWN GAP (OD-55): `quot` at `Double` is ACCEPTED where GHC rejects it
    // (`No instance for Integral Double`). The dispatch resolves the four
    // operators from the LITERAL operands and never re-checks the RESULT's type,
    // so a `Double` position is not rejected. Asserted as the current behaviour
    // so the suite is honest; this assertion FLIPS when OD-55 closes.
    assert!(
        infer("x :: Double\nx = quot 7 2\nmain = 0.0;").is_ok(),
        "OD-55: `quot` at `Double` is currently accepted; closing the gap flips this"
    );
    // `toInteger` widens exactly.
    let to_int = |src: &str| {
        let (defs, expr) = src.rsplit_once("main = ").expect("the source binds `main`");
        let world = eval_ffi(
            defs,
            expr.trim().trim_end_matches(';'),
            ForeignRegistry::<f32>::new(),
        )
        .unwrap_or_else(|e| panic!("`{src}` must compile: {e}"));
        match world.arena().get(world.result().unwrap()).unwrap() {
            rill_lang::arena::Value::Integer(v) => v.clone(),
            other => panic!("expected an Integer, got {other:?}"),
        }
    };
    assert_eq!(to_int("main = toInteger (5 :: Int);"), 5.into());
}

#[test]
fn a_signature_context_restricts_its_body() {
    // Haskell 2010 §4.1.3: a type signature's context RESTRICTS the body. Every
    // expectation below is GHC 9.10.3's measured behaviour.
    let infer = |src: &str| {
        let toks = rill_lang::lexer::tokenize(src).expect("lex");
        let mut prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("parse");
        rill_lang::desugar::desugar_program(&mut prog).expect("desugar");
        rill_lang::types::infer::infer_program(&prog, Mode::Strict)
    };
    // REJECTED: `(<)` needs `Ord`, which no context supplies.
    for (why, src) in [
        (
            "no context at all",
            "f :: a -> a -> Bool;\nf x y = x < y;\nmain = 0.0;",
        ),
        (
            "`Num` does not entail `Ord`",
            "f :: Num a => a -> Bool;\nf x = x < x;\nmain = 0.0;",
        ),
        (
            "`Eq` does not entail `Ord`",
            "f :: Eq a => a -> Bool;\nf x = x < x;\nmain = 0.0;",
        ),
    ] {
        let err = match infer(src) {
            Ok(_) => panic!("`{src}` must be rejected ({why})"),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains("no witness for `Ord`"),
            "expected a context-witness error ({why}), got: {err}"
        );
    }
    // ACCEPTED: the context supplies it, directly or through a superclass.
    for (why, src) in [
        (
            "`Ord a` is exactly what `(<)` needs",
            "f :: Ord a => a -> a -> Bool;\nf x y = x < y;\nmain = 0.0;",
        ),
        (
            "`Ord` entails `Eq`",
            "f :: Ord a => a -> Bool;\nf x = x == x;\nmain = 0.0;",
        ),
        (
            "`Num a` is what `(+)` needs",
            "f :: Num a => a -> a -> a;\nf x y = x + y;\nmain = 0.0;",
        ),
        (
            "a CONCRETE type needs no witness",
            "f :: Double -> Double -> Bool;\nf x y = x < y;\nmain = 0.0;",
        ),
        (
            "an UNSIGNED binding's context is inferred, not violated",
            "f (Just x) | x > 2.0 = 10.0 | otherwise = 0.0;\nf Nothing = 99.0;\nmain = 0.0;",
        ),
    ] {
        assert!(
            infer(src).is_ok(),
            "`{src}` must be accepted ({why}): {:?}",
            infer(src).err().map(|e| e.to_string())
        );
    }
}

#[test]
fn an_open_expectation_defers_instead_of_failing() {
    // A result-directed method whose expected RESULT is still a type VARIABLE is
    // the ordinary mid-inference state, not an error (Haskell 2010 §4.3.4). GHC
    // measures `fromInteger 2 + 0.5` as Core
    //   `(+ @Double $fNumDouble (fromInteger @Double $fNumDouble (IS 2#)) (D# 0.5##))`
    // — ONE `a` shared by `(+)`, `Num a` and `Fractional a`, decided by
    // DEFAULTING at the group's close. The apply path used to demand a CONCRETE
    // expectation and reject the program; the bare-reference path already
    // deferred, which is why `2 + 0.5` worked while `fromInteger 2 + 0.5` did not.
    let run_num = |src: &str| {
        let (defs, expr) = src
            .rsplit_once("main = ")
            .expect("the source must bind `main`");
        let world = eval_ffi(
            defs,
            expr.trim().trim_end_matches(';'),
            ForeignRegistry::<f32>::new(),
        )
        .unwrap_or_else(|e| panic!("`{src}` must typecheck: {e}"));
        match world.arena().get(world.result().unwrap()).unwrap() {
            rill_lang::arena::Value::Double(v) => format!("Double({v})"),
            rill_lang::arena::Value::Integer(v) => format!("Integer({v})"),
            rill_lang::arena::Value::Int(v) => format!("Int({v})"),
            other => panic!("expected a number, got {other:?}"),
        }
    };
    // One `a` for both operands: `0.5` forces `Fractional`, so `a := Double`.
    assert_eq!(run_num("main = fromInteger 2 + 0.5;"), "Double(2.5)");
    assert_eq!(run_num("main = 0.5 + fromInteger 2;"), "Double(2.5)");
    // The argument may be any `Integer`, not only a literal.
    assert_eq!(
        run_num("k :: Integer\nk = 2\nmain = fromInteger k + 0.5;"),
        "Double(2.5)"
    );
    // With NO fractional operand the same judgement defaults to `Integer`.
    assert_eq!(
        run_num("main = fromInteger (2 :: Integer) + fromInteger (1 :: Integer);"),
        "Integer(3)"
    );
    // A USER class is still not defaultable (§4.3.4's standard-class rule):
    // GHC rejects `z + 0.5` for `z :: Z a => a`, and so must rill.
    let user =
        "typeclass Z a where { z: a; }\ninstance Z Double where { z = 1.5; }\nmain = z + 0.5;";
    let (defs, expr) = user.rsplit_once("main = ").unwrap();
    let err = match eval_ffi(
        defs,
        expr.trim().trim_end_matches(';'),
        ForeignRegistry::<f32>::new(),
    ) {
        Ok(_) => panic!("a non-standard class variable must not be defaulted"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("Ambiguous")
            || err.to_string().contains("expected type unknown")
            || err.to_string().contains("expected type is not concrete"),
        "expected an ambiguity diagnostic, got: {err}"
    );
}

#[test]
fn a_nullary_method_inside_a_buffer_operand_still_resolves() {
    // The shared application rule ran a bare inference with `?` first, so a
    // nullary argument that only types under the parameter's EXPECTED type
    // (`zero` — a result-directed method with nothing of its own to solve the
    // class variable) aborted the application before the fallback could run.
    // The defect was a COMPILE ERROR, so compilation succeeding is the
    // requirement here; the operator's own result type is checked too.
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_lang::register::register_foreign_generators(&mut ffi);
    let src = "typeclass Zero a where { zero: a; }\n\
               instance Zero (FixedBuffer Double) where { zero = sine 440.0 0.5 0.0; }\n\
               x :: Buffer = sine 440.0 0.5 0.0 + zero;\n\
               main = 0.0;";
    let ir = rill_lang::compile_eval::<f32, 256>(src, "x", Mode::Complete)
        .expect("a nullary method in a Buffer operand must resolve by the expected type");
    assert!(
        ir.dicts
            .iter()
            .any(|d| d.class == "Zero" && d.ty_key == "FixedBuffer Double"),
        "`zero` must select `Zero (FixedBuffer Double)`; table = {:?}",
        ir.dicts
            .iter()
            .map(|d| (d.class.clone(), d.ty_key.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_literal_does_not_widen_into_a_buffer_parameter() {
    // GHC 9.10.3: `((\b -> b) 1.0) :: Buffer` is a type error (`No instance for
    // Fractional (FixedBuffer Double)`). The shared rule used the operator path's
    // literal carve-out — a literal in a `Buffer` parameter position is that
    // buffer's constant — for EVERY application, so this compiled.
    match infer_err("main = ((\\b -> b) 1.0) :: Buffer;")
        .expect("a literal must not widen into a Buffer parameter")
    {
        rill_lang::CompileError::Type { msg, .. } => assert!(
            msg.contains("cannot unify"),
            "the rejection must be a unification failure, got: {msg}"
        ),
        other => panic!("expected a type error, got {other:?}"),
    }
    // The operator path's carve-out stays: a literal operand of a Buffer
    // operator IS that buffer's constant.
    assert!(
        infer_err("main = sine 440.0 0.5 0.0 * 0.5;").is_none(),
        "a literal operand of a Buffer operator must still be accepted"
    );
}

#[test]
fn a_literal_at_a_builtin_buffer_parameter_is_that_buffers_constant() {
    // Haskell 2010 §6.4.1 `fromInteger`-polymorphism: the literal takes the
    // type its position demands, and a foreign builtin's `Buffer` slot demands
    // a buffer — so `arrayFromBuffer 1` is the block's constant. GHC 9.10.3
    // accepts `arrayFromBuffer 1` at the FFI boundary the same way.
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_lang::register::register_foreign_generators(&mut ffi);
    let ir = rill_lang::compile_eval::<f32, 256>(
        "main = length (arrayFromBuffer 1);",
        "main",
        Mode::Strict,
    )
    .expect("a literal at a Buffer parameter is that buffer's constant");
    let mut world = WorldState::<f32, 256>::new(rill_lang::program::WorldConfig {
        ffi: Some(ffi),
        params: rill_core::Params::new(RATE),
        ..Default::default()
    });
    rill_lang::test_eval(&ir, &mut world).expect("the primitive must run");
    let r = world.result().expect("the program produced no result");
    assert_eq!(
        world.arena().get(r).expect("live"),
        &rill_lang::arena::Value::Int(256),
        "the buffer's constant fills the whole block"
    );
    // A NON-literal still does not widen into a `Buffer` parameter.
    assert!(
        infer_err("main = ((\\b -> b) 1.0) :: Buffer;").is_some(),
        "a non-literal must not widen into a Buffer parameter"
    );
}

#[test]
fn a_constructor_class_method_section_is_a_curried_value() {
    // GHC: `g = fmap (+ 1)` is `Functor f => f Integer -> f Integer`, and
    // `g (Just 1)` is `Just 2`. The constructor-class dispatch demanded its
    // exact arity, so a partial application was rejected even though the
    // missing argument is exactly the one that selects the instance.
    let src = r#"
        g = fmap (+ 1.0);
        main = case g (Just 1.0) of { Nothing -> 0.0; Just x -> x };
    "#;
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
    // The SAME binding is usable at another container: the instance is selected
    // at each use site, not at the binding.
    //
    // SCOPE OF THIS FIX — a known limitation, not a general solution: the
    // β-substitution in `lower_applied_method_binding_site` covers a
    // single-expression use and the FIRST statement of a `do` block ONLY. A
    // `do` block lowers as nested fragments, so a binding whose name is a
    // fragment capture is skipped — a `g = fmap (+ 1.0)` reached from the
    // second `do` statement or passed as a higher-order argument is still
    // rejected ("the argument type is not a constructor application") where
    // GHC 9.10.3 accepts it. That is the same gap as bare `g = fmap`: the
    // instance is chosen at the use site, so both need a runtime dictionary.
    // Recorded as an open defect; `a_method_binding_works_after_the_first_do_
    // statement` below pins the current failure.
    let src = r#"
        g = fmap (+ 1.0);
        main = length (g [1.0, 2.0]);
    "#;
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
#[ignore = "open defect: a method binding is beta-substituted only in a non-capture \
            position; needs a runtime dictionary. Remove this ignore when that lands."]
fn a_method_binding_works_after_the_first_do_statement() {
    // GHC 9.10.3 accepts all of these. rill reaches the rewrite only when the
    // binding is not a fragment capture, i.e. a single-expression use or the
    // first `do` statement.
    assert_eq!(
        run_out("g = fmap (+ 1.0);\nmain = do { write (Just (show_int 0)) stdout; write (Just (show_int (length (g [1.0, 2.0])))) stdout; return () };"),
        "02"
    );
}
