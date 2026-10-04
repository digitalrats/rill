//! Evaluation mode: the strict/complete contract.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Evaluation contract selected per program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Mode {
    /// Current behavior: eager value track, recursion banned.
    Strict,
    /// Lazy (call-by-need) value track, Haskell-style recursion.
    Complete,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Strict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_strict() {
        assert_eq!(Mode::default(), Mode::Strict);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(Mode::Strict, Mode::Complete);
    }
}
