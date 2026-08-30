use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::match_facts::{KillFact, MatchFacts, PlayerId, Side, round_index_for_tick};

const CS2_TICKS_PER_SECOND: i32 = 64;
const TRADE_WINDOW_SECONDS: i32 = 5;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMetrics {
    pub steam_id: PlayerId,
    pub name: String,
    pub kills: usize,
    pub deaths: usize,
    pub assists: usize,
    pub kd: f64,
    pub adr: f64,
    pub kast: f64,
    pub rating: f64,
    pub headshot_percent: f64,
    pub opening_kills: usize,
    pub opening_deaths: usize,
    pub trade_kills: usize,
    pub traded_deaths: usize,
    pub utility_damage: i32,
    pub enemies_flashed: usize,
    pub friends_flashed: usize,
    pub enemy_flash_seconds: f64,
    pub rounds: usize,
    pub rounds_for: usize,
    pub rounds_against: usize,
    pub result: &'static str,
}

pub fn resolve_player(facts: &MatchFacts, selector: &str) -> Result<PlayerId> {
    if let Ok(id) = selector.parse::<u64>() {
        let id = PlayerId(id);
        if facts.players.iter().any(|player| player.steam_id == id) {
            return Ok(id);
        }
    }

    let mut matches = facts
        .players
        .iter()
        .filter(|player| player.name.eq_ignore_ascii_case(selector));
    let first = matches.next();
    if let (Some(player), None) = (first, matches.next()) {
        return Ok(player.steam_id);
    }

    let choices = facts
        .players
        .iter()
        .map(|player| player.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("player {selector:?} was not found; available players: {choices}")
}

pub fn calculate(facts: &MatchFacts, player: PlayerId) -> PlayerMetrics {
    let in_match =
        |tick: i32| facts.rounds.is_empty() || round_index_for_tick(&facts.rounds, tick).is_some();
    let enemy_kills = facts
        .kills
        .iter()
        .enumerate()
        .filter(|(_, kill)| in_match(kill.tick) && is_enemy_kill(kill))
        .collect::<Vec<_>>();
    let (traded_deaths, trade_kills) = trade_flags(facts, &enemy_kills);

    let kills = enemy_kills
        .iter()
        .filter(|(_, kill)| kill.attacker == Some(player))
        .count();
    let deaths = enemy_kills
        .iter()
        .filter(|(_, kill)| kill.victim == Some(player))
        .count();
    let assists = enemy_kills
        .iter()
        .filter(|(_, kill)| kill.assister == Some(player))
        .count();
    let headshots = enemy_kills
        .iter()
        .filter(|(_, kill)| kill.attacker == Some(player) && kill.headshot)
        .count();

    let mut round_events = vec![RoundEvents::default(); facts.rounds.len()];
    let mut first_kill_by_round = BTreeMap::<usize, &KillFact>::new();
    for (source_index, kill) in &enemy_kills {
        let Some(round_index) = round_index_for_tick(&facts.rounds, kill.tick) else {
            continue;
        };
        first_kill_by_round
            .entry(round_index)
            .and_modify(|first| {
                if kill.tick < first.tick {
                    *first = kill;
                }
            })
            .or_insert(kill);
        let event = &mut round_events[round_index];
        event.kill |= kill.attacker == Some(player);
        event.assist |= kill.assister == Some(player);
        if kill.victim == Some(player) {
            event.died = true;
            event.traded |= traded_deaths.contains(source_index);
        }
    }

    let kast_rounds = round_events
        .iter()
        .filter(|event| event.kill || event.assist || !event.died || event.traded)
        .count();
    let opening_kills = first_kill_by_round
        .values()
        .filter(|kill| kill.attacker == Some(player))
        .count();
    let opening_deaths = first_kill_by_round
        .values()
        .filter(|kill| kill.victim == Some(player))
        .count();

    let mut damage = 0;
    let mut utility_damage = 0;
    for fact in facts.damages.iter().filter(|fact| {
        in_match(fact.tick)
            && fact.attacker == Some(player)
            && fact.victim != Some(player)
            && (fact.attacker_side == Side::Unknown
                || fact.victim_side == Side::Unknown
                || fact.attacker_side != fact.victim_side)
    }) {
        let amount = fact.health_damage.max(0);
        damage += amount;
        if is_utility_weapon(&fact.weapon) {
            utility_damage += amount;
        }
    }

    let mut enemies_flashed = 0;
    let mut friends_flashed = 0;
    let mut enemy_flash_seconds = 0.0_f64;
    for blind in facts.blinds.iter().filter(|blind| {
        in_match(blind.tick) && blind.attacker == Some(player) && blind.victim != Some(player)
    }) {
        if blind.attacker_side != Side::Unknown && blind.attacker_side == blind.victim_side {
            friends_flashed += 1;
        } else {
            enemies_flashed += 1;
            enemy_flash_seconds += f64::from(blind.duration_seconds.max(0.0));
        }
    }

    let round_sides = infer_round_sides(facts, player);
    let (mut rounds_for, mut rounds_against) = (0, 0);
    for (round_index, round) in facts.rounds.iter().enumerate() {
        let Some(side) = round_sides.get(&round_index) else {
            continue;
        };
        if round.winner == Side::Unknown {
            continue;
        }
        if round.winner == *side {
            rounds_for += 1;
        } else {
            rounds_against += 1;
        }
    }

    let round_count = facts.rounds.len();
    let denominator = round_count.max(1) as f64;
    let kpr = kills as f64 / denominator;
    let apr = assists as f64 / denominator;
    let dpr = deaths as f64 / denominator;
    let adr = f64::from(damage) / denominator;
    let kast = 100.0 * kast_rounds as f64 / denominator;
    let impact = 2.13 * kpr + 0.42 * apr - 0.41;
    let rating =
        0.0073 * kast + 0.3591 * kpr - 0.5329 * dpr + 0.2372 * impact + 0.0032 * adr + 0.1587;

    let result = match rounds_for.cmp(&rounds_against) {
        std::cmp::Ordering::Greater => "W",
        std::cmp::Ordering::Less => "L",
        std::cmp::Ordering::Equal => "D",
    };
    let name = facts
        .players
        .iter()
        .find(|candidate| candidate.steam_id == player)
        .map(|candidate| candidate.name.clone())
        .unwrap_or_default();

    PlayerMetrics {
        steam_id: player,
        name,
        kills,
        deaths,
        assists,
        kd: round_to(kills as f64 / deaths.max(1) as f64, 2),
        adr: round_to(adr, 1),
        kast: round_to(kast, 1),
        rating: round_to(rating, 2),
        headshot_percent: round_to(100.0 * headshots as f64 / kills.max(1) as f64, 1),
        opening_kills,
        opening_deaths,
        trade_kills: trade_kills
            .iter()
            .filter(|index| facts.kills[**index].attacker == Some(player))
            .count(),
        traded_deaths: traded_deaths
            .iter()
            .filter(|index| facts.kills[**index].victim == Some(player))
            .count(),
        utility_damage,
        enemies_flashed,
        friends_flashed,
        enemy_flash_seconds: round_to(enemy_flash_seconds, 1),
        rounds: round_count,
        rounds_for,
        rounds_against,
        result,
    }
}

#[derive(Clone, Default)]
struct RoundEvents {
    kill: bool,
    assist: bool,
    died: bool,
    traded: bool,
}

fn is_enemy_kill(kill: &KillFact) -> bool {
    matches!((kill.attacker, kill.victim), (Some(attacker), Some(victim)) if attacker != victim)
        && (kill.attacker_side == Side::Unknown
            || kill.victim_side == Side::Unknown
            || kill.attacker_side != kill.victim_side)
}

fn trade_flags(
    facts: &MatchFacts,
    kills: &[(usize, &KillFact)],
) -> (BTreeSet<usize>, BTreeSet<usize>) {
    let mut traded_deaths = BTreeSet::new();
    let mut trade_kills = BTreeSet::new();
    let mut by_round = BTreeMap::<usize, Vec<(usize, &KillFact)>>::new();
    for (source_index, kill) in kills {
        if let Some(round_index) = round_index_for_tick(&facts.rounds, kill.tick) {
            by_round
                .entry(round_index)
                .or_default()
                .push((*source_index, *kill));
        }
    }

    let window = CS2_TICKS_PER_SECOND * TRADE_WINDOW_SECONDS;
    for round_kills in by_round.values_mut() {
        round_kills.sort_by_key(|(_, kill)| kill.tick);
        for position in 0..round_kills.len() {
            let (death_index, death) = round_kills[position];
            let (Some(dead_player), Some(killer)) = (death.victim, death.attacker) else {
                continue;
            };
            for (trade_index, trade) in &round_kills[position + 1..] {
                if trade.tick - death.tick > window {
                    break;
                }
                if trade.victim == Some(killer)
                    && trade.attacker != Some(dead_player)
                    && (death.victim_side == Side::Unknown
                        || trade.attacker_side == death.victim_side)
                {
                    traded_deaths.insert(death_index);
                    trade_kills.insert(*trade_index);
                    break;
                }
            }
        }
    }
    (traded_deaths, trade_kills)
}

fn infer_round_sides(facts: &MatchFacts, player: PlayerId) -> BTreeMap<usize, Side> {
    let mut sides = facts.ticks.majority_sides(player);
    if sides.is_empty() {
        sides = event_round_sides(facts, player);
    }

    // A player can complete a round without producing a combat event (for
    // example, an untouched survivor). Tick observations cover that case;
    // event-derived sides still fill the nearest known round if a round has
    // no observation for this player.
    for round_index in 0..facts.rounds.len() {
        if sides.contains_key(&round_index) {
            continue;
        }
        let nearest = sides
            .iter()
            .min_by_key(|(known_round, _)| known_round.abs_diff(round_index))
            .map(|(_, side)| *side);
        if let Some(side) = nearest {
            sides.insert(round_index, side);
        }
    }
    sides
}

fn event_round_sides(facts: &MatchFacts, player: PlayerId) -> BTreeMap<usize, Side> {
    let mut counts = BTreeMap::<(usize, Side), usize>::new();
    let mut observe = |tick: i32, candidate: Option<PlayerId>, side: Side| {
        if candidate != Some(player) || side == Side::Unknown {
            return;
        }
        if let Some(round_index) = round_index_for_tick(&facts.rounds, tick) {
            *counts.entry((round_index, side)).or_default() += 1;
        }
    };
    for kill in &facts.kills {
        observe(kill.tick, kill.attacker, kill.attacker_side);
        observe(kill.tick, kill.victim, kill.victim_side);
    }
    for damage in &facts.damages {
        observe(damage.tick, damage.attacker, damage.attacker_side);
        observe(damage.tick, damage.victim, damage.victim_side);
    }
    for blind in &facts.blinds {
        observe(blind.tick, blind.attacker, blind.attacker_side);
        observe(blind.tick, blind.victim, blind.victim_side);
    }
    for shot in &facts.shots {
        observe(shot.tick, shot.player, shot.side);
    }

    let mut result = BTreeMap::new();
    for ((round_index, side), count) in counts {
        let replace = result
            .get(&round_index)
            .is_none_or(|(_, best_count)| count > *best_count);
        if replace {
            result.insert(round_index, (side, count));
        }
    }
    result
        .into_iter()
        .map(|(round_index, (side, _))| (round_index, side))
        .collect()
}

fn is_utility_weapon(weapon: &str) -> bool {
    let weapon = weapon.to_ascii_lowercase();
    [
        "hegrenade",
        "he grenade",
        "molotov",
        "incgrenade",
        "incendiary",
    ]
    .iter()
    .any(|token| weapon.contains(token))
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10_f64.powi(places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::{BlindFact, DamageFact, PlayerFact, RoundFact, ShotFact};
    use crate::ticks::{TickObservations, TickSample};

    const PLAYER: PlayerId = PlayerId(76_561_198_000_000_001);
    const TEAMMATE: PlayerId = PlayerId(76_561_198_000_000_002);
    const ENEMY_A: PlayerId = PlayerId(76_561_198_000_000_003);
    const ENEMY_B: PlayerId = PlayerId(76_561_198_000_000_004);

    fn kill(
        tick: i32,
        attacker: PlayerId,
        victim: PlayerId,
        attacker_side: Side,
        victim_side: Side,
    ) -> KillFact {
        KillFact {
            tick,
            attacker: Some(attacker),
            victim: Some(victim),
            assister: None,
            attacker_side,
            victim_side,
            headshot: false,
        }
    }

    fn fixture() -> MatchFacts {
        let mut kills = vec![
            kill(
                100,
                PLAYER,
                ENEMY_A,
                Side::CounterTerrorist,
                Side::Terrorist,
            ),
            kill(
                300,
                ENEMY_B,
                TEAMMATE,
                Side::Terrorist,
                Side::CounterTerrorist,
            ),
            kill(
                1000,
                ENEMY_A,
                PLAYER,
                Side::CounterTerrorist,
                Side::Terrorist,
            ),
            kill(
                1100,
                TEAMMATE,
                ENEMY_A,
                Side::Terrorist,
                Side::CounterTerrorist,
            ),
            kill(
                2000,
                PLAYER,
                ENEMY_B,
                Side::Terrorist,
                Side::CounterTerrorist,
            ),
        ];
        kills[0].headshot = true;
        kills[1].assister = Some(PLAYER);
        let rounds = vec![
            RoundFact {
                number: 1,
                start_tick: 0,
                freeze_end_tick: None,
                end_tick: 500,
                official_end_tick: 600,
                winner: Side::CounterTerrorist,
            },
            RoundFact {
                number: 2,
                start_tick: 900,
                freeze_end_tick: None,
                end_tick: 1400,
                official_end_tick: 1500,
                winner: Side::Terrorist,
            },
            RoundFact {
                number: 3,
                start_tick: 1900,
                freeze_end_tick: None,
                end_tick: 2400,
                official_end_tick: 2500,
                winner: Side::CounterTerrorist,
            },
        ];
        let ticks = TickObservations::from_samples(
            [
                TickSample {
                    tick: 50,
                    player: PLAYER,
                    side: Side::CounterTerrorist,
                    health: 100,
                    origin: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    duck_amount: 0.0,
                    velocity: 0.0,
                    shots_fired: 0,
                    weapon: "ak47".to_owned(),
                    spotted_by: vec![],
                },
                TickSample {
                    tick: 950,
                    player: PLAYER,
                    side: Side::Terrorist,
                    health: 100,
                    origin: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    duck_amount: 0.0,
                    velocity: 0.0,
                    shots_fired: 0,
                    weapon: "ak47".to_owned(),
                    spotted_by: vec![],
                },
                TickSample {
                    tick: 1950,
                    player: PLAYER,
                    side: Side::Terrorist,
                    health: 100,
                    origin: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    duck_amount: 0.0,
                    velocity: 0.0,
                    shots_fired: 0,
                    weapon: "ak47".to_owned(),
                    spotted_by: vec![],
                },
            ],
            &rounds,
        );
        MatchFacts {
            map: "de_test".to_owned(),
            players: vec![PlayerFact {
                steam_id: PLAYER,
                name: "Kieren".to_owned(),
            }],
            rounds,
            kills,
            damages: vec![
                DamageFact {
                    tick: 90,
                    attacker: Some(PLAYER),
                    victim: Some(ENEMY_A),
                    attacker_side: Side::CounterTerrorist,
                    victim_side: Side::Terrorist,
                    weapon: "ak47".to_owned(),
                    health_damage: 100,
                },
                DamageFact {
                    tick: 1990,
                    attacker: Some(PLAYER),
                    victim: Some(ENEMY_B),
                    attacker_side: Side::Terrorist,
                    victim_side: Side::CounterTerrorist,
                    weapon: "hegrenade".to_owned(),
                    health_damage: 40,
                },
                DamageFact {
                    tick: 1991,
                    attacker: Some(PLAYER),
                    victim: Some(PLAYER),
                    attacker_side: Side::Terrorist,
                    victim_side: Side::Terrorist,
                    weapon: "hegrenade".to_owned(),
                    health_damage: 10,
                },
            ],
            blinds: vec![
                BlindFact {
                    tick: 990,
                    attacker: Some(PLAYER),
                    victim: Some(ENEMY_A),
                    attacker_side: Side::Terrorist,
                    victim_side: Side::CounterTerrorist,
                    duration_seconds: 2.4,
                },
                BlindFact {
                    tick: 991,
                    attacker: Some(PLAYER),
                    victim: Some(TEAMMATE),
                    attacker_side: Side::Terrorist,
                    victim_side: Side::Terrorist,
                    duration_seconds: 1.0,
                },
            ],
            shots: vec![
                ShotFact {
                    tick: 50,
                    player: Some(PLAYER),
                    side: Side::CounterTerrorist,
                    weapon: "ak47".to_owned(),
                },
                ShotFact {
                    tick: 950,
                    player: Some(PLAYER),
                    side: Side::Terrorist,
                    weapon: "ak47".to_owned(),
                },
                ShotFact {
                    tick: 1950,
                    player: Some(PLAYER),
                    side: Side::Terrorist,
                    weapon: "ak47".to_owned(),
                },
            ],
            bullets: vec![],
            ticks,
            tick_rows: 0,
        }
    }

    #[test]
    fn matches_python_mvp_metrics_fixture() {
        let stats = calculate(&fixture(), PLAYER);
        assert_eq!(stats.kills, 2);
        assert_eq!(stats.deaths, 1);
        assert_eq!(stats.assists, 1);
        assert_eq!(stats.headshot_percent, 50.0);
        assert_eq!(stats.adr, 46.7);
        assert_eq!(stats.kast, 100.0);
        assert_eq!(stats.opening_kills, 2);
        assert_eq!(stats.opening_deaths, 1);
        assert_eq!(stats.traded_deaths, 1);
        assert_eq!(stats.trade_kills, 0);
        assert_eq!(stats.utility_damage, 40);
        assert_eq!(stats.enemies_flashed, 1);
        assert_eq!(stats.friends_flashed, 1);
        assert_eq!(stats.rounds_for, 2);
        assert_eq!(stats.rounds_against, 1);
        assert_eq!(stats.result, "W");
    }

    #[test]
    fn resolves_players_by_id_or_case_insensitive_exact_name() {
        let facts = fixture();
        assert_eq!(
            resolve_player(&facts, &PLAYER.0.to_string()).unwrap(),
            PLAYER
        );
        assert_eq!(resolve_player(&facts, "kIeReN").unwrap(), PLAYER);
        assert!(resolve_player(&facts, "missing").is_err());
    }

    #[test]
    fn round_score_uses_tick_observation_sides_over_event_sides() {
        let mut facts = fixture();
        facts.ticks = TickObservations::from_samples(
            [TickSample {
                tick: 50,
                player: PLAYER,
                side: Side::Terrorist,
                health: 100,
                origin: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                duck_amount: 0.0,
                velocity: 0.0,
                shots_fired: 0,
                weapon: "ak47".to_owned(),
                spotted_by: vec![],
            }],
            &facts.rounds,
        );

        let stats = calculate(&facts, PLAYER);
        assert_eq!(stats.rounds_for, 1);
        assert_eq!(stats.rounds_against, 2);
        assert_eq!(stats.result, "L");
    }
}
