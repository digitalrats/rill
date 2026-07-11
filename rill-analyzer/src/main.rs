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
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { graph, .. } => {
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
    }
}
