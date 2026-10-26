//! The stdio surface and the scalar renderers.
//!
//! `stdin :: InputStream String`, `stdout`/`stderr :: OutputStream String` are
//! the runner's realized console (line-oriented text — rill has no
//! `ByteString`); `show_double`/`show_int`/`show_bool` render scalars to
//! `String`. `reference_program_runs` runs the spec's §7 reference program
//! end-to-end, and `write_nothing_does_not_close_the_console` pins the console
//! close semantics the rest of this file relies on.

use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::run_on_stack;

/// Compile and run a program, returning its captured `(stdout, stderr)`.
///
/// `stdin_lines` is injected into the runner's console before the run, so
/// `read stdin` yields those lines in order. The program runs on a thread with
/// a large stack: the lazy `IO` interpreter nests one `bind` per `do`
/// statement, and a multi-statement program would exceed libtest's default
/// 2 MiB thread stack in a debug build (see [`rill_lang::run_on_stack`]).
fn run(src: &str, stdin_lines: &[String]) -> (Vec<String>, Vec<String>) {
    let src = src.to_string();
    let stdin_lines = stdin_lines.to_vec();
    run_on_stack(16 << 20, move || {
        let mut r = compile::<f32, 256>(&src, ForeignRegistry::new())
            .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
        r.set_stdin(&stdin_lines);
        r.run()
            .unwrap_or_else(|e| panic!("run must succeed: {e}\nsource:\n{src}"));
        (r.stdout().to_vec(), r.stderr().to_vec())
    })
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
show_mb m = case m of { Nothing -> "eof"; Just x -> x };
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
fn to_list_drains_stdin() {
    // `toList :: InputStream a -> IO (List a)` on the cursor-backed console
    // input: it pulls every line through the normal read path, then reports
    // EOF. `stdin` is spent afterwards (a subsequent read yields `Nothing`).
    let src = r#"
show_mb m = case m of { Nothing -> "eof"; Just x -> x };
main = do {
    ys <- toList stdin;
    write (Just (fold (\acc -> \x -> concat_string acc x) "" ys)) stdout;
    z <- read stdin;
    write (Just (show_mb z)) stdout;
    return ()
};
"#;
    let (stdout, _) = run(src, &["alpha".to_string(), "beta".to_string()]);
    assert_eq!(stdout, ["alphabeta", "eof"]);
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
    //   `Nothing -> return ()` arm cannot resolve a monad (a self-referential
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
    write (Just (concat_string "sum-ish: " (show_double ref))) stdout;
    return ()
};
"#;
    // `ys` is drained by `toList` for the memory-stream round-trip but not
    // rendered: the spec's `show ys` needs list rendering, which is out of
    // scope here, so the scalar `runST` result is the observable payload.
    //
    // The exact line pins the §7 payload: the region runs AND `writeSTRef`'s
    // write is observed (`readSTRef` yields 42.0, not the 0.0 initial value).
    // `show_double` renders an integral f64 via `{:?}` → `"42.0"`, which is
    // deterministic (a fixed value, a fixed formatter — no locale or formatting
    // state involved).
    let (stdout, _) = run(src, &[]);
    assert_eq!(stdout, ["sum-ish: 42.0"]);
}

#[test]
fn write_nothing_does_not_close_the_console() {
    // Pins the console-close decision: `write Nothing` to a stdio handle is the
    // io-streams close signal but is INERT for the runner's console, because
    // `stdout`/`stderr` are output handles of the realized `World` (owned by the
    // runner for its whole lifetime), not program-owned streams. A later `Just`
    // write must therefore still arrive. Only program-owned `makeOutputStream`
    // sinks close on `write Nothing` (see `io_streams_memory.rs`); a regression
    // to latching-close here would also break the two Task 3 tests that use
    // `write Nothing stdout` as a per-write stop signal.
    let src = r#"
main = do {
    write Nothing stdout;
    write (Just "after") stdout;
    write Nothing stderr;
    write (Just "err-after") stderr;
    return ()
};
"#;
    let (stdout, stderr) = run(src, &[]);
    assert_eq!(stdout, ["after"]);
    assert_eq!(stderr, ["err-after"]);
}

#[test]
fn show_renders_scalars() {
    let src = r#"
main = do {
    write (Just (show_double 1.5)) stdout;
    write (Just (show_int 3)) stdout;
    write (Just (show_bool true)) stdout;
    write (Just (show_bool false)) stdout;
    return ()
};
"#;
    assert_eq!(run_stdout(src), ["1.5", "3", "True", "False"]);
}

#[test]
fn double_renders_ghc_double_values() {
    // `Double` is the double-precision type, so its arithmetic must be byte-for
    // -byte what GHC prints for `Double`. These two strings are the reason the
    // type is NAMED `Double`: `0.1 + 0.2` is not `0.3` in binary64, and
    // `1.0 / 3.0` has the full 17-digit expansion. GHC's `Float`
    // (single precision) would print `0.3` and `0.33333334` instead, so these
    // assertions fail if the payload is ever narrowed to `f32`.
    let src = r#"
main = do {
    write (Just (show_double (0.1 + 0.2))) stdout;
    write (Just (show_double (1.0 / 3.0))) stdout;
    return ()
};
"#;
    assert_eq!(
        run_stdout(src),
        ["0.30000000000000004", "0.3333333333333333"]
    );
}
