use rill_lang::ast::Expr;
use rill_lang::lexer;
use rill_lang::parser;

fn parse_str(src: &str) -> Result<rill_lang::ast::Program, rill_lang::CompileError> {
    let tokens = lexer::tokenize(src).unwrap();
    parser::parse(&tokens, src.as_bytes())
}

#[test]
fn parses_main_with_where() {
    let prg = parse_str("main = myFilter : _ where { myFilter = _ : sin 440.0; }").unwrap();
    let main = prg.main_def().unwrap();
    assert_eq!(main.where_defs().len(), 1);
}

#[test]
fn parses_main_with_where_layout() {
    let prg =
        parse_str("main = myFilter : _ where\n    myFilter = _ : sin 440.0;\n    gain = _ * 0.5\n")
            .unwrap();
    let main = prg.main_def().unwrap();
    assert_eq!(main.where_defs().len(), 2);
    assert_eq!(main.where_defs()[0].name(), "myFilter");
    assert_eq!(main.where_defs()[1].name(), "gain");
}

#[test]
fn parses_main_simple() {
    let prg = parse_str("main = _ : _").unwrap();
    let main = prg.main_def().unwrap();
    assert!(matches!(main.body(), Expr::Bin { .. }));
}

#[test]
fn empty_program_is_rejected() {
    assert!(parse_str("").is_err());
}

#[test]
fn parses_top_level_multi_def() {
    let prg = parse_str("sq x = x * x; main = sq 0.5").unwrap();
    assert_eq!(prg.defs.len(), 2);
}

#[test]
fn parses_let_expression() {
    let prg = parse_str("main = let g = _ * 0.5 in g").unwrap();
    let main = prg.main_def().unwrap();
    assert!(matches!(main.body(), Expr::Let { .. }));
}
