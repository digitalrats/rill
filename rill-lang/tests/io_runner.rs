//! Task 2: the new `ProgramRunner` executor — `main : IO ()` runs to
//! completion against the realized `World`, with captured `stdout`.

use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program_runner::ProgramRunner;

fn runner(src: &str) -> ProgramRunner<f32, 256> {
    compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"))
}

#[test]
fn no_io_main_runs_to_completion() {
    // An IO action with no observable output still evaluates to completion.
    let mut r = runner("main = do { write Nothing stdout; return () };");
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), [] as [String; 0]);
}

#[test]
fn memory_stream_roundtrip() {
    // fromList builds a memory input stream; toList drains it back to a List;
    // show_double renders each element and fold concatenates them.
    let mut r = runner(
        r#"
main = do {
    xs <- fromList [1.0, 2.0, 3.0];
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_double x)) "" ys)) stdout;
    return ()
};
"#,
    );
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["1.02.03.0"]);
}

#[test]
fn io_effects_are_ordered() {
    // (a) two writes in a do-block appear in source order.
    let mut r =
        runner(r#"main = do { write (Just "a") stdout; write (Just "b") stdout; return () };"#);
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["a", "b"]);

    // (b) the same IO action bound under `let` and used twice fires twice, in
    // program order.
    let mut r = runner(r#"main = let a = write (Just "x") stdout in do { a; a };"#);
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["x", "x"]);
}

#[test]
fn bounded_memory_overflow_is_a_clean_error() {
    // An unbounded lazy list is drained by fromList: the fixed arena budget is
    // exhausted and the runner reports a clean CAPACITY error instead of
    // panicking. The assertion matches the capacity wording, so any other
    // failure path (a fault, a missing `main`, an internal invariant) does not
    // satisfy it.
    let mut r = runner(
        r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
main = do { xs <- fromList (from 0.0); return () };
",
    );
    let err = r.run().expect_err("unbounded drain must fail cleanly");
    let message = err.to_string();
    // The fixed arena is a hard budget: the interpreter latches a
    // `value-track resource exhausted: …` error (whichever resource trips
    // first — the arena, the payload-buffer pool, or the thunk chain that
    // depends on them) rather than panicking or looping forever.
    assert!(
        message.contains("resource exhausted"),
        "expected a resource-exhaustion error, got: {message}"
    );
}

#[test]
fn non_void_main_result_is_a_compile_error() {
    // `main : IO ()` constrains the ACTION's result, not just the action: an
    // `IO Double` main is malformed and is rejected by the compile-time gate on
    // the compile entry (`ir.main_io_unit`), before any world exists — not
    // latched at run time. (`main = return 1.0` on its own does not compile —
    // the monad is unconstrained — so the result type is fixed by an effect in
    // the block.)
    for src in [
        r#"main = do { write (Just "a") stdout; return 1.0 };"#,
        // The value track erases `()` to `Double(0.0)` inside a closure cell,
        // so this case pins the static-type distinction: the block is typed
        // `IO Double`, hence a 0.0 payload is NOT accepted as unit.
        r#"main = do { write (Just "a") stdout; return 0.0 };"#,
        r#"main = do { write (Just "a") stdout; return "s" };"#,
    ] {
        let err = match compile::<f32, 256>(src, ForeignRegistry::new()) {
            Ok(_) => panic!("a non-`()` main result must be rejected"),
            Err(e) => e,
        };
        let message = err.to_string();
        assert!(
            message.contains("IO ()"),
            "expected the `IO ()` contract error, got: {message}\nsource:\n{src}"
        );
    }
}

#[test]
fn void_main_result_succeeds() {
    // `return ()` travels through the value track's cell encoding (which
    // represents unit as Double(0.0)); `main`'s static `IO ()` type accepts it.
    let mut r = runner(r#"main = do { write (Just "a") stdout; return () };"#);
    r.run()
        .expect("`main` returning `()` must run to completion");
    assert_eq!(r.stdout(), ["a"]);

    // A program whose final action is an effect still succeeds: the effect's
    // result is `()` directly.
    let mut r = runner(r#"main = do { write (Just "ok") stdout };"#);
    r.run()
        .expect("an effect-final program must run to completion");
    assert_eq!(r.stdout(), ["ok"]);
}

#[test]
fn run_on_stack_returns_the_closure_result() {
    // The helper spawns a grown-stack thread and hands back the closure's
    // result. Compiling *inside* the closure is the supported pattern: the
    // runner itself is not `Send` (it owns `Rc` state).
    let stdout = rill_lang::run_on_stack(16 << 20, || {
        let mut r = runner(r#"main = do { write (Just "on-stack") stdout; return () };"#);
        r.run().expect("run must succeed");
        r.stdout().to_vec()
    });
    assert_eq!(stdout, ["on-stack"]);
}
