//! Serializable program definition. The source string is the canonical form.

use rill_core::math::Transcendental;

use crate::error::CompileError;
use crate::ffi::ForeignRegistry;
use crate::program_runner::ProgramRunner;

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

/// Compile a [`RillLangDef`] into a runnable [`ProgramRunner`] for scalar type
/// `T`. Uses the default block size (`BUF = 256`) and an empty foreign registry.
pub fn compile_def<T: Transcendental>(
    def: &RillLangDef,
) -> Result<ProgramRunner<T, 256>, CompileError> {
    crate::compile::<T, 256>(&def.source, ForeignRegistry::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_def_runs() {
        let def = RillLangDef::new(
            "hello",
            r#"main = do { write (Just "hi") stdout; return () };"#,
        );
        let mut runner = compile_def::<f32>(&def).unwrap();
        runner.run().unwrap();
        assert_eq!(runner.stdout(), ["hi"]);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn round_trips_json() {
        let def = RillLangDef::new("gain", "main = 1.0 * 0.5");
        let json = serde_json::to_string(&def).unwrap();
        let back: RillLangDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }
}
