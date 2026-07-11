//! JSON Lines formatter for machine-readable debug output.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

use super::EventFormatter;

/// Formats debug events as newline-delimited JSON.
pub struct JsonFormatter {
    writer: Box<dyn Write + Send>,
    log_file: Option<BufWriter<File>>,
}

impl JsonFormatter {
    /// Create a JsonFormatter writing to stdout and an optional log file.
    pub fn new(log_file: Option<std::path::PathBuf>) -> Self {
        let log = log_file.and_then(|path| {
            File::create(&path)
                .or_else(|_| OpenOptions::new().append(true).create(true).open(&path))
                .ok()
                .map(BufWriter::new)
        });
        Self {
            writer: Box::new(std::io::stdout()),
            log_file: log,
        }
    }

    fn writeln_json(&mut self, value: &serde_json::Value) {
        let mut buf = Vec::new();
        if serde_json::to_writer(&mut buf, value).is_ok() {
            buf.push(b'\n');
            let _ = self.writer.write_all(&buf);
            if let Some(ref mut log) = self.log_file {
                let _ = log.write_all(&buf);
            }
        }
    }
}

impl EventFormatter for JsonFormatter {
    fn format_probe(&mut self, probe_id: u32, name: &str, value_bits: u64, block_index: u64) {
        let value = f64::from_bits(value_bits);
        let json = serde_json::json!({
            "type": "probe",
            "block_index": block_index,
            "probe_id": probe_id,
            "name": name,
            "value": value,
            "value_bits": value_bits,
        });
        self.writeln_json(&json);
    }

    fn format_command(
        &mut self,
        block_index: u64,
        command_kind: &str,
        node_name: &str,
        param_name: Option<&str>,
        value_repr: &str,
    ) {
        let json = serde_json::json!({
            "type": "command",
            "block_index": block_index,
            "command_kind": command_kind,
            "node_name": node_name,
            "param_name": param_name,
            "value_repr": value_repr,
        });
        self.writeln_json(&json);
    }

    fn format_break(&mut self, probe: &str, value: f64, block_index: u64) {
        let json = serde_json::json!({
            "type": "breakpoint",
            "block_index": block_index,
            "probe": probe,
            "value": value,
        });
        self.writeln_json(&json);
    }

    fn format_pause(&mut self, reason: &str) {
        let json = serde_json::json!({
            "type": "pause",
            "reason": reason,
        });
        self.writeln_json(&json);
    }

    fn format_info(&mut self, message: &str) {
        let json = serde_json::json!({
            "type": "info",
            "message": message,
        });
        self.writeln_json(&json);
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
        if let Some(ref mut log) = self.log_file {
            let _ = log.flush();
        }
    }
}
