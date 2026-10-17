//! Task 2: the new `ProgramRunner` executor — `main : IO ()` runs to
//! completion against the realized `World`, with captured `stdout`.

use rill_lang::builtin::Registry;
use rill_lang::compile_runner;
use rill_lang::program_runner::ProgramRunner;

fn runner(src: &str) -> ProgramRunner<f32, 256> {
    compile_runner::<f32, 256>(src, &Registry::new(), 44_100.0)
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
    // show_float renders each element and fold concatenates them.
    let mut r = runner(
        r#"
main = do {
    xs <- fromList [1.0, 2.0, 3.0];
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
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
fn main_string_args_bind() {
    let mut r = runner(r#"main a = do { write (Just a) stdout; return () };"#);
    r.set_args(&["hi".to_string()]);
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["hi"]);
}

#[test]
fn bounded_memory_overflow_is_a_clean_error() {
    // An unbounded lazy list is drained by fromList: the fixed arena budget is
    // exhausted and the runner reports a clean error instead of panicking.
    let mut r = runner(
        r"{-# MODE complete #-}
from n = cons n (from (n + 1.0));
main = do { xs <- fromList (from 0.0); return () };
",
    );
    let result = r.run();
    assert!(result.is_err(), "unbounded drain must fail cleanly");
}
