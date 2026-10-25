//! Constrained (`C v => T`) definitions: the signature drives parameter typing,
//! the body is NOT inferred eagerly, and each call site specializes the body
//! with the concrete class-variable type and the bound instance — β-substitution
//! at compile time, never a runtime dictionary.
//!
//! The probe class `ProbeIx` is a purpose-built stand-in that stays OUT of the
//! prelude's way: the real `Ix` class (with `range`/`index`/`inRange`/
//! `rangeSize`) now ships in `CATEGORY_PRELUDE` with its `Int` and
//! `Pair Int Int` instances, so a probe class of that name (or a probe method
//! named `rangeSize`) would collide with real machinery — and a method name
//! declared by two classes is deliberately ambiguous. Its method takes a
//! `Pair`-shaped index, so the class variable is only recoverable by unifying
//! the method's parameter pattern (`Pair i i`) against the concrete index
//! type.
pub mod common;

use common::*;
use rill_lang::ast::TypeExpr;
use rill_lang::compile_eval;
use rill_lang::mode::Mode;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::TypeEnv;

/// Probe index class: `Int` (1-D) and `Pair Int Int` (2-D) instances. Each
/// instance's `probeSize` returns a distinct marker so the test can tell which
/// one a call site specialized to.
const PROBE_IX: &str = r#"
    typeclass ProbeIx i where { probeSize: Pair i i -> Float; }
    instance ProbeIx Int where { probeSize b = 1.0; }
    instance ProbeIx (Pair Int Int) where { probeSize b = 2.0; }
"#;

/// A constrained definition whose body calls the `ProbeIx` method: it can only be
/// typed AFTER a call site solves `i`.
const PROBE_DEF: &str = "mk p v :: ProbeIx i => Pair i i -> Float -> Float = probeSize p;\n";

/// Infer `src` and return its type environment.
fn env_of(src: &str) -> TypeEnv {
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    infer_program(&prog, Mode::Strict).unwrap().type_env
}

