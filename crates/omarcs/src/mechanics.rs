use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::geometry::{self, Mesh, VisibilityRow};
use crate::match_facts::{
    DamageFact, MatchFacts, PlayerId, RoundFact, ShotFact, round_index_for_tick,
};
use crate::ticks::TickObservations;

const CS2_TICKS_PER_SECOND: i32 = 64;
const ENGAGEMENT_TICKS: i32 = CS2_TICKS_PER_SECOND;

const RIFLE_MAX_SPEED: &[(&str, f32)] = &[
    ("ak47", 215.0),
    ("aug", 220.0),
    ("famas", 220.0),
    ("galilar", 215.0),
    ("m4a1", 225.0),
    ("m4a1_silencer", 225.0),
    ("sg556", 210.0),
];

const NON_GUN_TOKENS: &[&str] = &[
    "bayonet",
    "c4",
    "decoy",
    "flashbang",
    "hegrenade",
    "incgrenade",
    "inferno",
    "knife",
    "molotov",
    "smokegrenade",
    "taser",
];

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MechanicsMetrics {
    pub crosshair_placement: Option<f64>,
    pub horizontal_adjustment: Option<f64>,
    pub vertical_adjustment: Option<f64>,
    pub reaction_time_ms: Option<f64>,
    pub time_to_damage_ms: Option<f64>,
    pub spotted_accuracy: Option<f64>,
    pub counter_strafe_percent: Option<f64>,
    pub mechanics_engagements: usize,
    pub mechanics_exposures: usize,
    pub spotted_shots: usize,
    pub counter_strafe_shots: usize,
    pub mechanics_quality: &'static str,
}

pub fn empty() -> MechanicsMetrics {
    MechanicsMetrics {
        crosshair_placement: None,
        horizontal_adjustment: None,
        vertical_adjustment: None,
        reaction_time_ms: None,
        time_to_damage_ms: None,
        spotted_accuracy: None,
        counter_strafe_percent: None,
        mechanics_engagements: 0,
        mechanics_exposures: 0,
        spotted_shots: 0,
        counter_strafe_shots: 0,
        mechanics_quality: "radar-beta",
    }
}

