//! `Program` ↔ JSON round-trip contract: the AST must survive serialization
//! and deserialization unchanged.

use rill_lang::ast::Program;
use rill_lang::program_serde::{program_from_json, program_to_json};

/// Parse `src` into an AST program.
fn parse(src: &str) -> Program {
    let tokens = rill_lang::lexer::tokenize(src).unwrap();
    rill_lang::parser::parse(&tokens, src.as_bytes()).unwrap()
}

/// Serialize `src`'s AST to JSON, deserialize it back, and assert exact equality.
fn roundtrip(src: &str) {
    let program = parse(src);
    let json = program_to_json(&program).unwrap();
    assert!(
        json.contains('\n'),
        "program_to_json must emit pretty-printed JSON"
    );
    let back: Program = program_from_json(&json).unwrap();
    assert_eq!(
        back, program,
        "Program<->JSON round-trip must be exact for:\n{src}"
    );
}

#[test]
fn roundtrips_scalar_main() {
    roundtrip("main = 1.0");
}

#[test]
fn roundtrips_foreign_declaration() {
    roundtrip(
        "foreign onepole : Double -> Double -> FixedBuffer f32 -> FixedBuffer f32; main = ()",
    );
}

#[test]
fn roundtrips_conditional() {
    // The brief's `if (1.0 > 0.5) then ...` spelling does not parse: the
    // parser reads a parenthesized expression followed by an atom as an
    // application, so the condition is written without parentheses.
    roundtrip("main = if 1.0 > 0.5 then 2.0 else 3.0");
}

#[test]
fn roundtrips_data_declaration() {
    roundtrip("data Shape = Circle Double | Square; main = Circle 1.0");
}

#[test]
fn roundtrips_strict_data_declaration() {
    roundtrip("data P = { x: !Double, y: Double }; data T = C !Int | D Double; main = C 1");
}

#[test]
fn roundtrips_let_and_hole() {
    roundtrip("main = let g = 4.0 * ?gain in g + 1.0");
}

#[test]
fn roundtrips_io_do_form() {
    // The do-form IO shape is mode-agnostic: it compiles under the default
    // Strict mode, so no pragma is needed.
    roundtrip(r#"main = do { write (Just "x") stdout; return () }"#);
}

#[test]
fn roundtrips_mode_pragma() {
    // Mode pragma values are lowercase (`strict` / `complete`).
    roundtrip("{-# MODE complete #-} main = 1.0");
}
