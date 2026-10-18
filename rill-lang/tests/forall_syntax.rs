use rill_lang::compile_eval;
use rill_lang::lexer::tokenize;
use rill_lang::mode::Mode;
use rill_lang::parser::parse;
use rill_lang::render::render;
use rill_lang::CompileError;

#[test]
fn forall_parses_and_roundtrips() {
    let src = "typeclass C a where { f: forall b. a -> b -> a; }; main = 1.0;";
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

#[test]
fn forall_requires_at_least_one_type_variable() {
    // Spec: `forall . T` (an empty binder) is a parse error, not a
    // degenerate quantification that silently absorbs the whole type.
    let src = "typeclass C a where { f: forall . a; }; main = 1.0;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes());
    assert!(
        prog.is_err(),
        "`forall . T` with no type variables must fail to parse: {prog:?}"
    );
    match prog.expect_err("parse must fail") {
        CompileError::Parse { msg, .. } => {
            assert!(
                msg.contains("at least one type variable"),
                "expected the empty-binder diagnostic, got: {msg}"
            );
        }
        other => panic!("expected a Parse error, got {other:?}"),
    }
}

#[test]
fn forall_render_is_fixed_point() {
    // Render must be idempotent: re-rendering the re-parse of the rendered
    // program reproduces the same text. (Program's PartialEq includes spans, so
    // assert the render fixed point, not structural equality.) Covers multi-var
    // binders, a nested binder, and a forall used as a function ARGUMENT — the
    // binder must be parenthesized there so it does not absorb the surrounding
    // arrow chain on re-parse.
    let src = r#"
        typeclass C a where {
            f: forall b c. a -> b -> c;
            g: forall b. forall c. b -> c;
            h: (forall b. a) -> a -> a;
        };
        main = 1.0;
    "#;
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes());
    assert!(prog.is_ok(), "forall type expressions must parse: {prog:?}");
    let out1 = render(&prog.unwrap()).unwrap();
    let toks2 = tokenize(&out1).unwrap();
    let prog2 = parse(&toks2, out1.as_bytes());
    assert!(
        prog2.is_ok(),
        "rendered program must re-parse: {prog2:?} (rendered: {out1})"
    );
    let out2 = render(&prog2.unwrap()).unwrap();
    assert_eq!(
        out1, out2,
        r"render must be a fixed point: re-render of the re-parse must case"
    );
}

#[test]
fn forall_two_arg_method_compiles() {
    // A 2-arg method whose signature is universally quantified
    // (`f: forall b. a -> b -> a`) must report arity 2 — not the `_ => 1`
    // wildcard's 1 — so `f 1 2` dispatches by its arguments (Issue 1).
    let src = r#"
        typeclass C a where { f: forall b. a -> b -> a; }
        instance C Int where { f x y = x; }
        main = f 1 2;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(
        res.is_ok(),
        "2-arg forall method must compile (arity mis-report regression): {:?}",
        res.err(),
    );
}
