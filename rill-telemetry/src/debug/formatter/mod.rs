//! Event formatter abstraction for debug output.

use std::io::Write;

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

    /// Format a breakpoint hit event.
    fn format_break(&mut self, probe_id: u32, name: &str, block_index: u64);

    /// Format a pause event.
    fn format_pause(&mut self, block_index: u64);

    /// Format an informational message.
    fn format_info(&mut self, message: &str);

    /// Flush any buffered output.
    fn flush(&mut self);
}
