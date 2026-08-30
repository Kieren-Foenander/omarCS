mod application;
mod autofetch;
mod coaching;
mod config;
mod geometry;
mod match_facts;
mod mechanics;
mod metrics;
mod parser_adapter;
mod report;
mod spray;
mod storage;
mod ticks;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "omarcs", about = "Local CS2 match analysis for Omarchy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import one Demo or a directory of Demos.
    Import {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// SteamID64 or exact in-demo player name.
        #[arg(long)]
        player: Option<String>,
        #[arg(long)]
        quiet: bool,
    },
    /// Scan configured Demo directories.
    Refresh {
        /// SteamID64 or exact in-demo player name.
        #[arg(long)]
        player: Option<String>,
        #[arg(long)]
        quiet: bool,
    },
    /// Print the current Dashboard Summary.
    Status {
        #[arg(long)]
        pretty: bool,
    },
    /// Install and enable automatic Valve Demo fetching.
    SetupAuto,
    #[command(hide = true)]
    Bootstrap,
    #[command(hide = true)]
    AutoRun,
    /// Print automatic fetcher state.
    AutoStatus {
        #[arg(long)]
        pretty: bool,
    },
    /// Exercise the Demo parser and report the extracted shape.
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
    /// Calculate the core statistics for one player.
    Stats {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate Engagement mechanics for one player.
    Mechanics {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate Sprays for one player.
    Sprays {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Calculate coaching insights for one player.
    Insights {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
    /// Assemble a Match Report for one player.
    Report {
        demo: PathBuf,
        /// SteamID64 or exact in-demo player name.
        player: String,
        #[arg(long)]
        pretty: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    match cli.command {
        Command::Import {
            paths,
            player,
            quiet,
        } => {
            return application::import_paths(&paths, player.as_deref(), quiet);
        }
        Command::Refresh { player, quiet } => {
            return application::refresh(player.as_deref(), quiet);
        }
        Command::Status { pretty } => {
            let store = storage::Store::open(None)?;
            print_json(&store.current_summary()?, pretty)?;
        }
        Command::SetupAuto => {
            autofetch::setup_auto(true)?;
            autofetch::enable_daemon()?;
            autofetch::remove_legacy_runtime()?;
            println!("Automatic match fetching is enabled.");
        }
        Command::Bootstrap => return autofetch::bootstrap(),
        Command::AutoRun => return autofetch::run_daemon(),
        Command::AutoStatus { pretty } => print_json(&autofetch::load_state(), pretty)?,
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
    Ok(0)
}

fn print_json(value: &impl Serialize, pretty: bool) -> Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}

#[cfg(test)]
mod presentation_tests {
    const PANEL: &str = include_str!("../../../Panel.qml");

    #[test]
    fn every_local_text_control_forces_plain_text() {
        assert_eq!(
            PANEL.matches("Text {").count(),
            PANEL.matches("textFormat: Text.PlainText").count()
        );
    }

    #[test]
    fn demo_strings_passed_to_shared_modules_are_neutralized() {
        assert!(
            PANEL.contains(
                "tooltipText: root.selectedMatch ? root.safeText(root.selectedMatch.map)"
            )
        );
        assert!(
            PANEL.contains("title: root.selectedMatch ? root.safeText(root.selectedMatch.map)")
        );
        assert!(PANEL.contains("root.safeText(root.selectedMatch.player.name)"));
    }
}
