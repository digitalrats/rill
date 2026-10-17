//! The io-streams memory surface: `fromList`/`toList`, `read`/`write`,
//! `atEnd`, `unread`, `connect`, and the `makeInputStream`/`makeOutputStream`
//! wrappers — faithful to Haskell io-streams (`Nothing` = EOF / close).

use rill_lang::builtin::Registry;
use rill_lang::compile_runner;
use rill_lang::program_runner::ProgramRunner;

fn runner(src: &str) -> ProgramRunner<f32, 256> {
    compile_runner::<f32, 256>(src, &Registry::new(), 44_100.0)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"))
}

/// Compile and run a whole program, returning its captured `stdout`.
///
/// The program runs on a thread with a large stack: the interpreter evaluates
/// the `IO` chain recursively (every `do` statement is one nested `bind`), and
/// in a debug build a single statement's frame chain is large enough that a
/// multi-statement stream program would exceed libtest's default 2 MiB thread
/// stack. That is a depth property of the lazy `IO` interpreter, not of the
/// stream surface under test.
fn run(src: &str) -> Vec<String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let mut r = runner(&src);
            r.run()
                .unwrap_or_else(|e| panic!("run must succeed: {e}\nsource:\n{src}"));
            r.stdout().to_vec()
        })
        .expect("spawn program thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Compile a program and run its `IO main` action `runs` times, returning the
/// captured `stdout`, the arena's live-slot count, and its capacity.
///
/// Repeated runs execute the same write sequence with a fixed instruction
/// count, which is how a long sink write sequence is expressed here: the
/// language has no `IO` loop construct. Growth that is proportional to the
/// number of *writes* (rather than to the number of runs) is what a leak looks
/// like. The work runs on a big-stack thread for the same reason as [`run`].
fn run_many(src: &str, runs: usize) -> (Vec<String>, usize, usize) {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let mut r = runner(&src);
            for i in 0..runs {
                r.run()
                    .unwrap_or_else(|e| panic!("run {i} must succeed: {e}\nsource:\n{src}"));
            }
            let arena = r.program().arena();
            (r.stdout().to_vec(), arena.live(), arena.capacity())
        })
        .expect("spawn program thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Shared program prefix: renders a `Maybe Float` as text (`Nothing` = EOF).
const SHOW_MAYBE: &str = r#"show_mb m = case m of { Nothing => "eof"; Just x => show_float x };"#;

/// Wrap a stream-producing expression in `fromList` → `toList` → `stdout`,
/// rendering the drained list with `fold`/`concat_string`/`show_float`.
fn render(src_body: &str) -> Vec<String> {
    let src = format!(
        r#"
main = do {{
    xs <- {src_body};
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
    return ()
}};
"#
    );
    run(&src)
}

#[test]
fn from_list_to_list_preserves_order() {
    assert_eq!(render("fromList [1.0, 2.0, 3.0]"), vec!["1.02.03.0"]);
}

#[test]
fn to_list_on_empty_stream_is_empty() {
    assert_eq!(render("fromList list"), vec![""]);
}

#[test]
fn read_pulls_elements_and_nothing_at_eof() {
    // Two elements then `Nothing`, and reading past EOF keeps yielding
    // `Nothing` rather than erroring.
    let src = format!(
        r#"{SHOW_MAYBE}
main = do {{
    xs <- fromList [1.0, 2.0];
    a <- read xs;
    b <- read xs;
    c <- read xs;
    d <- read xs;
    write (Just (show_mb a)) stdout;
    write (Just (show_mb b)) stdout;
    write (Just (show_mb c)) stdout;
    write (Just (show_mb d)) stdout;
    return ()
}};
"#
    );
    assert_eq!(run(&src), ["1.0", "2.0", "eof", "eof"]);
}

#[test]
fn write_nothing_closes_the_stream() {
    // `write Nothing` is the close signal: the sink sees it, and every later
    // write is inert (it never reaches the sink).
    let src = r#"
main = do {
    out <- makeOutputStream (\mb -> case mb of {
        Nothing => write (Just "closed") stdout;
        Just x => write (Just (show_float x)) stdout
    });
    write (Just 1.0) out;
    write Nothing out;
    write (Just 2.0) out;
    return ()
};
"#;
    assert_eq!(run(src), ["1.0", "closed"]);
}

#[test]
fn at_end_reports_eof_without_consuming() {
    // `atEnd` on a live stream is False and does not consume (the following
    // `read` still yields the first element); on a drained stream it is True.
    let src = format!(
        r#"{SHOW_MAYBE}
main = do {{
    xs <- fromList [1.0];
    before <- atEnd xs;
    v <- read xs;
    after <- atEnd xs;
    w <- read xs;
    write (Just (case before of {{ true => "at-end"; false => "open" }})) stdout;
    write (Just (show_mb v)) stdout;
    write (Just (case after of {{ true => "at-end"; false => "open" }})) stdout;
    write (Just (show_mb w)) stdout;
    return ()
}};
"#
    );
    assert_eq!(run(&src), ["open", "1.0", "at-end", "eof"]);
}

#[test]
fn unread_then_read_roundtrips() {
    // `unread` pushes a value back: the next `read` returns the pushed value,
    // then the stream continues where it was.
    let src = format!(
        r#"{SHOW_MAYBE}
main = do {{
    xs <- fromList [1.0, 2.0];
    a <- read xs;
    unread 5.0 xs;
    b <- read xs;
    c <- read xs;
    write (Just (show_mb a)) stdout;
    write (Just (show_mb b)) stdout;
    write (Just (show_mb c)) stdout;
    return ()
}};
"#
    );
    assert_eq!(run(&src), ["1.0", "5.0", "2.0"]);
}

#[test]
fn connect_pipes_input_to_output() {
    // `connect` pulls every element of the input into the output and closes the
    // output (`Nothing`) at input EOF.
    let src = r#"
main = do {
    xs <- fromList [1.0, 2.0, 3.0];
    out <- makeOutputStream (\mb -> case mb of {
        Nothing => write (Just "|") stdout;
        Just x => write (Just (show_float x)) stdout
    });
    connect xs out;
    return ()
};
"#;
    assert_eq!(run(src), ["1.0", "2.0", "3.0", "|"]);
}

#[test]
fn make_stream_wraps_program_closures() {
    // `makeInputStream` wraps a program-provided `IO (Maybe a)` action: each
    // `read` runs it, and a `Nothing` result is EOF. `atEnd` on such a source
    // pulls one element and pushes it back, so it does not consume.
    let src = format!(
        r#"{SHOW_MAYBE}
main = do {{
    src <- makeInputStream (return (Just 7.0));
    e <- atEnd src;
    a <- read src;
    write (Just (case e of {{ true => "at-end"; false => "open" }})) stdout;
    write (Just (show_mb a)) stdout;
    return ()
}};
"#
    );
    assert_eq!(run(&src), ["open", "7.0"]);

    let src = format!(
        r#"{SHOW_MAYBE}
main = do {{
    src <- makeInputStream (return Nothing);
    e <- atEnd src;
    a <- read src;
    write (Just (case e of {{ true => "at-end"; false => "open" }})) stdout;
    write (Just (show_mb a)) stdout;
    return ()
}};
"#
    );
    assert_eq!(run(&src), ["at-end", "eof"]);
}

#[test]
fn connect_unequal_lengths_ends_cleanly() {
    // The input is longer than what the sink accepts: the sink drops the
    // elements from 3.0 on (`write Nothing` on stdout is inert). `connect`
    // still ends at input EOF (no error) and delivers the closing `Nothing`.
    let src = r#"
main = do {
    xs <- fromList [1.0, 2.0, 3.0, 4.0];
    out <- makeOutputStream (\mb -> case mb of {
        Nothing => write (Just "closed") stdout;
        Just x => if x < 3.0 then write (Just (show_float x)) stdout else write Nothing stdout
    });
    connect xs out;
    return ()
};
"#;
    assert_eq!(run(src), ["1.0", "2.0", "closed"]);
}

#[test]
fn long_sink_write_sequence_does_not_exhaust_the_arena() {
    // Every sink write runs the handler and must release the handler's `IO ()`
    // result. If that owned value leaked, one arena slot would be lost per
    // element and this sequence would die with "value arena capacity" — the
    // pre-fix code did, at run 282 of 600 (capacity 3864). Ten element writes
    // per run make the leak grow faster than the (heap-budgeted) capacity.
    let mut writes = String::new();
    for i in 0..10 {
        writes.push_str(&format!("    write (Just {i}.0) out;\n"));
    }
    let src = format!(
        r#"main = do {{
    out <- makeOutputStream (\mb -> case mb of {{
        Nothing => write (Just "closed") stdout;
        Just x => write Nothing stdout
    }});
{writes}    write Nothing out;
    return ()
}};
"#
    );
    let (lines, live, capacity) = run_many(&src, 600);
    assert_eq!(lines.len(), 600, "one close line per run");
    assert!(
        lines.iter().all(|line| line == "closed"),
        "the sink must see exactly the closing `Nothing` per run"
    );
    assert!(
        live < capacity,
        "arena live slots must stay below capacity: {live} of {capacity}"
    );
}

