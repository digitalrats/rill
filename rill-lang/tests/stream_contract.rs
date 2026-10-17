//! Stream contract type-level tests: `InputStream`/`OutputStream`/`Stream` are
//! first-class higher-kinded types, `read`/`write`/`channel`/`runStream` are
//! the boundary operations (with the `runStream` handler bound over the general
//! `Stream` and the concrete views reached via `asInput`/`asOutput`), and
//! `mkStream` creates a stream pair inside the program. A stream program's
//! process type is `IO ()`.

#![cfg(feature = "dsp")]

use rill_lang::types::{infer, ValueTy};

/// Type-check `src` and return the typed program.
fn ty_of(src: &str) -> rill_lang::types::infer::TypedProgram {
    let tokens = rill_lang::lexer::tokenize(src).unwrap();
    let program = rill_lang::parser::parse(&tokens, src.as_bytes()).unwrap();
    infer::infer_program(&program, rill_lang::mode::Mode::default()).unwrap()
}

/// Assert the program's process type is exactly `IO ()`.
fn assert_io_void(t: &rill_lang::types::infer::TypedProgram) {
    assert_eq!(
        t.process_ty.vty,
        ValueTy::Data("IO".into(), vec![ValueTy::Void]),
        "expected IO (), got {:?}",
        t.process_ty.vty
    );
}

#[test]
fn run_stream_identity_types_io_void() {
    // `handler master slave = do { write (asOutput slave) (read (asInput master)); return () };
    //  main = runStream handler` is the stream form of the 1→1 identity: read the
    // master input, write the slave output. The handler is a multi-parameter
    // definition; the parser curries it into nested single-argument lambdas
    // (`\master -> \slave -> …`), producing a NESTED `Thunk` that matches
    // `runStream`'s declared (also nested) `Stream Buffer -> Stream Buffer -> IO ()`.
    // Literal lambdas are strictly unary, so a handler is written as a named
    // definition. The handler params are the general `Stream`; the body reaches
    // the concrete views via the `asInput`/`asOutput` projections.
    let t = ty_of(
        "handler master slave = do { write (asOutput slave) (read (asInput master)); return () }; main = runStream handler",
    );
    assert_io_void(&t);
}

#[test]
fn run_stream_write_only_types_io_void() {
    // Write-only, 0-input-channel program (Review Focus 2: read ins is empty).
    // The slave output is written from a generated buffer, no master input read.
    let t = ty_of("handler master slave = do { write (asOutput slave) [sine 440.0 1.0 0.0]; return () }; main = runStream handler");
    assert_io_void(&t);
}

#[test]
fn run_stream_map_pipeline_types_io_void() {
    // Scalar pipeline: `map (\x -> x * 0.5)` over the master input frame,
    // written to the slave output.
    let t = ty_of("handler master slave = do { write (asOutput slave) (map (\\x -> x * 0.5) (read (asInput master))); return () }; main = runStream handler");
    assert_io_void(&t);
}

#[test]
fn run_stream_output_master_types_io_void() {
    // Output-master form: the handler's first parameter (the master) is used as
    // the output (`asOutput master`), and the slave as the input (`asInput slave`).
    // This is the `lang_chiptune`-class shape — the runtime validates the
    // projections against the backend's active side (Task 3); at the type level
    // both directions are expressible over the general `Stream`.
    let t = ty_of("handler master slave = do { write (asOutput master) (map (\\x -> x * 2.0) (read (asInput slave))); return () }; main = runStream handler");
    assert_io_void(&t);
}

#[test]
fn mk_stream_types_io_void() {
    // `mkStream (BackendSpec { name: "test" }) : IO (Pair (Stream Buffer) (Stream Buffer))`
    // creates a backend (resolved by name against the runtime registry) together
    // with its stream pair inside the program. The whole pair is bound; tuple
    // *patterns* are not part of the grammar (tuple values `(a, b)` are). The
    // spec value is the record CONSTRUCTOR form (`BackendSpec { name: "test" }`):
    // a bare record literal infers as a Float channel in the value track, so the
    // constructor application is how a record value is built.
    let t = ty_of("main = do { p <- mkStream (BackendSpec { name: \"test\" }); return () }");
    assert_io_void(&t);
}
