//! IO-1: `IO a` is a first-class type with a `Monad` instance; the IO
//! primitives `readInput`/`writeOutput` type through builtin signatures.

use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::ValueTy;

fn arrowty(src: &str) -> rill_lang::types::ty::ArrowTy {
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let tp = infer_program(&prog, prog.mode).unwrap();
    tp.process_ty
}

#[test]
fn write_output_types_as_io_void() {
    // `writeOutput : Int -> Buffer -> IO ()` — the result is `IO Void`.
    let t = arrowty("main = writeOutput 0 1.0");
    assert_eq!(t.outs[0], ValueTy::Data("IO".into(), vec![ValueTy::Void]));
}

#[test]
fn read_input_types_as_io_buffer() {
    // `readInput : Int -> IO Buffer`.
    let t = arrowty("main = readInput 0");
    assert_eq!(t.outs[0], ValueTy::Data("IO".into(), vec![ValueTy::Buffer]));
}

#[test]
fn do_notation_over_io_uses_monad_instance() {
    // A do-block over IO binds through `instance Monad IO`; the result is the
    // final action's type.
    let t = arrowty("main = do { writeOutput 0 1.0; return () }");
    assert_eq!(t.outs[0], ValueTy::Data("IO".into(), vec![ValueTy::Void]));
}
