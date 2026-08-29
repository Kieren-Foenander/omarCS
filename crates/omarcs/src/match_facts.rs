use std::collections::{BTreeMap, BTreeSet};

use parser::parse_demo::DemoOutput;
use parser::second_pass::game_events::GameEvent;
use parser::second_pass::variants::Variant;
use serde::Serialize;

use crate::ticks::TickObservations;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PlayerId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Side {
    #[serde(rename = "T")]
    Terrorist,
    #[serde(rename = "CT")]
    CounterTerrorist,
    Unknown,
}

impl Side {
    pub(crate) fn from_team_num(value: u32) -> Self {
        match value {
            2 => Self::Terrorist,
            3 => Self::CounterTerrorist,
            _ => Self::Unknown,
        }
    }

    fn parse(value: Option<&Variant>) -> Self {
        match value.and_then(as_string).as_deref() {
            Some("T") | Some("TERRORIST") => Self::Terrorist,
            Some("CT") => Self::CounterTerrorist,
            _ => match value.and_then(as_i32) {
                Some(2) => Self::Terrorist,
                Some(3) => Self::CounterTerrorist,
                _ => Self::Unknown,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerFact {
    pub steam_id: PlayerId,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundMarker {
    pub tick: i32,
    pub kind: RoundMarkerKind,
    pub winner: Side,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundMarkerKind {
    Start,
    FreezeEnd,
    End,
    OfficialEnd,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundFact {
    pub number: usize,
    pub start_tick: i32,
    pub freeze_end_tick: Option<i32>,
    pub end_tick: i32,
    pub official_end_tick: i32,
    pub winner: Side,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillFact {
    pub tick: i32,
    pub attacker: Option<PlayerId>,
    pub victim: Option<PlayerId>,
    pub assister: Option<PlayerId>,
    pub attacker_side: Side,
    pub victim_side: Side,
    pub headshot: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageFact {
    pub tick: i32,
    pub attacker: Option<PlayerId>,
    pub victim: Option<PlayerId>,
    pub attacker_side: Side,
    pub victim_side: Side,
    pub weapon: String,
    pub health_damage: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlindFact {
    pub tick: i32,
    pub attacker: Option<PlayerId>,
    pub victim: Option<PlayerId>,
    pub attacker_side: Side,
    pub victim_side: Side,
    pub duration_seconds: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotFact {
    pub tick: i32,
    pub player: Option<PlayerId>,
    pub side: Side,
    pub weapon: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulletFact {
    pub tick: i32,
    pub player: Option<PlayerId>,
    pub item_definition: Option<i32>,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
}

#[derive(Debug)]
pub struct MatchFacts {
    pub map: String,
    pub players: Vec<PlayerFact>,
    pub rounds: Vec<RoundFact>,
    pub kills: Vec<KillFact>,
    pub damages: Vec<DamageFact>,
    pub blinds: Vec<BlindFact>,
    pub shots: Vec<ShotFact>,
    pub bullets: Vec<BulletFact>,
    pub ticks: TickObservations,
    pub tick_rows: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchFactsSummary {
    pub map: String,
    pub demo_bytes: usize,
    pub parser_elapsed_ms: f64,
    pub tick_rows: usize,
    pub tick_observations: usize,
    pub players: Vec<PlayerFact>,
    pub rounds: usize,
    pub kills: usize,
    pub damages: usize,
    pub blinds: usize,
    pub shots: usize,
    pub bullets: usize,
}

impl MatchFacts {
    pub fn from_output(output: DemoOutput) -> Self {
        let map = output
            .header
            .as_ref()
            .and_then(|header| header.get("map_name"))
            .cloned()
            .unwrap_or_default();
        let tick_rows = output
            .df
            .values()
            .map(|column| column.len())
            .max()
            .unwrap_or(0);
        let mut players = BTreeMap::<PlayerId, String>::new();
        let mut round_markers = Vec::new();
        let mut kills = Vec::new();
        let mut damages = Vec::new();
        let mut blinds = Vec::new();
        let mut shots = Vec::new();
        let mut bullets = Vec::new();

        for event in &output.game_events {
            collect_players(event, &mut players);
            match event.name.as_str() {
                "round_start" => round_markers.push(round_marker(event, RoundMarkerKind::Start)),
                "round_freeze_end" => {
                    round_markers.push(round_marker(event, RoundMarkerKind::FreezeEnd));
                }
                "round_end" => round_markers.push(round_marker(event, RoundMarkerKind::End)),
                "round_officially_ended" => {
                    round_markers.push(round_marker(event, RoundMarkerKind::OfficialEnd));
                }
                "player_death" => kills.push(KillFact {
                    tick: event.tick,
                    attacker: player_id(event, "attacker_steamid"),
                    victim: player_id(event, "user_steamid"),
                    assister: player_id(event, "assister_steamid"),
                    attacker_side: player_side(event, "attacker"),
                    victim_side: player_side(event, "user"),
                    headshot: field(event, "headshot").and_then(as_bool).unwrap_or(false),
                }),
                "player_hurt" => damages.push(DamageFact {
                    tick: event.tick,
                    attacker: player_id(event, "attacker_steamid"),
                    victim: player_id(event, "user_steamid"),
                    attacker_side: player_side(event, "attacker"),
                    victim_side: player_side(event, "user"),
                    weapon: field(event, "weapon")
                        .and_then(as_string)
                        .unwrap_or_default(),
                    health_damage: real_damage(event),
                }),
                "player_blind" => blinds.push(BlindFact {
                    tick: event.tick,
                    attacker: player_id(event, "attacker_steamid"),
                    victim: player_id(event, "user_steamid"),
                    attacker_side: player_side(event, "attacker"),
                    victim_side: player_side(event, "user"),
                    duration_seconds: field(event, "blind_duration")
                        .and_then(as_f32)
                        .unwrap_or(0.0),
                }),
                "weapon_fire" => shots.push(ShotFact {
                    tick: event.tick,
                    player: player_id(event, "user_steamid"),
                    side: player_side(event, "user"),
                    weapon: field(event, "weapon")
                        .and_then(as_string)
                        .unwrap_or_default(),
                }),
                "fire_bullets" => bullets.push(BulletFact {
                    tick: event.tick,
                    player: player_id(event, "user_steamid"),
                    item_definition: field(event, "item_def_index").and_then(as_i32),
                    origin: vector(event, "origin"),
                    angles: vector(event, "angles"),
                }),
                _ => {}
            }
        }

        round_markers.sort_by_key(|marker| marker.tick);
        let rounds = reconstruct_rounds(round_markers);
        let ticks = TickObservations::from_demo(&output, &rounds, &mut players);
        let players = players
            .into_iter()
            .map(|(steam_id, name)| PlayerFact { steam_id, name })
            .collect();

        Self {
            map,
            players,
            rounds,
            kills,
            damages,
            blinds,
            shots,
            bullets,
            ticks,
            tick_rows,
        }
    }

    pub fn summary(self, demo_bytes: usize, parser_elapsed_ms: f64) -> MatchFactsSummary {
        MatchFactsSummary {
            map: self.map,
            demo_bytes,
            parser_elapsed_ms,
            tick_rows: self.tick_rows,
            tick_observations: self.ticks.len(),
            players: self.players,
            rounds: self.rounds.len(),
            kills: self.kills.len(),
            damages: self.damages.len(),
            blinds: self.blinds.len(),
            shots: self.shots.len(),
            bullets: self.bullets.len(),
        }
    }
}

fn reconstruct_rounds(markers: Vec<RoundMarker>) -> Vec<RoundFact> {
    struct PendingRound {
        start_tick: i32,
        freeze_end_tick: Option<i32>,
        end_tick: Option<i32>,
        official_end_tick: Option<i32>,
        winner: Side,
    }

    fn finish(pending: Option<PendingRound>, rounds: &mut Vec<RoundFact>) {
        let Some(pending) = pending else { return };
        let Some(end_tick) = pending.end_tick else {
            return;
        };
        rounds.push(RoundFact {
            number: rounds.len() + 1,
            start_tick: pending.start_tick,
            freeze_end_tick: pending.freeze_end_tick,
            end_tick,
            official_end_tick: pending.official_end_tick.unwrap_or(end_tick),
            winner: pending.winner,
        });
    }

    let mut rounds = Vec::new();
    let mut pending: Option<PendingRound> = None;
    for marker in markers {
        match marker.kind {
            RoundMarkerKind::Start => {
                finish(pending.take(), &mut rounds);
                pending = Some(PendingRound {
                    start_tick: marker.tick,
                    freeze_end_tick: None,
                    end_tick: None,
                    official_end_tick: None,
                    winner: Side::Unknown,
                });
            }
            RoundMarkerKind::FreezeEnd => {
                if let Some(round) = &mut pending {
                    round.freeze_end_tick = Some(marker.tick);
                }
            }
            RoundMarkerKind::End => {
                if let Some(round) = &mut pending {
                    round.end_tick = Some(marker.tick);
                    round.winner = marker.winner;
                }
            }
            RoundMarkerKind::OfficialEnd => {
                if let Some(round) = &mut pending {
                    round.official_end_tick = Some(marker.tick);
                }
            }
        }
    }
    finish(pending, &mut rounds);
    rounds
}

pub(crate) fn round_index_for_tick(rounds: &[RoundFact], tick: i32) -> Option<usize> {
    let index = rounds
        .partition_point(|round| round.start_tick <= tick)
        .checked_sub(1)?;
    (tick <= rounds[index].official_end_tick).then_some(index)
}

fn round_marker(event: &GameEvent, kind: RoundMarkerKind) -> RoundMarker {
    RoundMarker {
        tick: event.tick,
        kind,
        winner: Side::parse(field(event, "winner")),
    }
}

fn collect_players(event: &GameEvent, players: &mut BTreeMap<PlayerId, String>) {
    let mut prefixes = BTreeSet::new();
    for event_field in &event.fields {
        if let Some(prefix) = event_field.name.strip_suffix("_steamid") {
            prefixes.insert(prefix);
        }
    }
    for prefix in prefixes {
        let Some(id) = player_id(event, &format!("{prefix}_steamid")) else {
            continue;
        };
        if id.0 == 0 {
            continue;
        }
        let name = field(event, &format!("{prefix}_name"))
            .and_then(as_string)
            .unwrap_or_default();
        if !name.is_empty() {
            players.insert(id, name);
        }
    }
}

fn real_damage(event: &GameEvent) -> i32 {
    let damage = field(event, "dmg_health")
        .and_then(as_i32)
        .unwrap_or(0)
        .max(0);
    let health = field(event, "user_health")
        .and_then(as_i32)
        .unwrap_or(damage)
        .max(0);
    damage.min(health)
}

fn player_id(event: &GameEvent, name: &str) -> Option<PlayerId> {
    field(event, name)
        .and_then(as_u64)
        .filter(|id| *id != 0)
        .map(PlayerId)
}

fn side(event: &GameEvent, name: &str) -> Side {
    Side::parse(field(event, name))
}

fn player_side(event: &GameEvent, prefix: &str) -> Side {
    let team_num = format!("{prefix}_team_num");
    let parsed = side(event, &team_num);
    if parsed != Side::Unknown {
        return parsed;
    }
    side(event, &format!("{prefix}_team_name"))
}

fn vector(event: &GameEvent, prefix: &str) -> [f32; 3] {
    [
        field(event, &format!("{prefix}_x"))
            .and_then(as_f32)
            .unwrap_or(0.0),
        field(event, &format!("{prefix}_y"))
            .and_then(as_f32)
            .unwrap_or(0.0),
        field(event, &format!("{prefix}_z"))
            .and_then(as_f32)
            .unwrap_or(0.0),
    ]
}

fn field<'a>(event: &'a GameEvent, name: &str) -> Option<&'a Variant> {
    event
        .fields
        .iter()
        .find(|event_field| event_field.name == name)
        .and_then(|event_field| event_field.data.as_ref())
}

fn as_bool(value: &Variant) -> Option<bool> {
    match value {
        Variant::Bool(value) => Some(*value),
        Variant::I32(value) => Some(*value != 0),
        Variant::U32(value) => Some(*value != 0),
        _ => None,
    }
}

fn as_f32(value: &Variant) -> Option<f32> {
    match value {
        Variant::F32(value) => Some(*value),
        Variant::I32(value) => Some(*value as f32),
        Variant::U32(value) => Some(*value as f32),
        _ => None,
    }
}

fn as_i32(value: &Variant) -> Option<i32> {
    match value {
        Variant::I32(value) => Some(*value),
        Variant::U32(value) => i32::try_from(*value).ok(),
        Variant::U64(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

fn as_u64(value: &Variant) -> Option<u64> {
    match value {
        Variant::U64(value) => Some(*value),
        Variant::U32(value) => Some(u64::from(*value)),
        Variant::I32(value) => u64::try_from(*value).ok(),
        Variant::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn as_string(value: &Variant) -> Option<String> {
    match value {
        Variant::String(value) => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parser::second_pass::game_events::EventField;
    use std::path::Path;

    fn event(name: &str, fields: Vec<(&str, Variant)>) -> GameEvent {
        GameEvent {
            name: name.to_owned(),
            tick: 42,
            fields: fields
                .into_iter()
                .map(|(name, data)| EventField {
                    name: name.to_owned(),
                    data: Some(data),
                })
                .collect(),
        }
    }

    #[test]
    fn normalizes_player_identity_side_and_real_damage() {
        let hurt = event(
            "player_hurt",
            vec![
                ("attacker_steamid", Variant::U64(76561198000000001)),
                ("attacker_name", Variant::String("Attacker".to_owned())),
                ("attacker_team_name", Variant::String("CT".to_owned())),
                (
                    "user_steamid",
                    Variant::String("76561198000000002".to_owned()),
                ),
                ("user_name", Variant::String("Victim".to_owned())),
                ("user_team_name", Variant::String("TERRORIST".to_owned())),
                ("dmg_health", Variant::I32(70)),
                ("user_health", Variant::I32(24)),
            ],
        );
        let mut players = BTreeMap::new();

        collect_players(&hurt, &mut players);

        assert_eq!(players.len(), 2);
        assert_eq!(players[&PlayerId(76561198000000001)], "Attacker");
        assert_eq!(side(&hurt, "attacker_team_name"), Side::CounterTerrorist);
        assert_eq!(side(&hurt, "user_team_name"), Side::Terrorist);
        assert_eq!(real_damage(&hurt), 24);
        assert_eq!(Side::from_team_num(2), Side::Terrorist);
        assert_eq!(Side::from_team_num(3), Side::CounterTerrorist);
        assert_eq!(Side::from_team_num(1), Side::Unknown);
    }

    #[test]
    fn normalizes_bullet_vectors() {
        let bullet = event(
            "fire_bullets",
            vec![
                ("origin_x", Variant::F32(1.0)),
                ("origin_y", Variant::F32(2.0)),
                ("origin_z", Variant::F32(3.0)),
                ("angles_x", Variant::F32(4.0)),
                ("angles_y", Variant::F32(5.0)),
                ("angles_z", Variant::F32(6.0)),
            ],
        );

        assert_eq!(vector(&bullet, "origin"), [1.0, 2.0, 3.0]);
        assert_eq!(vector(&bullet, "angles"), [4.0, 5.0, 6.0]);
    }

    #[test]
    #[ignore = "requires the upstream 60.6 MB fixture in OMARCS_DEMO_FIXTURE"]
    fn upstream_fixture_normalizes_expected_match_facts() {
        let path = std::env::var_os("OMARCS_DEMO_FIXTURE").expect("OMARCS_DEMO_FIXTURE");
        let parsed = crate::parser_adapter::parse(Path::new(&path)).expect("parse fixture");
        let facts = MatchFacts::from_output(parsed.output);

        assert_eq!(facts.map, "de_mirage");
        assert_eq!(facts.tick_rows, 579_700);
        assert_eq!(facts.players.len(), 10);
        assert_eq!(facts.rounds.len(), 10);
        assert_eq!(facts.kills.len(), 73);
        assert_eq!(facts.damages.len(), 264);
        assert_eq!(facts.blinds.len(), 72);
        assert_eq!(facts.shots.len(), 1_590);
        assert_eq!(facts.bullets.len(), 1_242);
        assert_eq!(facts.ticks.len(), 565_963);
        assert_eq!(facts.ticks.unique_players().len(), 10);
        for index in 0..facts.ticks.len() {
            assert!(facts.ticks.round_index(index) < facts.rounds.len());
            assert_ne!(facts.ticks.side(index), Side::Unknown);
        }
        let player = facts.players[0].steam_id;
        let sides = facts.ticks.majority_sides(player);
        assert_eq!(sides.len(), facts.rounds.len());
    }

    #[test]
    fn reconstructs_rounds_with_optional_freeze_and_official_end_markers() {
        let rounds = reconstruct_rounds(vec![
            RoundMarker {
                tick: 10,
                kind: RoundMarkerKind::Start,
                winner: Side::Unknown,
            },
            RoundMarker {
                tick: 20,
                kind: RoundMarkerKind::FreezeEnd,
                winner: Side::Unknown,
            },
            RoundMarker {
                tick: 100,
                kind: RoundMarkerKind::End,
                winner: Side::Terrorist,
            },
            RoundMarker {
                tick: 110,
                kind: RoundMarkerKind::OfficialEnd,
                winner: Side::Unknown,
            },
            RoundMarker {
                tick: 120,
                kind: RoundMarkerKind::Start,
                winner: Side::Unknown,
            },
            RoundMarker {
                tick: 200,
                kind: RoundMarkerKind::End,
                winner: Side::CounterTerrorist,
            },
        ]);

        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].freeze_end_tick, Some(20));
        assert_eq!(rounds[0].official_end_tick, 110);
        assert_eq!(rounds[1].freeze_end_tick, None);
        assert_eq!(rounds[1].official_end_tick, 200);
    }
}
