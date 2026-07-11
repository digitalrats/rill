mod history;
mod parser;

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;

use colored::Colorize;

use rill_telemetry::debug::protocol::{AnalyzerCommand, AnalyzerResponse, NodeInfo};

use self::history::History;
use self::parser::Command;

pub fn run(
    cmd_tx: mpsc::Sender<AnalyzerCommand>,
    resp_rx: mpsc::Receiver<AnalyzerResponse>,
    nodes: Vec<NodeInfo>,
) {
    println!(
        "{} {} node(s)",
        "[rill-analyzer 0.5]".green(),
        nodes.len().to_string().bold()
    );

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut history = History::new();
    let mut line = String::new();

    loop {
        print_prompt();
        line.clear();
        let _ = std::io::stdout().flush();

        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        history.add(trimmed.clone());

        let parsed = parser::parse(&trimmed);

        match parsed {
            Some(Command::Quit) => {
                let _ = cmd_tx.send(AnalyzerCommand::Quit);
                while let Ok(resp) = resp_rx.try_recv() {
                    if matches!(resp, AnalyzerResponse::Ok) {
                        break;
                    }
                }
                break;
            }
            Some(Command::Help) => {
                print_help();
            }
            Some(Command::Analyzer(cmd)) => {
                let _ = cmd_tx.send(cmd);
            }
            None => {
                eprintln!(
                    "{} Unknown command: '{}'. Type 'help' for available commands.",
                    "ERROR:".red(),
                    trimmed
                );
            }
        }

        drain_responses(&resp_rx);
    }
}

fn drain_responses(resp_rx: &mpsc::Receiver<AnalyzerResponse>) {
    while let Ok(resp) = resp_rx.try_recv() {
        match resp {
            AnalyzerResponse::Ok => {}
            AnalyzerResponse::ProbeValue {
                probe_id,
                value_bits,
            } => {
                let val = f64::from_bits(value_bits);
                println!(
                    "{} {} = {}",
                    format!("probe#{}", probe_id).cyan(),
                    "→".dimmed(),
                    val.to_string().yellow()
                );
            }
            AnalyzerResponse::ProbeValues(values) => {
                for (probe_id, value_bits) in &values {
                    let val = f64::from_bits(*value_bits);
                    println!(
                        "{} {} {}",
                        format!("probe#{}", probe_id).cyan(),
                        "→".dimmed(),
                        val.to_string().yellow()
                    );
                }
            }
            AnalyzerResponse::NodeList(list) => {
                for ni in &list {
                    println!(
                        "{} {} {} {} {}",
                        format!("[{}]", ni.node_type).magenta(),
                        ni.name.bold(),
                        format!("({} in)", ni.num_inputs).dimmed(),
                        format!("({} out)", ni.num_outputs).dimmed(),
                        if ni.num_inputs > 0 || ni.num_outputs > 0 {
                            "".to_string()
                        } else {
                            "(endpoint)".dimmed().to_string()
                        }
                    );
                }
            }
            AnalyzerResponse::ProbeList(list) => {
                for pi in &list {
                    let bp = if pi.has_breakpoint {
                        " [bp]".yellow()
                    } else {
                        "".normal()
                    };
                    println!(
                        "{}{} {} {} ({})",
                        format!("probe#{}", pi.probe_id).cyan(),
                        bp,
                        pi.name.bold(),
                        "→".dimmed(),
                        pi.node_name.dimmed()
                    );
                }
            }
            AnalyzerResponse::CommandLog(entries) => {
                for entry in &entries {
                    println!(
                        "{} {} {} {} {}",
                        format!("[block#{}]", entry.block_index).dimmed(),
                        entry.command_kind.blue(),
                        "→".to_string().dimmed(),
                        entry.node_name.bold(),
                        if let Some(ref p) = entry.param_name {
                            format!("{} = {}", p, entry.value_repr)
                        } else {
                            String::new()
                        }
                    );
                }
            }
            AnalyzerResponse::Error(msg) => {
                eprintln!("{} {}", "ERROR:".red(), msg);
            }
            AnalyzerResponse::Paused => {
                println!("{}", "[paused]".yellow());
            }
            AnalyzerResponse::AutomatonsList(list) => {
                for name in &list {
                    println!("{} {}", "automaton".cyan(), name.bold());
                }
            }
            AnalyzerResponse::AutomatonState(json) => {
                println!("{}", json);
            }
            AnalyzerResponse::SensorList(list) => {
                for name in &list {
                    println!("{} {}", "sensor".cyan(), name.bold());
                }
            }
            AnalyzerResponse::SensorStatus(json) => {
                println!("{}", json);
            }
            AnalyzerResponse::QueueList(list) => {
                for qs in &list {
                    println!(
                        "{} {} {} {}/{}",
                        "queue".cyan(),
                        qs.name.bold(),
                        "→".dimmed(),
                        qs.len,
                        qs.capacity
                    );
                }
            }
        }
    }
}

fn print_prompt() {
    print!("{} ", "(rla)".blue().bold());
}

fn print_help() {
    let title = "[rill-analyzer]".green();
    let header = "Available commands:".bold();
    println!("{} {}", title, header);
    println!(
        "  {:<20} {}",
        "q, quit".yellow(),
        "Exit the debugger".dimmed()
    );
    println!(
        "  {:<20} {}",
        "h, ?, help".yellow(),
        "Show this help".dimmed()
    );
    println!(
        "  {:<20} {}",
        "c, continue".yellow(),
        "Continue execution".dimmed()
    );
    println!(
        "  {:<20} {}",
        "s, step".yellow(),
        "Step one block and pause".dimmed()
    );
    println!("  {:<20} {}", "pause".yellow(), "Pause the engine".dimmed());
    println!(
        "  {:<20} {}",
        "p <id>, print <id>, get <id>".yellow(),
        "Print probe value".dimmed()
    );
    println!(
        "  {:<20} {}",
        "b <id>, break <id>".yellow(),
        "Set breakpoint".dimmed()
    );
    println!(
        "  {:<20} {}",
        "clear <id>, del <id>".yellow(),
        "Clear breakpoint".dimmed()
    );
    println!(
        "  {:<20} {}",
        "enable <id>".yellow(),
        "Enable probe".dimmed()
    );
    println!(
        "  {:<20} {}",
        "disable <id>".yellow(),
        "Disable probe".dimmed()
    );
    println!(
        "  {:<20} {}",
        "probes, lp".yellow(),
        "List all probes".dimmed()
    );
    println!(
        "  {:<20} {}",
        "nodes, ln".yellow(),
        "List all nodes".dimmed()
    );
    println!(
        "  {:<20} {}",
        "commands, lc".yellow(),
        "List command log".dimmed()
    );
    println!(
        "  {:<20} {}",
        "values, gv".yellow(),
        "Get all probe values".dimmed()
    );
}
