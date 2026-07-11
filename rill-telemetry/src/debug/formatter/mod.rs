//! Event formatter abstraction for debug output.

pub mod json;
pub mod text;

pub use json::JsonFormatter;
pub use text::TextFormatter;

/// Trait for formatting debug events (probe values, commands, breakpoints, etc.).
pub trait EventFormatter {
    /// Format a probe value capture event.
    fn format_probe(&mut self, probe_id: u32, name: &str, value_bits: u64, block_index: u64);

    /// Format a command capture event.
    fn format_command(
        &mut self,
        block_index: u64,
        command_kind: &str,
        node_name: &str,
        param_name: Option<&str>,
        value_repr: &str,
    );

    /// Format a breakpoint hit (Phase 3 — called from REPL-aware collector).
    fn format_break(&mut self, probe: &str, value: f64, block_index: u64);

    /// Format engine pause event (Phase 3).
    fn format_pause(&mut self, reason: &str);

    /// Format informational message (Phase 3).
    fn format_info(&mut self, message: &str);

    /// Flush any buffered output.
    fn flush(&mut self);
}