pub fn calculate(facts: &MatchFacts, player: PlayerId, mesh: Option<&Mesh>) -> MechanicsMetrics {
    let mut metrics = empty();
    if facts.ticks.is_empty() {
        return metrics;
    }

    let shots = facts
        .shots
        .iter()
        .filter(|shot| shot.player == Some(player) && is_gun(&shot.weapon))
        .collect::<Vec<_>>();
    let damages = facts
        .damages
        .iter()
        .filter(|damage| {
            damage.attacker == Some(player)
                && damage.victim != Some(player)
                && is_gun(&damage.weapon)
        })
        .collect::<Vec<_>>();

    let shot_ticks = shots.iter().map(|shot| shot.tick).collect::<BTreeSet<_>>();
    let (exposures, visible_ticks) = match mesh {
        Some(mesh) => {
            metrics.mechanics_quality = "geometry";
            geometry_exposures(
                &facts.ticks,
                &facts.rounds,
                player,
                &damages,
                &shot_ticks,
                mesh,
            )
        }
        None => {
            metrics.mechanics_quality = "radar-beta";
            radar_exposures(&facts.ticks, player)
        }
    };
    metrics.mechanics_exposures = exposures.len();

    let spotted_shots = shots
        .iter()
        .copied()
        .filter(|shot| visible_ticks.contains(&shot.tick))
        .collect::<Vec<_>>();
    metrics.spotted_shots = spotted_shots.len();
    let hit_ticks = damages
        .iter()
        .map(|damage| damage.tick)
        .filter(|tick| visible_ticks.contains(tick))
        .collect::<BTreeSet<_>>();
    if !spotted_shots.is_empty() {
        metrics.spotted_accuracy = Some(round_to(
            100.0 * hit_ticks.len() as f64 / spotted_shots.len() as f64,
            1,
        ));
    }

    let tick_index = player_tick_index(&facts.ticks);
    let mut counter_shots = 0;
    let mut proper_counter_shots = 0;
    for shot in &spotted_shots {
        let Some(index) = tick_index.get(&(player, shot.tick)).copied() else {
            continue;
        };
        let Some(max_speed) = rifle_max_speed(&shot.weapon) else {
            continue;
        };
        let duck = facts.ticks.duck_amount(index);
        let velocity = facts.ticks.velocity(index);
        if duck >= 0.1 || !velocity.is_finite() {
            continue;
        }
        counter_shots += 1;
        if velocity < max_speed * 0.34 {
            proper_counter_shots += 1;
        }
    }
    metrics.counter_strafe_shots = counter_shots;
    if counter_shots > 0 {
        metrics.counter_strafe_percent = Some(round_to(
            100.0 * proper_counter_shots as f64 / counter_shots as f64,
            1,
        ));
    }

    let mut exposures_by_target = BTreeMap::<(PlayerId, usize), Vec<&Exposure>>::new();
    for exposure in &exposures {
        exposures_by_target
            .entry((exposure.enemy, exposure.round_index))
            .or_default()
            .push(exposure);
    }
    let mut shots_by_round = BTreeMap::<usize, Vec<&ShotFact>>::new();
    for shot in &spotted_shots {
        if let Some(round_index) = round_index_for_tick(&facts.rounds, shot.tick) {
            shots_by_round.entry(round_index).or_default().push(*shot);
        }
    }

    let mut corrections = Vec::new();
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    let mut reaction_times = Vec::new();
    let mut damage_times = Vec::new();
    let mut used_exposures = BTreeSet::new();

    let mut ordered_damages = damages;
    ordered_damages.sort_by_key(|damage| damage.tick);
    for damage in ordered_damages {
        let Some(victim) = damage.victim else {
            continue;
        };
        let Some(round_index) = round_index_for_tick(&facts.rounds, damage.tick) else {
            continue;
        };
        let Some(exposure) =
            exposures_by_target
                .get(&(victim, round_index))
                .and_then(|candidates| {
                    candidates
                        .iter()
                        .copied()
                        .filter(|exposure| {
                            let elapsed = damage.tick - exposure.tick;
                            (0..ENGAGEMENT_TICKS).contains(&elapsed)
                        })
                        .max_by_key(|exposure| exposure.tick)
                })
        else {
            continue;
        };
        let exposure_key = (victim, round_index, exposure.tick);
        if !used_exposures.insert(exposure_key) {
            continue;
        }

        damage_times.push(
            1000.0 * f64::from(damage.tick - exposure.tick) / f64::from(CS2_TICKS_PER_SECOND),
        );
        if let Some(index) = tick_index.get(&(player, damage.tick)).copied() {
            let yaw_change = angle_delta(exposure.viewer_yaw, facts.ticks.yaw(index)).abs();
            let pitch_change = (facts.ticks.pitch(index) - exposure.viewer_pitch).abs();
            horizontal.push(f64::from(yaw_change));
            vertical.push(f64::from(pitch_change));
            corrections.push(f64::from(yaw_change.hypot(pitch_change)));
        }

        if let Some(first_shot) = shots_by_round.get(&round_index).and_then(|round_shots| {
            round_shots
                .iter()
                .copied()
                .find(|shot| shot.tick >= exposure.tick && shot.tick <= damage.tick)
        }) {
            reaction_times.push(
                1000.0 * f64::from(first_shot.tick - exposure.tick)
                    / f64::from(CS2_TICKS_PER_SECOND),
            );
        }
    }

    metrics.crosshair_placement = median(&mut corrections, 2);
    metrics.horizontal_adjustment = median(&mut horizontal, 2);
    metrics.vertical_adjustment = median(&mut vertical, 2);
    metrics.reaction_time_ms = median(&mut reaction_times, 0);
    metrics.time_to_damage_ms = median(&mut damage_times, 0);
    metrics.mechanics_engagements = used_exposures.len();
    metrics
}

struct Exposure {
    tick: i32,
    round_index: usize,
    enemy: PlayerId,
    viewer_pitch: f32,
    viewer_yaw: f32,
}

