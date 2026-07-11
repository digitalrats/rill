//! Human-readable text formatter with colored output.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

use colored::*;

use super::EventFormatter;

/// Formats debug events as colored terminal text, with optional file logging.
pub struct TextFormatter {
    writer: Box<dyn Write + Send>,
    log_file: Option<BufWriter<File>>,
}

impl TextFormatter {
    /// Create a TextFormatter writing to stdout and an optional log file.
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

    fn writeln(&mut self, line: &str) {
        let _ = writeln!(self.writer, "{}", line);
        if let Some(ref mut log) = self.log_file {
            let _ = writeln!(log, "{}", line);
        }
    }
}

impl EventFormatter for TextFormatter {
    fn format_probe(&mut self, probe_id: u32, name: &str, value_bits: u64, block_index: u64) {
        let value = f64::from_bits(value_bits);
        let line = format!(
            "{} probe[{}] {} = {:.6}",
            format!("[block {}]", block_index).dimmed(),
            probe_id.to_string().cyan().bold(),
            name.yellow(),
            value.to_string().green().bold(),
        );
        self.writeln(&line);
    }

    fn format_command(
        &mut self,
        block_index: u64,
        command_kind: &str,
        node_name: &str,
        param_name: Option<&str>,
        value_repr: &str,
    ) {
        let param_str = match param_name {
            Some(p) => format!(" {}: {}", p.magenta(), value_repr.green()),
            None => String::new(),
        };
        let line = format!(
            "{} cmd {} → {}{}",
            format!("[block {}]", block_index).dimmed(),
            command_kind.blue().bold(),
            node_name.cyan(),
            param_str,
        );
        self.writeln(&line);
    }

    fn format_break(&mut self, probe: &str, value: f64, block_index: u64) {
        let line = format!(
            "{} Breakpoint hit at {} = {:.6}",
            format!("[block {}]", block_index).dimmed(),
            probe.red().bold(),
            value.to_string().green().bold(),
        );
        self.writeln(&line);
    }

    fn format_pause(&mut self, reason: &str) {
        let line = format!(
            "{} Engine paused: {}",
            format!("[reason: {}]", reason).dimmed(),
            "BREAK".white().on_red(),
        );
        self.writeln(&line);
    }

    fn format_info(&mut self, message: &str) {
        let line = format!("{} {}", "INFO".white().on_blue(), message);
        self.writeln(&line);
    }

    fn flush(&mut self) {
        let _ = self.writer.flush();
        if let Some(ref mut log) = self.log_file {
            let _ = log.flush();
        }
    }
}
