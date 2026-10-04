//! Evaluation mode: the strict/complete contract.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Evaluation contract selected per program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Mode {
    /// Current behavior: eager value track, recursion banned.
    #[default]
    Strict,
    /// Lazy (call-by-need) value track, Haskell-style recursion.
    Complete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_strict() {
        assert_eq!(Mode::default(), Mode::Strict);
    }
}