/// Compile and evaluate the `main` expression of `src` in a single run,
/// returning its Float value.
fn run(src: &str) -> f64 {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    match prog.arena().get(v) {
        Some(rill_lang::arena::Value::Float(x)) => *x,
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn constrained_def_registers_without_eager_body_inference() {
    // The body `probeSize p` is only dispatchable once a call site pins the
    // class variable: eagerly typing it would bind `p` to `Float` and fail
    // (`no instance of ProbeIx for Float`). A successful load therefore proves the
    // body was DEFERRED, not inferred.
    let src = format!("{PROBE_IX}\n{PROBE_DEF}\nmain = mk (0,9) 0.0;");
    let env = env_of(&src);
    let cd = env
        .constrained_defs
        .get("mk")
        .expect("a `=>` definition must register in `constrained_defs`");
    assert!(
        matches!(cd.sig, TypeExpr::TConstrained(_, _)),
        "the registered signature keeps its class context: {:?}",
        cd.sig
    );
    assert_eq!(cd.params.len(), 2, "both λ-parameters are registered");
}

#[test]
fn unconstrained_defs_do_not_register_as_constrained() {
    // The registry starts out holding the STANDARD PRELUDE's constrained array
    // surface (`listArray`, `newArray`, `ixmap`, …) — those are `C v => T`
    // definitions shipping with the language. The assertion here is that the
    // USER's plain def did not join them.
    let src = "g x = x + 1.0;\nmain = g 1.0;";
    let env = env_of(src);
    assert!(
        !env.constrained_defs.contains_key("g"),
        "a plain signature is inferred eagerly and must not be deferred; present: {:?}",
        env.constrained_defs.keys().collect::<Vec<_>>()
    );
    assert!(
        env.constrained_defs.contains_key("listArray"),
        "the standard prelude's array surface is registered as constrained"
    );
}

#[test]
fn call_site_specializes_the_body_to_the_index_instance() {
    let one_d = format!("{PROBE_IX}\n{PROBE_DEF}\nmain = mk (0,9) 0.0;");
    assert_eq!(
        run(&one_d),
        1.0,
        "`Pair Int Int` must solve `i := Int` and take the `ProbeIx Int` body"
    );

    let two_d = format!("{PROBE_IX}\n{PROBE_DEF}\nmain = mk ((0,0),(2,2)) 0.0;");
    assert_eq!(
        run(&two_d),
        2.0,
        "a nested `Pair` index must solve `i := Pair Int Int` and take its body"
    );
}

#[test]
fn every_call_site_specializes_independently() {
    // The SAME definition applied at the two index types in one program: a
    // compile-time dispatch is per call site, never a shared (first-call) body.
    let src = format!("{PROBE_IX}\n{PROBE_DEF}\nmain = mk (0,9) 0.0 + mk ((0,0),(2,2)) 0.0;");
    assert_eq!(run(&src), 3.0);
}

#[test]
fn non_instanced_index_type_is_a_compile_error() {
    // `Bool` is a legal index TYPE, but no `ProbeIx Bool` instance is registered:
    // the constraint cannot be discharged and must fail at COMPILE time
    // (never fall back to a runtime dictionary).
    let src = format!("{PROBE_IX}\n{PROBE_DEF}\nmain = mk (true,false) 0.0;");
    let err = match compile_eval::<f32, 256>(&src, "main", Mode::Strict) {
        Ok(_) => panic!("`ProbeIx Bool` has no instance, so the program must not compile"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("no instance of `ProbeIx` for type `Bool`"),
        "expected a missing-instance error, got: {msg}"
    );
}

#[test]
fn unconstrained_defs_are_still_inferred_eagerly() {
    // Deferral is only for constrained signatures: an ordinary def with a
    // garbage body must still fail at load, not silently compile.
    let err = match compile_eval::<f32, 256>("g x = missing x;\nmain = 1.0;", "main", Mode::Strict)
    {
        Ok(_) => panic!("an unconstrained body is still type-checked eagerly"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("unknown identifier `missing`"),
        "expected the eager body error, got: {msg}"
    );
}

#[test]
fn partial_application_specializes_and_applies_later() {
    // `mk` applied to one of its two arguments must still specialize `i` from
    // that argument, and the returned closure must dispatch the body against
    // the same instance when the second argument arrives (the class variable is
    // solved at the FIRST call site, not at the closure's invocation).
    //
    // Partial application is bound at top level: a `let`-bound function value
    // is not yet supported by the v1 lowering (same for plain definitions).
    let src = format!("{PROBE_IX}\n{PROBE_DEF}\np1 = mk (0,9);\nmain = p1 0.0;");
    assert_eq!(run(&src), 1.0);

    let src_2d = format!("{PROBE_IX}\n{PROBE_DEF}\np1 = mk ((0,0),(2,2));\nmain = p1 0.0;");
    assert_eq!(run(&src_2d), 2.0);
}

#[test]
fn where_block_of_a_constrained_def_is_in_scope() {
    // A `where`-binding of the definition is visible in the (deferred) body and
    // is inlined at its use site — the same scoping plain definitions have.
    let src = format!(
        "{PROBE_IX}\nmk p v :: ProbeIx i => Pair i i -> Float -> Float = probeSize p + off where {{ off = 0.5; }};\nmain = mk (0,9) 0.0;"
    );
    assert_eq!(run(&src), 1.5);
}

/// Compile `src`, expecting an error; return its rendered message.
fn compile_err(src: &str) -> String {
    match compile_eval::<f32, 256>(src, "main", Mode::Strict) {
        Ok(_) => panic!("expected a compile error, but the program compiled"),
        Err(e) => format!("{e}"),
    }
}

#[test]
fn constrained_def_can_call_another_constrained_def() {
    // The `listArray` → index-helper chain shape: an outer constrained
    // definition delegates to an inner one. Each call site specializes BOTH,
    // and the class variable solved by the outermost argument flows through the
    // inner call (its argument is the outer definition's `Pair i i` parameter).
    let src = format!(
        "{PROBE_IX}\n\
         inner p :: ProbeIx i => Pair i i -> Float = probeSize p;\n\
         outer q :: ProbeIx i => Pair i i -> Float = inner q;\n\
         main = outer (0,9) + outer ((0,0),(2,2));"
    );
    assert_eq!(run(&src), 3.0);
}

#[test]
fn specialized_body_result_must_match_the_signature() {
    // The body is deferred, so the signature's declared result is the only
    // contract it is held to. A specialization that produces another type is a
    // compile error, not a silently mistyped call site.
    let src =
        format!("{PROBE_IX}\nmk p :: ProbeIx i => Pair i i -> Float = \"nope\";\nmain = mk (0,9);");
    let msg = compile_err(&src);
    assert!(
        msg.contains("body of `mk` has type `String`, but its signature declares `Float`"),
        "expected a body/signature result mismatch, got: {msg}"
    );
}

#[test]
fn recursive_constrained_def_is_a_compile_error() {
    // A recursive call's class variable is only solved by the enclosing call
    // site — the very expansion in flight. Specializing it would not terminate,
    // so it is diagnosed. Direct self-recursion is caught by the general
    // fragment-recursion check, which runs before lowering.
    let src =
        format!("{PROBE_IX}\nmk p :: ProbeIx i => Pair i i -> Float = mk p;\nmain = mk (0,9);");
    let msg = compile_err(&src);
    assert!(
        msg.contains("recursive function call: `mk` calls itself"),
        "expected the recursion diagnostic, got: {msg}"
    );
}

#[test]
fn self_recursion_through_a_where_binding_is_a_compile_error() {
    // A `where` binding that calls back into the definition it belongs to is
    // recursion, and the class variable of the recursive call is only solved by
    // the enclosing call site — the very expansion in flight. The `where`
    // clause is desugared to an `Expr::Let` INSIDE the definition body (so its
    // bindings see the definition's parameters), which puts the back-reference
    // on the ordinary fragment call graph: the general recursion check now
    // reports it directly instead of leaving it to lowering's specialization
    // guard. Either way the program must not compile.
    let src = format!(
        "{PROBE_IX}\nmk p :: ProbeIx i => Pair i i -> Float = g p where {{ g x = mk x; }};\nmain = mk (0,9);"
    );
    let msg = compile_err(&src);
    assert!(
        msg.contains("recursive"),
        "expected a recursion diagnostic, got: {msg}"
    );
    assert!(
        msg.contains('`') && msg.contains("mk`"),
        "the diagnostic must name the recursing definition `mk`, got: {msg}"
    );
}

#[test]
fn bare_reference_to_a_constrained_def_is_a_compile_error() {
    // `f = mk` has no call site at which `i` could be solved. Reporting that
    // directly beats lowering the deferred body against an unsolved variable
    // and surfacing a misleading `no instance of ProbeIx for Float`.
    let src = format!("{PROBE_IX}\n{PROBE_DEF}\nf = mk;\nmain = f (0,9) 0.0;");
    let msg = compile_err(&src);
    assert!(
        msg.contains("cannot use class-constrained definition `mk` without applying it"),
        "expected the bare-reference diagnostic, got: {msg}"
    );
}
