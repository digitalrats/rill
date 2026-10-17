//! Stream contract type-level tests: `InputStream`/`OutputStream` are
//! first-class higher-kinded types, `read`/`write`/`channel`/`runStream` are
//! the boundary operations, and a stream program's process type is `IO ()`.

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
    // `handler ins outs = do { write outs (read ins); return () }; main = runStream handler`
    // is the stream form of the 1→1 identity: read all inputs, write them back.
    // The handler is a multi-parameter definition (flat `Thunk`, matching
    // `runStream`'s `(InputStream Buffer -> OutputStream Buffer -> IO ())`);
    // literal lambdas are strictly unary and curry (nested `Thunk`), so a
    // handler is written as a named definition.
    let t = ty_of(
        "handler ins outs = do { write outs (read ins); return () }; main = runStream handler",
    );
    assert_io_void(&t);
}

#[test]
fn run_stream_write_only_types_io_void() {
    // Write-only, 0-input-channel program (Review Focus 2: read ins is empty).
    let t = ty_of("handler ins outs = do { write outs [sine 440.0 1.0 0.0]; return () }; main = runStream handler");
    assert_io_void(&t);
}

#[test]
fn run_stream_map_pipeline_types_io_void() {
    // Scalar pipeline: `map (\x -> x * 0.5)` over the input frame, written out.
    let t = ty_of("handler ins outs = do { write outs (map (\\x -> x * 0.5) (read ins)); return () }; main = runStream handler");
    assert_io_void(&t);
}
