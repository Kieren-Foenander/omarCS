use std::fs::File;
use std::path::Path;
use std::time::Instant;

use ahash::AHashMap;
use anyhow::{Context, Result};
use memmap2::MmapOptions;
use parser::first_pass::parser_settings::ParserInputs;
use parser::parse_demo::{DemoOutput, Parser, ParsingMode};
use parser::second_pass::parser_settings::create_huffman_lookup_table;
use serde::Serialize;

const PLAYER_PROPERTIES: &[&str] = &[
    "tick",
    "steamid",
    "name",
    "team_num",
    "health",
    "pitch",
    "yaw",
    "X",
    "Y",
    "Z",
    "duck_amount",
    "velocity",
    "approximate_spotted_by",
    "active_weapon_name",
    "shots_fired",
];

const EVENTS: &[&str] = &[
    "round_start",
    "round_freeze_end",
    "round_end",
    "round_officially_ended",
    "player_death",
    "player_hurt",
    "player_blind",
    "weapon_fire",
    "fire_bullets",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserProbe {
    parser_revision: &'static str,
    demo_bytes: usize,
    map: Option<String>,
    rows: usize,
    events: usize,
    columns: Vec<String>,
    elapsed_ms: f64,
}

pub struct ParsedDemo {
    pub output: DemoOutput,
    pub demo_bytes: usize,
    pub elapsed_ms: f64,
}

pub fn parse(path: &Path) -> Result<ParsedDemo> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mmap = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("mapping {}", path.display()))?;
    let huffman = create_huffman_lookup_table();
    let properties = PLAYER_PROPERTIES
        .iter()
        .map(|property| (*property).to_owned())
        .collect::<Vec<_>>();
    let settings = ParserInputs {
        wanted_player_props: properties.clone(),
        wanted_events: EVENTS.iter().map(|event| (*event).to_owned()).collect(),
        real_name_to_og_name: AHashMap::default(),
        wanted_other_props: properties,
        parse_ents: true,
        wanted_players: Vec::new(),
        wanted_ticks: Vec::new(),
        parse_projectiles: false,
        parse_grenades: false,
        only_header: false,
        list_props: false,
        only_convars: false,
        huffman_lookup_table: &huffman,
        order_by_steamid: false,
        wanted_prop_states: AHashMap::default(),
        fallback_bytes: None,
    };

    let started = Instant::now();
    let mut parser = Parser::new(settings, ParsingMode::Normal);
    let output = parser
        .parse_demo(&mmap)
        .map_err(|error| anyhow::anyhow!("parsing {}: {error}", path.display()))?;
    Ok(ParsedDemo {
        output,
        demo_bytes: mmap.len(),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

pub fn probe(path: &Path) -> Result<ParserProbe> {
    let parsed = parse(path)?;
    let output = parsed.output;

    let mut columns = output
        .df
        .keys()
        .map(|id| {
            output
                .prop_controller
                .id_to_name
                .get(id)
                .cloned()
                .or_else(|| {
                    output
                        .prop_controller
                        .name_to_special_id
                        .iter()
                        .find_map(|(name, special_id)| (special_id == id).then(|| name.clone()))
                })
                .unwrap_or_else(|| format!("property_{id}"))
        })
        .collect::<Vec<_>>();
    columns.sort();
    columns.dedup();
    let rows = output
        .df
        .values()
        .map(|column| column.len())
        .max()
        .unwrap_or(0);
    let map = output
        .header
        .as_ref()
        .and_then(|header| header.get("map_name"))
        .cloned();

    Ok(ParserProbe {
        parser_revision: "57f24c76776ac176e893833f3a5b4aad718a8196",
        demo_bytes: parsed.demo_bytes,
        map,
        rows,
        events: output.game_events.len(),
        columns,
        elapsed_ms: parsed.elapsed_ms,
    })
}
