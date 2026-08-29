mod match_facts;
mod metrics;
mod parser_adapter;
mod ticks;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "omarcs-native", about = "Native omarCS backend")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Exercise the native demo parser and report the extracted shape.
    Probe {
        demo: PathBuf,
        #[arg(long)]
        pretty: bool,
    },
    /// Parse a Demo into normalized Match Facts and report their shape.
    Facts {
        demo: PathBuf,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate the native core statistics for one player.
    Stats {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Probe { demo, pretty } => {
            let probe = parser_adapter::probe(&demo)?;
            if pretty {
                println!("{}", serde_json::to_string_pretty(&probe)?);
            } else {
                println!("{}", serde_json::to_string(&probe)?);
            }
        }
        Command::Facts { demo, pretty } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let summary = facts.summary(parsed.demo_bytes, parsed.elapsed_ms);
            if pretty {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("{}", serde_json::to_string(&summary)?);
            }
        }
        Command::Stats {
            demo,
            player,
            pretty,
        } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let player = metrics::resolve_player(&facts, &player)?;
            let stats = metrics::calculate(&facts, player);
            if pretty {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("{}", serde_json::to_string(&stats)?);
            }
        }
    }
    Ok(())
}
