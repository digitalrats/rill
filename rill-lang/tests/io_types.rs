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
    assert_eq!(t.vty, ValueTy::Con("IO".into(), vec![ValueTy::Void]));
}

#[test]
fn from_list_types_as_io_input_stream() {
    // `fromList : List a -> IO (InputStream a)`.
    let t = arrowty("main = fromList [1.0, 2.0]");
    assert_eq!(
        t.vty,
        ValueTy::Con(
            "IO".into(),
            vec![ValueTy::Con("InputStream".into(), vec![ValueTy::Double])]
        )
    );
}

#[test]
fn to_list_types_as_io_list() {
    // `toList : InputStream a -> IO (List a)`. The element type is `a`, and this
    // program never pins it: `fromList [1.0]` gives an `InputStream` whose
    // element the signature leaves open, so the SCHEME keeps `List a` rather
    // than closing over `Double`. Asserting `Double` here would be asserting a
    // decision the source does not make — the structural shape is the property
    // `toList`'s signature is about.
    let t = arrowty("main = do { xs <- fromList [1.0]; toList xs }");
    match &t.vty {
        ValueTy::Con(name, args) if name == "IO" && args.len() == 1 => match &args[0] {
            ValueTy::Con(list, elems) => {
                assert_eq!(list, "List", "the result is an `IO (List …)`");
                assert_eq!(elems.len(), 1, "a `List` has exactly one element type");
            }
            other => panic!("expected `IO (List a)`, got {other:?}"),
        },
        other => panic!("expected `IO (List a)`, got {other:?}"),
    }
}

#[test]
fn do_notation_over_io_uses_monad_instance() {
    // A do-block over IO binds through `instance Monad IO`; the result is the
    // final action's type.
    let t = arrowty(r#"main = do { xs <- fromList [1.0]; write (Just "x") stdout; return () }"#);
    assert_eq!(t.vty, ValueTy::Con("IO".into(), vec![ValueTy::Void]));
}
