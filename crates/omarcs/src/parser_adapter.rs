use std::fs::File;
use std::path::Path;
use std::time::Instant;

use ahash::AHashMap;
use anyhow::{Context, Result};
use memmap2::MmapOptions;
use parser::first_pass::parser_settings::{ParserInputs, rm_user_friendly_names};
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
    parse_with(path, true)
}

fn parse_generic(path: &Path) -> Result<ParsedDemo> {
    parse_with(path, false)
}

fn parse_with(path: &Path, compact_player_ticks: bool) -> Result<ParsedDemo> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mmap = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("mapping {}", path.display()))?;
    let huffman = create_huffman_lookup_table();
    let (properties, real_name_to_og_name) = player_properties()?;
    let settings = ParserInputs {
        wanted_player_props: properties,
        wanted_events: EVENTS.iter().map(|event| (*event).to_owned()).collect(),
        real_name_to_og_name,
        wanted_other_props: Vec::new(),
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
        compact_player_ticks,
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
    let parsed = parse_generic(path)?;
    let output = parsed.output;

    let mut columns = output
        .df
        .keys()
        .map(|id| {
            output
                .prop_controller
                .prop_infos
                .iter()
                .find(|info| info.id == *id)
                .map(|info| info.prop_friendly_name.clone())
                .or_else(|| output.prop_controller.id_to_name.get(id).cloned())
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

fn player_properties() -> Result<(Vec<String>, AHashMap<String, String>)> {
    let friendly = PLAYER_PROPERTIES
        .iter()
        .map(|property| (*property).to_owned())
        .collect::<Vec<_>>();
    let real = rm_user_friendly_names(&friendly)
        .map_err(|error| anyhow::anyhow!("resolving player properties: {error}"))?;
    let map = real
        .iter()
        .zip(friendly)
        .map(|(real_name, friendly_name)| (real_name.clone(), friendly_name))
        .collect();
    Ok((real, map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_friendly_tick_properties_to_parser_names() {
        let (real, map) = player_properties().unwrap();
        assert!(real.contains(&"CCSPlayerPawn.m_iHealth".to_owned()));
        assert!(real.contains(&"CCSPlayerPawn.m_iTeamNum".to_owned()));
        assert!(real.contains(&"weapon_name".to_owned()));
        assert_eq!(
            map.get("CCSPlayerPawn.m_iTeamNum").map(String::as_str),
            Some("team_num")
        );
        assert_eq!(
            map.get("CCSPlayerPawn.m_bSpottedByMask")
                .map(String::as_str),
            Some("approximate_spotted_by")
        );
    }
}
