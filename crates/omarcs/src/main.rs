mod coaching;
mod geometry;
mod match_facts;
mod mechanics;
mod metrics;
mod parser_adapter;
mod report;
mod spray;
mod ticks;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

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
    /// Calculate native Engagement mechanics for one player.
    Mechanics {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate native Sprays for one player.
    Sprays {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate native coaching insights for one player.
    Insights {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Assemble a native Match Report for one player.
    Report {
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
            print_json(&parser_adapter::probe(&demo)?, pretty)?;
        }
        Command::Facts { demo, pretty } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            print_json(&facts.summary(parsed.demo_bytes, parsed.elapsed_ms), pretty)?;
        }
        Command::Stats {
            demo,
            player,
            pretty,
        } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let player = metrics::resolve_player(&facts, &player)?;
            print_json(&metrics::calculate(&facts, player), pretty)?;
        }
        Command::Mechanics {
            demo,
            player,
            pretty,
        } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let player = metrics::resolve_player(&facts, &player)?;
            let mesh = geometry::load_map_mesh(&facts.map);
            print_json(&mechanics::calculate(&facts, player, mesh.as_ref()), pretty)?;
        }
        Command::Sprays {
            demo,
            player,
            pretty,
        } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let player = metrics::resolve_player(&facts, &player)?;
            let mesh = geometry::load_map_mesh(&facts.map);
            print_json(&spray::calculate(&facts, player, mesh.as_ref()), pretty)?;
        }
        Command::Insights {
            demo,
            player,
            pretty,
        } => {
            let parsed = parser_adapter::parse(&demo)?;
            let facts = match_facts::MatchFacts::from_output(parsed.output);
            let player = metrics::resolve_player(&facts, &player)?;
            let mesh = geometry::load_map_mesh(&facts.map);
            let stats = metrics::calculate(&facts, player);
            let mechanics = mechanics::calculate(&facts, player, mesh.as_ref());
            print_json(&coaching::calculate(&stats, &mechanics), pretty)?;
        }
        Command::Report {
            demo,
            player,
            pretty,
        } => {
            print_json(&report::generate(&demo, &player)?, pretty)?;
        }
    }
    Ok(())
}

fn print_json(value: &impl Serialize, pretty: bool) -> Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}
