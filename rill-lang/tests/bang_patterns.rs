//! Bang patterns (`!x`): WHNF forcing at parameters, `let` bindings, and
//! `case` arm patterns.
//!
//! The observable used throughout is a lazy thunk that fails only when it is
//! actually forced: `countdown 2000.0` exhausts the pre-allocated call scratch
//! (a latched `ProcessError`, see `tests/lazy_mode.rs`), so a run that forces
//! it fails and a run that leaves it thunked succeeds.

use rill_lang::mode::Mode;

/// A `countdown` definition that diverges by exhausting the call budget, plus
/// `tail` — the rest of the program follows in each test's source.
const DIVERGENT_DEF: &str = "countdown n = if n == 0.0 then 0.0 else countdown (n - 1.0)";

/// Compile and run `src` in complete (lazy) mode, returning the `main` value.
fn run(src: &str) -> Result<f64, String> {
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, Mode::Complete).map_err(|e| e.to_string())?;
    prog.run_value().map_err(|e| e.to_string())?;
    let v = prog.value_outputs()[0].ok_or("no value output")?;
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
    let ir = rill_lang::lower::lower_with_cafs(&typed, 44100.0, &typed.cafs).unwrap();
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
