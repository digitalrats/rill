use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::render::render;

#[test]
fn forall_parses_and_roundtrips() {
    let src = "typeclass C a where { f: forall b. a -> b -> a; }; main = _;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes());
    assert!(
        prog.is_ok(),
        "forall in a type expression must parse: {prog:?}"
    );

    let out = render(&prog.unwrap()).unwrap();
    assert!(
        out.contains("forall b."),
        "rendered program must keep the forall quantification: {out}"
    );
    let toks2 = tokenize(&out).unwrap();
    let prog2 = parse(&toks2, out.as_bytes());
    assert!(
        prog2.is_ok(),
        "rendered program must re-parse: {prog2:?} (rendered: {out})"
    );
}
