//! IO-1: `IO a` is a first-class type with a `Monad` instance; the io-streams
//! primitives (`fromList`/`toList`/`write`/`stdout`) type through builtin
//! signatures.

use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::ValueTy;

fn arrowty(src: &str) -> rill_lang::types::ty::ArrowTy {
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    let tp = infer_program(&prog, prog.mode).unwrap();
    tp.def_schemes["main"].ty.clone()
}

#[test]
fn write_output_types_as_io_void() {
    // `write : Maybe a -> OutputStream a -> IO ()` — the result is `IO Void`.
    let t = arrowty(r#"main = write (Just "x") stdout"#);
    assert_eq!(t.vty, ValueTy::Data("IO".into(), vec![ValueTy::Void]));
}

#[test]
fn from_list_types_as_io_input_stream() {
    // `fromList : List a -> IO (InputStream a)`.
    let t = arrowty("main = fromList [1.0, 2.0]");
    assert_eq!(
        t.vty,
        ValueTy::Data(
            "IO".into(),
            vec![ValueTy::Data("InputStream".into(), vec![ValueTy::Float])]
        )
    );
}

#[test]
fn to_list_types_as_io_list() {
    // `toList : InputStream a -> IO (List a)`; the element type flows from the
    // bound `fromList`.
    let t = arrowty("main = do { xs <- fromList [1.0]; toList xs }");
    assert_eq!(
        t.vty,
        ValueTy::Data(
            "IO".into(),
            vec![ValueTy::App("List".into(), vec![ValueTy::Float])]
        )
    );
}

#[test]
fn do_notation_over_io_uses_monad_instance() {
    // A do-block over IO binds through `instance Monad IO`; the result is the
    // final action's type.
    let t = arrowty(r#"main = do { xs <- fromList [1.0]; write (Just "x") stdout; return () }"#);
    assert_eq!(t.vty, ValueTy::Data("IO".into(), vec![ValueTy::Void]));
}
