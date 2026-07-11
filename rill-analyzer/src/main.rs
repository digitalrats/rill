use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rill-analyzer", version = "0.5.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        graph: PathBuf,
        #[arg(long)]
        no_repl: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        log: Option<PathBuf>,
        #[arg(long)]
        script: Option<PathBuf>,
    },

    /// Connect to a running rill process via shared memory.
    Attach {
        /// PID of the rill process to debug.
        pid: u64,
        #[arg(long)]
        json: bool,
    },

    /// Launch a target and connect the debugger.
    /// .json = serialized graph, .rll = rill-lang source, otherwise = binary.
    Launch {
        /// Graph (.json), rill-lang source (.rll), or binary to execute.
        target: String,
        /// Extra arguments for the launched process.
        #[arg(last = true)]
        args: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run {
            graph,
            no_repl,
            json,
            log,
            script,
        } => {
            if no_repl {
                println!(
                    "{} --no-repl mode not yet implemented",
                    "[rill-analyzer]".yellow()
                );
            }
            if json {
                println!(
                    "{} --json mode not yet implemented",
                    "[rill-analyzer]".yellow()
                );
            }
            if let Some(ref path) = log {
                println!(
                    "{} --log {} mode not yet implemented",
                    "[rill-analyzer]".yellow(),
                    path.display()
                );
            }
            if let Some(ref path) = script {
                println!(
                    "{} --script {} mode not yet implemented",
                    "[rill-analyzer]".yellow(),
                    path.display()
                );
            }

            let json_str = std::fs::read_to_string(&graph).unwrap_or_else(|e| {
                eprintln!("ERROR: {}", e);
                std::process::exit(1);
            });
            let graph_def: rill_graph::serialization::GraphDef = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| {
                    eprintln!("ERROR: invalid graph JSON: {}", e);
                    std::process::exit(1);
                });
            println!(
                "{} Graph loaded: {} nodes, {} connections",
                "[rill-analyzer]".green(),
                graph_def.nodes.len(),
                graph_def.connections.len()
            );
        }

        Commands::Attach { pid, json } => {
            if json {
                println!(
                    "{} --json mode for attach not yet implemented",
                    "[rill-analyzer]".yellow()
                );
            }

            let shmem = match rill_telemetry::debug::ipc::ShmemRegion::open(pid) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ERROR: cannot attach to PID {}: {}", pid, e);
                    std::process::exit(1);
                }
            };

            // Register as debugger
            let my_pid = std::process::id() as u64;
            shmem.set_debugger_pid(my_pid);
            shmem.set_flag(rill_telemetry::debug::ipc::FLAG_ATTACHED);

            println!(
                "{} attached to process {} (shmem: /dev/shm/rill-debug-{})",
                "[rill-analyzer]".green(),
                pid,
                pid
            );

            repl_loop_shmem(shmem);
        }

        Commands::Launch { target, args, json } => {
            if json {
                println!(
                    "{} --json mode for launch not yet implemented",
                    "[rill-analyzer]".yellow()
                );
            }

            let shmem = match rill_telemetry::debug::ipc::ShmemRegion::create() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ERROR: cannot create shmem: {}", e);
                    std::process::exit(1);
                }
            };

            // use the same pid that ShmemRegion::create() used internally
            let my_pid = std::process::id() as u64;
            let shmem_env = format!("RILL_DEBUG_SHMEM=/dev/shm/rill-debug-{}", my_pid);

            let child_pid = if target.ends_with(".json") {
                // Launch drift with graph
                let graph_arg = format!("--graph={}", target);
                launch_child("drift", &[&graph_arg], &shmem_env)
            } else if target.ends_with(".rll") {
                println!("{} compiling .rll source...", "[rill-analyzer]".green());
                println!(
                    "{} .rll launch not fully implemented — compile manually to .json first",
                    "[rill-analyzer]".yellow()
                );
                std::process::exit(0);
            } else {
                // Arbitrary binary
                let args_strs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                launch_child(&target, &args_strs, &shmem_env)
            };

            println!(
                "{} launched PID {} (shmem: /dev/shm/rill-debug-{})",
                "[rill-analyzer]".green(),
                child_pid,
                my_pid
            );

            repl_loop_shmem(shmem);
        }
    }
}

