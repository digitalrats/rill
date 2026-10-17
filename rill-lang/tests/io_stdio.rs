//! The stdio surface and the scalar renderers.
//!
//! `stdin :: InputStream String`, `stdout`/`stderr :: OutputStream String` are
//! the runner's realized console (line-oriented text — rill has no
//! `ByteString`); `show_float`/`show_int`/`show_bool` render scalars to
//! `String`. The last test runs the spec's §7 reference program end-to-end.

use rill_lang::builtin::Registry;
use rill_lang::compile_runner;

/// Compile and run a program, returning its captured `(stdout, stderr)`.
///
/// `stdin_lines` is injected into the runner's console before the run, so
/// `read stdin` yields those lines in order. The program runs on a thread with
/// a large stack: the lazy `IO` interpreter nests one `bind` per `do`
/// statement, and a multi-statement program would exceed libtest's default
/// 2 MiB thread stack in a debug build (see `io_streams_memory.rs`).
fn run(src: &str, stdin_lines: &[String]) -> (Vec<String>, Vec<String>) {
    let src = src.to_string();
    let stdin_lines = stdin_lines.to_vec();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let mut r = compile_runner::<f32, 256>(&src, &Registry::new(), 44_100.0)
                .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
            r.set_stdin(&stdin_lines);
            r.run()
                .unwrap_or_else(|e| panic!("run must succeed: {e}\nsource:\n{src}"));
            (r.stdout().to_vec(), r.stderr().to_vec())
        })
        .expect("spawn program thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// [`run`] with no injected stdin, returning only the captured `stdout`.
fn run_stdout(src: &str) -> Vec<String> {
    run(src, &[]).0
}

#[test]
fn stdout_capture_roundtrip() {
    let (stdout, stderr) = run(r#"main = do { write (Just "hi") stdout; return () };"#, &[]);
    assert_eq!(stdout, ["hi"]);
    assert_eq!(stderr, [] as [String; 0]);
}

#[test]
fn stderr_is_separate() {
    // stderr is its own sink: a write there never reaches stdout's capture.
    let (stdout, stderr) = run(
        r#"main = do { write (Just "oops") stderr; return () };"#,
        &[],
    );
    assert_eq!(stdout, [] as [String; 0]);
    assert_eq!(stderr, ["oops"]);
}

#[test]
fn stdin_feeds_lines() {
    // `read stdin` yields the injected console lines in order, then `Nothing`
    // (EOF) once they are drained.
    let src = r#"
show_mb m = case m of { Nothing => "eof"; Just x => x };
main = do {
    a <- read stdin;
    b <- read stdin;
    c <- read stdin;
    write (Just (show_mb a)) stdout;
    write (Just (show_mb b)) stdout;
    write (Just (show_mb c)) stdout;
    return ()
};
"#;
    let (stdout, _) = run(src, &["alpha".to_string(), "beta".to_string()]);
    assert_eq!(stdout, ["alpha", "beta", "eof"]);
}

#[test]
fn reference_program_runs() {
    // The spec §7 reference program, adapted to the settled surface: no `++`
    // (use `concat_string`), no `Show` typeclass (use the scalar renderers),
    // and list rendering is out of scope — the `runST` result is rendered.
    //
    // Two adaptations of the spec text:
    // - `ref <- runST (…)` is not well-typed inside a monadic `do`: `runST`
    //   yields a *pure* value, while `<-` requires an `m a` container
    //   (the spec's statement is ill-typed in Haskell too). The binding moves
    //   to a top-level definition — the idiomatic `let ref = runST (…)`.
    // - The spec's `handle = makeOutputStream (…)` binding is omitted: its
    //   `Nothing => return ()` arm cannot resolve a monad (a self-referential
    //   definition gives inference no expected type), and `handle` is dead
    //   code — it is not referenced by `main`. The `makeOutputStream` wrapper
    //   itself is covered by `io_streams_memory.rs`.
    let src = r#"
ref = runST (\s -> do {
    r <- newSTRef 0.0;
    writeSTRef r 42.0;
    readSTRef r
});

main = do {
    xs <- fromList (map (\x -> x * 2.0) [1.0, 2.0, 3.0]);
    ys <- toList xs;
    write (Just (concat_string "sum-ish: " (show_float ref))) stdout;
    return ()
};
"#;
    // `ys` is drained by `toList` for the memory-stream round-trip but not
    // rendered: the spec's `show ys` needs list rendering, which is out of
    // scope here, so the scalar `runST` result is the observable payload.
    let (stdout, _) = run(src, &[]);
    assert!(
        stdout.iter().any(|line| line.contains("sum-ish")),
        "the reference program must report its STRef result: {stdout:?}"
    );
}

#[test]
fn show_renders_scalars() {
    let src = r#"
main = do {
    write (Just (show_float 1.5)) stdout;
    write (Just (show_int 3)) stdout;
    write (Just (show_bool true)) stdout;
    write (Just (show_bool false)) stdout;
    return ()
};
"#;
    assert_eq!(run_stdout(src), ["1.5", "3", "True", "False"]);
}
