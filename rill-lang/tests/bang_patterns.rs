//! Bang patterns (`!x`): WHNF forcing at parameters, `let` bindings, and
//! `case` arm patterns.
//!
//! The observable used throughout is a lazy thunk that fails only when it is
//! actually forced: `countdown 2000.0` exhausts the pre-allocated call scratch
//! (a latched `ProcessError`, see `tests/lazy_mode.rs`), so a run that forces
//! it fails and a run that leaves it thunked succeeds.
pub mod common;

use common::*;
use rill_lang::mode::Mode;

/// A `countdown` definition that diverges by exhausting the call budget, plus
/// `tail` — the rest of the program follows in each test's source.
const DIVERGENT_DEF: &str = "countdown n = if n == 0.0 then 0.0 else countdown (n - 1.0)";

/// Compile and run `src` in complete (lazy) mode, returning the `main` value.
fn run(src: &str) -> Result<f64, String> {
    run_with(src, Mode::Complete)
}

/// Compile and run `src` in strict mode (the default), returning the `main`
/// value. Strict mode lowers a bang `let` through the inlined-def path, a
/// different code path from the complete-mode thunk.
fn run_strict(src: &str) -> Result<f64, String> {
    run_with(src, Mode::Strict)
}

fn run_with(src: &str, mode: Mode) -> Result<f64, String> {
    // Split the source at the `main` binding: everything before it is `defs`,
    // the `main` body is the evaluated expression (`test_eval` compiles the
    // body as a fresh entry and runs it once).
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_mode::<256>(defs, expr, ffi, mode).map_err(|e| e.to_string())?;
    let v = prog.result().ok_or("no value output")?;
    match prog.arena().get(v) {
        Some(rill_lang::arena::Value::Float(x)) => Ok(*x),
        other => Err(format!("expected Float, got {other:?}")),
    }
}

/// Complete-mode source with a `countdown` and a `main` body.
fn complete(main_body: &str) -> String {
    format!("{{-# MODE complete #-}}\n{DIVERGENT_DEF};\n{main_body}")
}

fn assert_forces_argument(src: &str) {
    let err = run(src).expect_err("the bang marker must force the thunk");
    assert!(
        err.contains("exhausted"),
        "expected a call-scratch exhaustion error, got: {err}"
    );
}

#[test]
fn bang_param_forces_argument() {
    let lazy = complete("f x = 1.0;\nmain = let bad = countdown 2000.0 in f bad");
    assert_eq!(
        run(&lazy).unwrap(),
        1.0,
        "an unused lazy argument is not forced"
    );
    let bang = complete("f !x = 1.0;\nmain = let bad = countdown 2000.0 in f bad");
    assert_forces_argument(&bang);
}

#[test]
fn bang_lambda_param_forces_argument() {
    // The divergent thunk is passed straight to the lambda: a lambda is a value
    // and applying it does not force its argument unless the parameter is bang.
    let lazy = complete("main = (\\x -> 1.0) (countdown 2000.0)");
    assert_eq!(run(&lazy).unwrap(), 1.0);
    let bang = complete("main = (\\ !x -> 1.0) (countdown 2000.0)");
    assert_forces_argument(&bang);
}

#[test]
fn bang_let_binding_forces_rhs() {
    let lazy = complete("main = let bad = countdown 2000.0 in 1.0");
    assert_eq!(run(&lazy).unwrap(), 1.0);
    let bang = complete("main = let !bad = countdown 2000.0 in 1.0");
    assert_forces_argument(&bang);
}

#[test]
fn strict_bang_let_binding_forces_rhs() {
    // Strict mode lowers `let` through the inlined-def branch of
    // `lower::Expr::Let`, not the complete-mode thunk: the binding-point force
    // must be emitted there too.
    assert_eq!(run_strict("main = let x = 2.0 in x + 1.0").unwrap(), 3.0);
    assert_eq!(run_strict("main = let !x = 2.0 in x + 1.0").unwrap(), 3.0);
    // The strict path emits exactly one force for the bang form (recursion is
    // rejected in strict mode, so divergence is not the observable here).
    assert_eq!(
        force_count("{-# MODE strict #-}\nmain = let !x = 2.0 in x + 1.0"),
        1
    );
    assert_eq!(
        force_count("{-# MODE strict #-}\nmain = let x = 2.0 in x + 1.0"),
        0
    );
}

#[test]
fn bang_let_rhs_is_forced_once_and_shared() {
    // The force is emitted ONCE at the binding point, and every use reads the
    // same forced register: `x + x` and `x + x + x` evaluate the right-hand side
    // exactly once. Pure code makes an evaluation COUNT unobservable at runtime,
    // so sharing is pinned two ways — the lowered IR holds exactly one force no
    // matter how many uses there are, and the runtime reads the shared value.
    assert_eq!(force_count("main = let !x = 2.0 in x + x"), 1);
    assert_eq!(
        force_count("main = let !x = 2.0 in x + x + x"),
        1,
        "an extra use must not add a force"
    );
    assert_eq!(run(&complete("main = let !x = 2.0 in x + x")).unwrap(), 4.0);
    assert_eq!(
        run(&complete("main = let !x = 2.0 in x + x + x")).unwrap(),
        6.0
    );
    // A shared, divergent right-hand side is forced once at the binding point:
    // the latched error surfaces even though the body never reads the name.
    assert_forces_argument(&complete("main = let !x = countdown 2000.0 in 1.0"));
    // The non-bang form stays lazy: neither use forces it.
    assert_eq!(
        run(&complete("main = let x = countdown 2000.0 in 1.0")).unwrap(),
        1.0
    );
}