fn radar_exposures(ticks: &TickObservations, player: PlayerId) -> (Vec<Exposure>, BTreeSet<i32>) {
    let mut viewer_at = HashMap::<(usize, i32), usize>::new();
    let mut paired = Vec::new();
    for index in 0..ticks.len() {
        if ticks.player(index) == player {
            viewer_at.insert((ticks.round_index(index), ticks.tick(index)), index);
        }
    }

    let mut visible_ticks = BTreeSet::new();
    for index in 0..ticks.len() {
        if ticks.player(index) == player {
            continue;
        }
        let tick = ticks.tick(index);
        let round_index = ticks.round_index(index);
        let Some(viewer_index) = viewer_at.get(&(round_index, tick)).copied() else {
            continue;
        };
        if ticks.health(index) <= 0 || ticks.health(viewer_index) <= 0 {
            continue;
        }
        if ticks.side(index) == ticks.side(viewer_index) {
            continue;
        }
        let seen = ticks.spotted_by(index).contains(&player);
        if seen {
            visible_ticks.insert(tick);
        }
        paired.push((
            ticks.player(index),
            round_index,
            tick,
            seen,
            ticks.pitch(viewer_index),
            ticks.yaw(viewer_index),
        ));
    }

    paired.sort_by_key(|(enemy, round_index, tick, _, _, _)| (*enemy, *round_index, *tick));
    let mut exposures = Vec::new();
    let mut previous: Option<(PlayerId, usize, i32, bool)> = None;
    for (enemy, round_index, tick, seen, pitch, yaw) in paired {
        let start = seen
            && previous.is_none_or(|(prev_enemy, prev_round, prev_tick, prev_seen)| {
                prev_enemy != enemy
                    || prev_round != round_index
                    || !prev_seen
                    || tick - prev_tick > 1
            });
        if start {
            exposures.push(Exposure {
                tick,
                round_index,
                enemy,
                viewer_pitch: pitch,
                viewer_yaw: yaw,
            });
        }
        previous = Some((enemy, round_index, tick, seen));
    }
    (exposures, visible_ticks)
}

fn geometry_exposures(
    ticks: &TickObservations,
    rounds: &[RoundFact],
    player: PlayerId,
    damages: &[&DamageFact],
    shot_ticks: &BTreeSet<i32>,
    mesh: &Mesh,
) -> (Vec<Exposure>, BTreeSet<i32>) {
    let pairs = geometry_pairs(ticks, player);
    if pairs.is_empty() {
        return (Vec::new(), BTreeSet::new());
    }

    let shot_rows = pairs
        .iter()
        .filter(|pair| shot_ticks.contains(&pair.tick))
        .map(|pair| pair.row)
        .collect::<Vec<_>>();
    let shot_visibility = geometry::visible_rows(&shot_rows, mesh);
    let visible_ticks = pairs
        .iter()
        .filter(|pair| shot_ticks.contains(&pair.tick))
        .zip(shot_visibility)
        .filter_map(|(pair, visible)| visible.then_some(pair.tick))
        .collect::<BTreeSet<_>>();

    let mut exposures = Vec::new();
    let mut seen_keys = BTreeSet::new();
    for damage in damages {
        let Some(enemy) = damage.victim else {
            continue;
        };
        let Some(round_index) = round_index_for_tick(rounds, damage.tick) else {
            continue;
        };
        let window = pairs
            .iter()
            .filter(|pair| {
                pair.enemy == enemy
                    && pair.round_index == round_index
                    && pair.tick >= damage.tick - ENGAGEMENT_TICKS
                    && pair.tick <= damage.tick
            })
            .collect::<Vec<_>>();
        if window.is_empty() {
            continue;
        }
        let rows = window.iter().map(|pair| pair.row).collect::<Vec<_>>();
        let visibility = geometry::visible_rows(&rows, mesh);
        let Some(last) = visibility
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, visible)| visible.then_some(index))
        else {
            continue;
        };
        let mut onset = last;
        while onset > 0 {
            let current_tick = window[onset].tick;
            let previous_tick = window[onset - 1].tick;
            if !visibility[onset - 1] || current_tick - previous_tick > 1 {
                break;
            }
            onset -= 1;
        }
        let pair = window[onset];
        if !seen_keys.insert((pair.enemy, pair.round_index, pair.tick)) {
            continue;
        }
        exposures.push(Exposure {
            tick: pair.tick,
            round_index: pair.round_index,
            enemy: pair.enemy,
            viewer_pitch: pair.viewer_pitch,
            viewer_yaw: pair.viewer_yaw,
        });
    }
    (exposures, visible_ticks)
}

struct GeometryPair {
    tick: i32,
    round_index: usize,
    enemy: PlayerId,
    viewer_pitch: f32,
    viewer_yaw: f32,
    row: VisibilityRow,
}

