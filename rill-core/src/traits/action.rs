//! Deprecated — use the `algorithm` module instead.
//!
//! This module was renamed to `algorithm` and split into:
//! - `Action`  → `Algorithm` (renamed, deprecation alias)
//! - `ActionContext` → kept as-is (re-exported from `algorithm`)

#![allow(deprecated)]

/// Alias for backward compatibility — use `Algorithm` instead.
#[deprecated(
    since = "0.4.0",
    note = "renamed to `Algorithm` — see the `algorithm` module"
)]
pub use super::algorithm::Algorithm as Action;

/// Re-exported from the `algorithm` module.
pub use super::algorithm::ActionContext;
