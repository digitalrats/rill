use rill_lang::ast::{DefKind, Program};
use rill_lang::lexer;
use rill_lang::parser;

fn parse_str(src: &str) -> Program {
    let tokens = lexer::tokenize(src).unwrap();
    parser::parse(&tokens).unwrap()
}

#[test]
fn parses_param_def() {
    let prg = parse_str("param myFilter = _ : sin(440.0); process = myFilter : _;");
    assert_eq!(prg.defs.len(), 2);
    assert_eq!(prg.defs[0].name, "myFilter");
    assert_eq!(prg.defs[0].kind, DefKind::Param);
    assert_eq!(prg.defs[1].name, "process");
    assert_eq!(prg.defs[1].kind, DefKind::Def);
}

#[test]
fn parses_keep_param() {
    let prg = parse_str("keep param osc = sin(440.0); process = osc : _;");
    assert_eq!(prg.defs[0].kind, DefKind::KeepParam);
}

#[test]
fn parses_inline_param() {
    let prg = parse_str("inline param gain = _ * 0.5; process = gain : _;");
    assert_eq!(prg.defs[0].kind, DefKind::InlineParam);
}

#[test]
fn bare_identifier_is_def_kind() {
    let prg = parse_str("myAlias = _ * 0.5; process = myAlias : _;");
    assert_eq!(prg.defs[0].kind, DefKind::Def);
}

#[test]
fn keep_without_param_is_error() {
    let tokens = lexer::tokenize("keep osc = sin(440.0); process = osc : _;").unwrap();
    let result = parser::parse(&tokens);
    assert!(result.is_err(), "`keep` without `param` should be an error");
}