fn geometry_pairs(ticks: &TickObservations, player: PlayerId) -> Vec<GeometryPair> {
    let mut viewer_at = HashMap::<(usize, i32), usize>::new();
    for index in 0..ticks.len() {
        if ticks.player(index) == player {
            viewer_at.insert((ticks.round_index(index), ticks.tick(index)), index);
        }
    }

    let mut pairs = Vec::new();
    for index in 0..ticks.len() {
        if ticks.player(index) == player {
            continue;
        }
        let tick = ticks.tick(index);
        let round_index = ticks.round_index(index);
        let Some(viewer_index) = viewer_at.get(&(round_index, tick)).copied() else {
            continue;
        };
        if ticks.health(index) <= 0 || ticks.health(viewer_index) <= 0 {
            continue;
        }
        if ticks.side(index) == ticks.side(viewer_index) {
            continue;
        }
        pairs.push(GeometryPair {
            tick,
            round_index,
            enemy: ticks.player(index),
            viewer_pitch: ticks.pitch(viewer_index),
            viewer_yaw: ticks.yaw(viewer_index),
            row: VisibilityRow {
                viewer_origin: ticks.origin(viewer_index),
                viewer_duck: ticks.duck_amount(viewer_index),
                viewer_pitch: ticks.pitch(viewer_index),
                viewer_yaw: ticks.yaw(viewer_index),
                target_origin: ticks.origin(index),
                target_duck: ticks.duck_amount(index),
            },
        });
    }
    pairs
}

fn player_tick_index(ticks: &TickObservations) -> HashMap<(PlayerId, i32), usize> {
    (0..ticks.len())
        .map(|index| ((ticks.player(index), ticks.tick(index)), index))
        .collect()
}

fn is_gun(weapon: &str) -> bool {
    let weapon = normalized_weapon(weapon);
    !weapon.is_empty() && !NON_GUN_TOKENS.iter().any(|token| weapon.contains(token))
}

fn normalized_weapon(weapon: &str) -> String {
    weapon.to_ascii_lowercase().replacen("weapon_", "", 1)
}

fn rifle_max_speed(weapon: &str) -> Option<f32> {
    let weapon = normalized_weapon(weapon);
    RIFLE_MAX_SPEED
        .iter()
        .find_map(|(name, speed)| (*name == weapon).then_some(*speed))
}

fn angle_delta(first: f32, second: f32) -> f32 {
    (second - first + 180.0).rem_euclid(360.0) - 180.0
}

