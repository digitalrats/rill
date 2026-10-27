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
