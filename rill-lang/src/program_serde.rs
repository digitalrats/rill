//! `Program` ↔ JSON serialization contract.
//!
//! The AST ([`Program`]) is the canonical serialized form of a rill-lang
//! program: [`program_to_json`] emits pretty-printed JSON and
//! [`program_from_json`] restores an identical [`Program`].

use crate::ast::Program;

/// Serialize a program AST to pretty-printed JSON.
///
/// The output is the exact inverse of [`program_from_json`]: deserializing it
/// yields a [`Program`] equal to `program`.
pub fn program_to_json(program: &Program) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(program)
}

/// Deserialize a program AST from JSON produced by [`program_to_json`].
pub fn program_from_json(json: &str) -> Result<Program, serde_json::Error> {
    serde_json::from_str(json)
}