#[test]
fn if_on_a_lazy_bool_takes_the_right_branch() {
    // In complete mode a call result is a suspended thunk, so `flag 2.0` is not
    // a WHNF Bool in the condition register: the block term must force it
    // before it dispatches (pre-fix, the true case took the `else` branch).
    let src = r#"flag n = 1.0 < n;
main = do {
    if flag 2.0 then write (Just "t") stdout else write (Just "f") stdout;
    if flag 0.5 then write (Just "t") stdout else write (Just "f") stdout;
    return ()
};
"#;
    assert_eq!(run(src), ["t", "f"]);
}

#[test]
fn if_arm_in_a_sink_handler_resolves_monad_io() {
    // A result-directed method in an `if` arm (`return ()`) selects its
    // instance from the handler's expected `IO ()`: inference and lowering both
    // thread the surrounding expected type into the branches. The condition
    // stays dynamic (a comparison, not a literal Bool), so the arm really runs
    // or does not at runtime.
    let src = r#"
main = do {
    skipped <- makeOutputStream (\mb -> if 2.0 < 1.0 then write (Just "bad") stdout else return ());
    write (Just 1.0) skipped;
    write Nothing skipped;
    run <- makeOutputStream (\mb -> if 1.0 < 2.0 then write (Just "kept") stdout else return ());
    write (Just 1.0) run;
    write Nothing run;
    return ()
};
"#;
    // `skipped`'s handler takes the `return ()` arm for both elements; `run`'s
    // handler writes once per element (the value and the closing `Nothing`).
    assert_eq!(run(src), ["kept", "kept"]);
}
