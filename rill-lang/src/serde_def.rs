//! Serializable program definition. The source string is the canonical form.

use rill_core::math::Transcendental;

use crate::error::CompileError;
use crate::program::RillProgram;

/// A serializable rill-lang program definition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RillLangDef {
    /// Format version tag, e.g. `"rill-lang/1"`.
    pub format_version: String,
    /// The DSL source text — the canonical serialized representation.
    pub source: String,
    /// Human-readable instance name.
    pub name: String,
}

impl RillLangDef {
    /// Construct a definition from source with the default format version.
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            format_version: "rill-lang/1".to_string(),
            source: source.into(),
            name: name.into(),
        }
    }
}

/// Compile a [`RillLangDef`] into a runnable program for scalar type `T`.
pub fn compile_def<T: Transcendental>(def: &RillLangDef) -> Result<RillProgram<T>, CompileError> {
    crate::compile::<T>(&def.source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::traits::Algorithm;

    #[test]
    fn compile_def_runs() {
        let def = RillLangDef::new("gain", "main = _ * 0.5");
        let mut prog = compile_def::<f32>(&def).unwrap();
        let mut out = [0.0f32; 2];
        prog.process(Some(&[2.0, 6.0]), &mut out).unwrap();
        assert_eq!(out, [1.0, 3.0]);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn round_trips_json() {
        let def = RillLangDef::new("gain", "main = _ * 0.5");
        let json = serde_json::to_string(&def).unwrap();
        let back: RillLangDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }
}
