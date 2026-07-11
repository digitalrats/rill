use rill_telemetry::debug::protocol::AnalyzerCommand;

#[derive(Debug, Clone)]
pub enum Command {
    Quit,
    Help,
    Analyzer(AnalyzerCommand),
}

pub fn parse(line: &str) -> Option<Command> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (cmd, args) = split_first(trimmed);

    match cmd {
        "q" | "quit" => Some(Command::Quit),
        "h" | "?" | "help" => Some(Command::Help),
        "c" | "continue" => Some(Command::Analyzer(AnalyzerCommand::Continue)),
        "s" | "step" => Some(Command::Analyzer(AnalyzerCommand::Step)),
        "pause" => Some(Command::Analyzer(AnalyzerCommand::Pause)),
        "probes" | "lp" => Some(Command::Analyzer(AnalyzerCommand::ListProbes)),
        "nodes" | "ln" => Some(Command::Analyzer(AnalyzerCommand::ListNodes)),
        "commands" | "lc" => Some(Command::Analyzer(AnalyzerCommand::ListCommands)),
        "values" | "gv" => Some(Command::Analyzer(AnalyzerCommand::GetProbeValues)),
        "p" | "print" | "get" => {
            let probe_id = parse_probe_id(args)?;
            Some(Command::Analyzer(AnalyzerCommand::GetProbeValue {
                probe_id,
            }))
        }
        "b" | "break" => {
            let probe_id = parse_probe_id(args)?;
            Some(Command::Analyzer(AnalyzerCommand::SetBreakpoint {
                probe_id,
            }))
        }
        "clear" | "del" => {
            let probe_id = parse_probe_id(args)?;
            Some(Command::Analyzer(AnalyzerCommand::ClearBreakpoint {
                probe_id,
            }))
        }
        "enable" => {
            let probe_id = parse_probe_id(args)?;
            Some(Command::Analyzer(AnalyzerCommand::EnableProbe { probe_id }))
        }
        "disable" => {
            let probe_id = parse_probe_id(args)?;
            Some(Command::Analyzer(AnalyzerCommand::DisableProbe {
                probe_id,
            }))
        }
        _ => {
            let matches = find_prefix_match(cmd, args);
            matches
        }
    }
}

fn find_prefix_match(cmd: &str, args: &str) -> Option<Command> {
    let lower = cmd.to_lowercase();

    if "continue".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::Continue));
    }
    if "step".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::Step));
    }
    if "pause".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::Pause));
    }
    if "quit".starts_with(&lower) {
        return Some(Command::Quit);
    }
    if "help".starts_with(&lower) {
        return Some(Command::Help);
    }
    if "print".starts_with(&lower) {
        return parse_probe_id(args)
            .map(|id| Command::Analyzer(AnalyzerCommand::GetProbeValue { probe_id: id }));
    }
    if "break".starts_with(&lower) {
        return parse_probe_id(args)
            .map(|id| Command::Analyzer(AnalyzerCommand::SetBreakpoint { probe_id: id }));
    }
    if "probes".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::ListProbes));
    }
    if "nodes".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::ListNodes));
    }
    if "commands".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::ListCommands));
    }
    if "values".starts_with(&lower) {
        return Some(Command::Analyzer(AnalyzerCommand::GetProbeValues));
    }

    None
}

fn split_first(input: &str) -> (&str, &str) {
    match input.find(char::is_whitespace) {
        Some(pos) => {
            let (cmd, rest) = input.split_at(pos);
            (cmd, rest.trim_start())
        }
        None => (input, ""),
    }
}

fn parse_probe_id(args: &str) -> Option<u32> {
    if args.is_empty() {
        return None;
    }
    args.split_whitespace().next()?.parse::<u32>().ok()
}
