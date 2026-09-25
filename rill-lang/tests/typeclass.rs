//! Typeclass tests: `typeclass`/`instance` declare polymorphic methods that
//! resolve **at compile time** to the concrete instance's body. The method
//! argument's static type selects the instance; the body is β-substituted at
//! compile time (zero runtime dispatch — typeclasses and instances produce no
//! arena values).

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn typeclass_method_resolves_at_compile_time() {
    // `show: a` declares the method over the class variable. `show f = f` is
    // the Float instance's identity body: `show 1.0` resolves to it, binds
    // `f := 1.0`, and the value output is Float(1.0). If resolution did not
    // happen (or the param binding were lost), the call could not compile.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = f; }
        main = show 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn typeclass_selects_instance_by_argument_type() {
    // Two instances of the same class with different bodies: the argument's
    // static type must pick the right one. `show 1` (an Int literal) must
    // resolve to the Int instance's body (2.5), not the Float instance's (1.5).
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = 1.5; }
        instance Show Int where { show f = 2.5; }
        main = show 1;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(2.5));
}

#[test]
fn recursive_typeclass_method_is_compile_error() {
    // A method body that inlines itself must be a compile error, not a stack
    // overflow / SIGABRT of the compiler.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = show f; }
        main = show 1.0;
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.err().expect("compile failed") {
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
        instance C Float where { a f = b f; b f = a f; }
        main = a 1.0;
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.err().expect("compile failed") {
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
fn signal_method_argument_is_compile_error() {
    // A genuine signal computation (`sin 1.0`) cannot select a typeclass
    // instance — only value expressions (and bare Float/Int literals) can.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = f; }
        main = show (sin 1.0);
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "expected a compile error, got success");
    match res.err().expect("compile failed") {
        rill_lang::CompileError::Type { msg, .. } => {
            assert!(
                msg.contains("value expression"),
                "expected value-expression message, got: {msg}",
            );
        }
        other => panic!("expected a Type error, got {other:?}"),
    }
}

#[test]
fn invalid_instance_body_is_compile_error() {
    // An instance body is validated at compile time even when the instance is
    // never called — a signal-expression body must be rejected.
    let src = r#"
        typeclass Show a where { show: a; }
        instance Show Float where { show f = sin 1.0; }
        main = 1.0;
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "expected a compile error, got success");
}