fn median(values: &mut [f64], digits: i32) -> Option<f64> {
    let finite = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return None;
    }
    let mut finite = finite;
    finite.sort_by(|left, right| left.total_cmp(right));
    let mid = finite.len() / 2;
    let value = if finite.len() % 2 == 1 {
        finite[mid]
    } else {
        (finite[mid - 1] + finite[mid]) / 2.0
    };
    Some(round_to(value, digits))
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10_f64.powi(places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::{DamageFact, PlayerFact, RoundFact, Side};
    use crate::ticks::TickSample;

    const PLAYER: PlayerId = PlayerId(76_561_198_000_000_001);
    const ENEMY: PlayerId = PlayerId(76_561_198_000_000_002);

    fn round() -> RoundFact {
        RoundFact {
            number: 1,
            start_tick: 1,
            freeze_end_tick: None,
            end_tick: 30,
            official_end_tick: 30,
            winner: Side::CounterTerrorist,
        }
    }

    fn sample(
        tick: i32,
        player: PlayerId,
        side: Side,
        yaw: f32,
        velocity: f32,
        spotted_by: Vec<PlayerId>,
    ) -> TickSample {
        TickSample {
            tick,
            player,
            side,
            health: 100,
            origin: [0.0; 3],
            pitch: 0.0,
            yaw,
            duck_amount: 0.0,
            velocity,
            shots_fired: 0,
            weapon: "ak47".to_owned(),
            spotted_by,
        }
    }

    fn fixture() -> MatchFacts {
        let rounds = vec![round()];
        let mut ticks = Vec::new();
        for tick in 1..=30 {
            let yaw = if tick == 20 { 5.0 } else { 0.0 };
            let velocity = if tick == 16 { 20.0 } else { 0.0 };
            ticks.push(sample(
                tick,
                PLAYER,
                Side::CounterTerrorist,
                yaw,
                velocity,
                vec![],
            ));
            ticks.push(sample(
                tick,
                ENEMY,
                Side::Terrorist,
                180.0,
                0.0,
                if tick >= 10 { vec![PLAYER] } else { vec![] },
            ));
        }
        MatchFacts {
            map: "de_test".to_owned(),
            players: vec![
                PlayerFact {
                    steam_id: PLAYER,
                    name: "Kieren".to_owned(),
                },
                PlayerFact {
                    steam_id: ENEMY,
                    name: "Enemy".to_owned(),
                },
            ],
            rounds,
            kills: vec![],
            damages: vec![DamageFact {
                tick: 20,
                attacker: Some(PLAYER),
                victim: Some(ENEMY),
                attacker_side: Side::CounterTerrorist,
                victim_side: Side::Terrorist,
                weapon: "ak47".to_owned(),
                health_damage: 100,
            }],
            blinds: vec![],
            shots: vec![ShotFact {
                tick: 16,
                player: Some(PLAYER),
                side: Side::CounterTerrorist,
                weapon: "ak47".to_owned(),
            }],
            bullets: vec![],
            ticks: TickObservations::from_samples(ticks, &[round()]),
            tick_rows: 0,
        }
    }

    #[test]
    fn matches_python_engagement_mechanics_fixture() {
        let stats = calculate(&fixture(), PLAYER, None);
        assert_eq!(stats.mechanics_exposures, 1);
        assert_eq!(stats.mechanics_engagements, 1);
        assert_eq!(stats.crosshair_placement, Some(5.0));
        assert_eq!(stats.horizontal_adjustment, Some(5.0));
        assert_eq!(stats.vertical_adjustment, Some(0.0));
        assert_eq!(stats.reaction_time_ms, Some(94.0));
        assert_eq!(stats.time_to_damage_ms, Some(156.0));
        assert_eq!(stats.spotted_accuracy, Some(100.0));
        assert_eq!(stats.counter_strafe_percent, Some(100.0));
        assert_eq!(stats.mechanics_quality, "radar-beta");
    }

    #[test]
    fn returns_empty_metrics_without_tick_observations() {
        let mut facts = fixture();
        facts.ticks = TickObservations::from_samples([], &facts.rounds);
        let stats = calculate(&facts, PLAYER, None);
        assert_eq!(stats, empty());
    }

    #[test]
    fn map_geometry_blocks_radar_visibility() {
        let mut facts = fixture();
        let mut ticks = Vec::new();
        for tick in 1..=30 {
            let yaw = if tick == 20 { 5.0 } else { 0.0 };
            let velocity = if tick == 16 { 20.0 } else { 0.0 };
            ticks.push(TickSample {
                tick,
                player: PLAYER,
                side: Side::CounterTerrorist,
                health: 100,
                origin: [0.0, 0.0, 0.0],
                pitch: 0.0,
                yaw,
                duck_amount: 0.0,
                velocity,
                shots_fired: 0,
                weapon: "ak47".to_owned(),
                spotted_by: vec![],
            });
            ticks.push(TickSample {
                tick,
                player: ENEMY,
                side: Side::Terrorist,
                health: 100,
                origin: [10.0, 0.0, 0.0],
                pitch: 0.0,
                yaw: 180.0,
                duck_amount: 0.0,
                velocity: 0.0,
                shots_fired: 0,
                weapon: "ak47".to_owned(),
                spotted_by: if tick >= 10 { vec![PLAYER] } else { vec![] },
            });
        }
        facts.ticks = TickObservations::from_samples(ticks, &facts.rounds);
        let wall = crate::geometry::Mesh::axis_aligned_box([1.0, 4.0, 100.0], [5.0, 0.0, 64.0]);

        let radar = calculate(&facts, PLAYER, None);
        assert_eq!(radar.mechanics_engagements, 1);
        assert_eq!(radar.mechanics_quality, "radar-beta");

        let blocked = calculate(&facts, PLAYER, Some(&wall));
        assert_eq!(blocked.mechanics_quality, "geometry");
        assert_eq!(blocked.mechanics_exposures, 0);
        assert_eq!(blocked.mechanics_engagements, 0);
        assert_eq!(blocked.spotted_shots, 0);
    }
}