#[test]
fn recursive_bang_binding_is_rejected() {
    // A bang binding forces its right-hand side at the binding point, so the
    // right-hand side cannot be the binding itself. Rejected with a
    // bang-specific diagnostic instead of a confusing "unknown `x`" (complete)
    // or "recursive value definition" (strict) error.
    for mode in [Mode::Complete, Mode::Strict] {
        let err = run_with("main = let !x = x in 1.0", mode)
            .expect_err("a recursive bang binding must be rejected");
        assert!(
            err.contains("bang binding cannot be recursive"),
            "expected a recursive-bang diagnostic in {mode:?}, got: {err}"
        );
    }
    // The lazy (non-bang) recursive binding is still accepted.
    assert_eq!(run(&complete("main = let x = x in 1.0")).unwrap(), 1.0);
}

#[test]
fn bang_case_pattern_forces_payload() {
    // The statically-known constructor selects the arm at compile time and the
    // case scrutinee is not forced on that path — only the bang pattern forces
    // the payload.
    let lazy = complete(
        "main = let bad = countdown 2000.0 in case Just bad of { Just x -> 1.0; Nothing -> 0.0 }",
    );
    assert_eq!(run(&lazy).unwrap(), 1.0);
    let bang = complete(
        "main = let bad = countdown 2000.0 in case Just bad of { Just !x -> 1.0; Nothing -> 0.0 }",
    );
    assert_forces_argument(&bang);
}

#[test]
fn bang_bindings_keep_their_values() {
    // Forcing must not disturb the bound value itself (WHNF, no deep copy).
    assert_eq!(run("f !x = x + 1.0;\nmain = f 2.0").unwrap(), 3.0);
    assert_eq!(run("main = (\\ !x -> x + 1.0) 2.0").unwrap(), 3.0);
    assert_eq!(run("main = let !x = 2.0 in x + 1.0").unwrap(), 3.0);
    assert_eq!(
        run("main = case Just 2.0 of { Just !x -> x + 1.0; Nothing -> 0.0 }").unwrap(),
        3.0
    );
    // A bang sub-pattern (`Just !x`) binds through the force as well.
    assert_eq!(
        run("main = case Just (Just 2.0) of { Just (Just !x) -> x + 1.0; _ -> 0.0 }").unwrap(),
        3.0
    );
}

/// Count the `ValueForce` instructions in a lowered program, fragments included.
fn value_force_count(ir: &rill_lang::ir::Ir) -> usize {
    ir.value_blocks
        .iter()
        .chain(ir.fragments.iter().flat_map(|f| &f.value_blocks))
        .flat_map(|b| &b.instrs)
        .filter(|i| matches!(i, rill_lang::ir::ValueInstr::ValueForce { .. }))
        .count()
}

fn force_count(src: &str) -> usize {
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, &typed.cafs).unwrap();
    value_force_count(&ir)
}

#[test]
fn bang_markers_lower_to_value_force() {
    // Each bang form emits the existing WHNF `ValueForce`; the unmarked forms
    // emit none.
    assert_eq!(force_count("f x = x + 1.0;\nmain = f 2.0"), 0);
    assert_eq!(force_count("f !x = x + 1.0;\nmain = f 2.0"), 1);
    assert_eq!(force_count("main = (\\ !x -> x + 1.0) 2.0"), 1);
    assert_eq!(force_count("main = let !x = 2.0 in x + 1.0"), 1);
    assert_eq!(
        force_count("main = case Just 2.0 of { Just !x -> x + 1.0; Nothing -> 0.0 }"),
        1
    );
}

#[test]
fn the_bang_marker_and_the_infix_index_operator_stay_distinct() {
    // `!x` in a PATTERN is a strictness marker; `a ! i` in an EXPRESSION is the
    // array accessor. Both appear in the same definition, so a parser or
    // lowering pass that confused the two would either lose the force or fail
    // to index.
    //
    // The SIGNATURE is what makes this a regression test. A signature-typed
    // bang parameter is only typed on the closure path: inlining the body
    // discards the parameter types and leaves the scalar numeric default, so
    // `a` was typed `Float` and `a ! i` asked for `Ix Float`. Keeping the
    // definition un-inlined (it must stay a closure anyway, because a bang
    // parameter forces its argument on entry) is what carries
    // `Array Int Float` into the body.
    let src = "{-# MODE strict #-}\n\
               at !a i :: Array Int Float -> Int -> Float = a ! i;\n\
               main = at (listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0]) 1";
    assert_eq!(run_strict(src).unwrap(), 20.0);
    // The same definition in the lazy mode, where the call goes through the
    // closure fragment instead of the strict inline path.
    assert_eq!(
        run(
            "{-# MODE complete #-}\nat !a i :: Array Int Float -> Int -> Float = a ! i;\n\
             main = at (listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0]) 2"
        )
        .unwrap(),
        30.0
    );
    // The parameter types must reach the body WITHOUT dropping the force: the
    // sig-typed definition still lowers to exactly one WHNF force.
    assert_eq!(
        force_count(
            "at !a i :: Array Int Float -> Int -> Float = a ! i;\n\
             main = at (listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0]) 1"
        ),
        1,
        "the signature must not cost the bang parameter its force"
    );
    // A non-array witness of the same class: a signature-typed bang parameter
    // whose body projects a record field. Inlining again lost the `Pair` type.
    assert_eq!(
        run(
            "f !p q :: Pair Int Float -> Float -> Float = p.second + q;\n\
             main = f (Pair { first: 1, second: 2.0 }) 3.0"
        )
        .unwrap(),
        5.0
    );
}