/// Fork + exec a child process. Returns the child PID.
fn launch_child(binary: &str, args: &[&str], shmem_env: &str) -> u32 {
    let mut cmd = std::process::Command::new(binary);
    cmd.args(args);
    cmd.env("RILL_DEBUG_SHMEM", shmem_env);
    cmd.stdin(std::process::Stdio::null());
    let child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("ERROR: cannot launch '{}': {}", binary, e);
        std::process::exit(1);
    });
    child.id()
}

/// Simple REPL loop using shmem for command/response transport.
fn repl_loop_shmem(shmem: rill_telemetry::debug::ipc::ShmemRegion) {
    use std::io::{self, Write};

    use colored::Colorize;
    use rill_telemetry::debug::protocol::AnalyzerCommand;

    println!(
        "{} type 'help' for commands, 'quit' to exit",
        "[rill-analyzer]".green()
    );

    loop {
        print!("{} ", "(rla)".blue().bold());
        io::stdout().flush().ok();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            break;
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        if input == "q" || input == "quit" {
            shmem.set_flag(rill_telemetry::debug::ipc::FLAG_SHUTDOWN);
            break;
        }
        if input == "h" || input == "help" {
            println!(
                "  break <probe>  continue  step  info nodes  info probes  print <probe>  pause  quit"
            );
            continue;
        }

        // Simple command parsing
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = match parts[0] {
            "break" | "b" => {
                let probe_id: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                AnalyzerCommand::SetBreakpoint { probe_id }
            }
            "continue" | "c" => {
                shmem.clear_flag(rill_telemetry::debug::ipc::FLAG_PAUSED);
                AnalyzerCommand::Continue
            }
            "step" | "s" => AnalyzerCommand::Step,
            "print" | "p" => {
                let probe_id: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                AnalyzerCommand::GetProbeValue { probe_id }
            }
            "info" => match parts.get(1).copied() {
                Some("nodes") => AnalyzerCommand::ListNodes,
                _ => AnalyzerCommand::ListProbes,
            },
            "pause" => AnalyzerCommand::Pause,
            _ => {
                println!("  unknown command: {}", input);
                continue;
            }
        };

        shmem.write_command(&cmd);
        shmem.notify_process();

        // Poll responses
        std::thread::sleep(std::time::Duration::from_millis(20));
        while let Some(resp) = shmem.read_response() {
            match resp {
                rill_telemetry::debug::protocol::AnalyzerResponse::Ok => {
                    println!("  ok");
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::ProbeValue {
                    probe_id,
                    value_bits,
                } => {
                    println!("  probe#{} = {:#x} ({})", probe_id, value_bits, value_bits);
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::ProbeValues(values) => {
                    for (pid, bits) in &values {
                        println!("  probe#{} = {:#x} ({})", pid, bits, bits);
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::NodeList(nodes) => {
                    for (i, n) in nodes.iter().enumerate() {
                        println!(
                            "  #{:<4} {:<16} ({:<16}) in:{} out:{}",
                            i, n.name, n.node_type, n.num_inputs, n.num_outputs
                        );
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::ProbeList(probes) => {
                    for p in &probes {
                        let status = if p.has_breakpoint {
                            "BREAK"
                        } else if p.enabled {
                            "ON"
                        } else {
                            "OFF"
                        };
                        println!(
                            "  [{}] probe#{} {} (node={})",
                            status, p.probe_id, p.name, p.node_name
                        );
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::CommandLog(entries) => {
                    for e in &entries {
                        println!(
                            "  block#{} {} {} {}: {}",
                            e.block_index,
                            e.command_kind,
                            e.node_name,
                            e.param_name.as_deref().unwrap_or("-"),
                            e.value_repr
                        );
                    }
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::Paused => {
                    println!("{} execution paused", "PAUSED:".red().bold());
                }
                rill_telemetry::debug::protocol::AnalyzerResponse::Error(message) => {
                    println!("{} {}", "ERROR:".red(), message);
                }
            }
        }
    }
}
