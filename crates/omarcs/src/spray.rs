use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::match_facts::{BulletFact, MatchFacts, PlayerId};

const SPRAY_GAP_TICKS: i32 = 16;
const MIN_SPRAY_SHOTS: usize = 5;
const MAX_SPRAY_SHOTS: usize = 10;
const MAX_TARGET_ANGLE: f64 = 15.0;
const MAX_PROJECTED_OFFSET: f64 = 220.0;
const SETTLED_SPEED_FRACTION: f32 = 0.34;

const WEAPONS: &[(i32, SprayWeapon)] = &[
    (
        7,
        SprayWeapon {
            id: "ak47",
            max_speed: 215.0,
        },
    ),
    (
        13,
        SprayWeapon {
            id: "galilar",
            max_speed: 215.0,
        },
    ),
    (
        16,
        SprayWeapon {
            id: "m4a4",
            max_speed: 225.0,
        },
    ),
    (
        60,
        SprayWeapon {
            id: "m4a1_silencer",
            max_speed: 225.0,
        },
    ),
];

#[derive(Clone, Copy)]
struct SprayWeapon {
    id: &'static str,
    max_speed: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SprayShot {
    pub number: usize,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SprayBurst {
    pub weapon: &'static str,
    pub shots: Vec<SprayShot>,
}

struct TargetCandidate {
    viewer_velocity: f32,
    target_origin: [f32; 3],
    target_duck: f32,
    visible: bool,
}

pub fn calculate(facts: &MatchFacts, player: PlayerId) -> Vec<SprayBurst> {
    if facts.ticks.is_empty() {
        return Vec::new();
    }

    let grouped = group_sprays(
        facts
            .bullets
            .iter()
            .filter(|bullet| bullet.player == Some(player)),
    );
    let shot_ticks = grouped
        .iter()
        .flat_map(|(_, burst)| burst.iter().map(|shot| shot.tick))
        .collect::<HashSet<_>>();
    let candidates = target_candidates(facts, player, &shot_ticks);
    let mut sprays = Vec::new();

    for (weapon, burst) in grouped {
        let mut points = Vec::new();
        for (number, shot) in burst.into_iter().enumerate() {
            let number = number + 1;
            let Some(target) =
                closest_target(shot, candidates.get(&shot.tick).map_or(&[], Vec::as_slice))
            else {
                continue;
            };
            if points.is_empty()
                && (!target.viewer_velocity.is_finite()
                    || target.viewer_velocity >= weapon.max_speed * SETTLED_SPEED_FRACTION)
            {
                break;
            }
            let Some((x, y)) = projected_offset(shot, target) else {
                continue;
            };
            points.push(SprayShot {
                number,
                x: round_to(x, 2),
                y: round_to(y, 2),
            });
        }
        if points.len() >= MIN_SPRAY_SHOTS {
            sprays.push(SprayBurst {
                weapon: weapon.id,
                shots: points,
            });
        }
    }
    sprays
}

fn group_sprays<'a>(
    bullets: impl IntoIterator<Item = &'a BulletFact>,
) -> Vec<(SprayWeapon, Vec<&'a BulletFact>)> {
    let mut bullets = bullets.into_iter().collect::<Vec<_>>();
    bullets.sort_by_key(|bullet| bullet.tick);

    let mut grouped = Vec::new();
    let mut current: Vec<&BulletFact> = Vec::new();
    let mut current_weapon: Option<SprayWeapon> = None;
    let mut previous_tick: Option<i32> = None;

    for bullet in bullets {
        let Some(item_definition) = bullet.item_definition else {
            finish_group(&mut grouped, &mut current, &mut current_weapon);
            previous_tick = None;
            continue;
        };
        let Some(weapon) = weapon(item_definition) else {
            finish_group(&mut grouped, &mut current, &mut current_weapon);
            previous_tick = None;
            continue;
        };
        let continues = current_weapon.is_some_and(|current| current.id == weapon.id)
            && previous_tick.is_some_and(|previous| bullet.tick - previous <= SPRAY_GAP_TICKS);
        if !continues {
            finish_group(&mut grouped, &mut current, &mut current_weapon);
            current_weapon = Some(weapon);
        }
        current.push(bullet);
        previous_tick = Some(bullet.tick);
    }
    finish_group(&mut grouped, &mut current, &mut current_weapon);
    grouped
}

fn finish_group<'a>(
    grouped: &mut Vec<(SprayWeapon, Vec<&'a BulletFact>)>,
    current: &mut Vec<&'a BulletFact>,
    current_weapon: &mut Option<SprayWeapon>,
) {
    if let Some(weapon) = current_weapon.take()
        && current.len() >= MIN_SPRAY_SHOTS
    {
        current.truncate(MAX_SPRAY_SHOTS);
        grouped.push((weapon, std::mem::take(current)));
    } else {
        current.clear();
    }
}

fn target_candidates(
    facts: &MatchFacts,
    player: PlayerId,
    shot_ticks: &HashSet<i32>,
) -> HashMap<i32, Vec<TargetCandidate>> {
    let ticks = &facts.ticks;
    let mut viewers = HashMap::<i32, usize>::new();
    let mut enemies = HashMap::<i32, Vec<usize>>::new();
    for index in 0..ticks.len() {
        let tick = ticks.tick(index);
        if !shot_ticks.contains(&tick) {
            continue;
        }
        if ticks.player(index) == player {
            viewers.insert(tick, index);
        } else {
            enemies.entry(tick).or_default().push(index);
        }
    }

    let mut candidates = HashMap::<i32, Vec<TargetCandidate>>::new();
    for (tick, viewer_index) in viewers {
        if ticks.health(viewer_index) <= 0 {
            continue;
        }
        for &enemy_index in enemies.get(&tick).map(Vec::as_slice).unwrap_or(&[]) {
            if ticks.health(enemy_index) <= 0 || ticks.side(enemy_index) == ticks.side(viewer_index)
            {
                continue;
            }
            candidates.entry(tick).or_default().push(TargetCandidate {
                viewer_velocity: ticks.velocity(viewer_index),
                target_origin: ticks.origin(enemy_index),
                target_duck: ticks.duck_amount(enemy_index),
                visible: ticks.spotted_by(enemy_index).contains(&player),
            });
        }
    }
    candidates
}

fn closest_target<'a>(
    shot: &BulletFact,
    candidates: &'a [TargetCandidate],
) -> Option<&'a TargetCandidate> {
    let origin = shot_origin(shot)?;
    let direction = shot_direction(shot)?;
    let mut best: Option<(f64, &TargetCandidate)> = None;
    for candidate in candidates {
        if !candidate.visible {
            continue;
        }
        let Some(target_direction) = normalize(sub(target_head(candidate), origin)) else {
            continue;
        };
        let angle = angular_distance(direction, target_direction);
        if best.is_none_or(|(best_angle, _)| angle < best_angle) {
            best = Some((angle, candidate));
        }
    }
    best.and_then(|(angle, target)| (angle <= MAX_TARGET_ANGLE).then_some(target))
}

fn projected_offset(shot: &BulletFact, target: &TargetCandidate) -> Option<(f64, f64)> {
    let origin = shot_origin(shot)?;
    let direction = shot_direction(shot)?;
    let head = target_head(target);
    let target_vector = sub(head, origin);
    let normal = normalize(target_vector)?;
    let denominator = dot(direction, normal);
    if denominator <= 0.05 {
        return None;
    }
    let impact = add_scaled(origin, direction, length(target_vector) / denominator);
    let offset = sub(impact, head);
    let right = normalize([-normal[1], normal[0], 0.0])?;
    let up = normalize(cross(normal, right))?;
    let horizontal = dot(offset, right);
    let vertical = dot(offset, up);
    if !horizontal.is_finite() || !vertical.is_finite() {
        return None;
    }
    if horizontal.abs() > MAX_PROJECTED_OFFSET || vertical.abs() > MAX_PROJECTED_OFFSET {
        return None;
    }
    Some((horizontal, vertical))
}

fn shot_origin(shot: &BulletFact) -> Option<[f64; 3]> {
    let origin = [
        f64::from(shot.origin[0]),
        f64::from(shot.origin[1]),
        f64::from(shot.origin[2]),
    ];
    origin
        .iter()
        .all(|value| value.is_finite())
        .then_some(origin)
}

fn shot_direction(shot: &BulletFact) -> Option<[f64; 3]> {
    let pitch = f64::from(shot.angles[0]);
    let yaw = f64::from(shot.angles[1]);
    if !pitch.is_finite() || !yaw.is_finite() {
        return None;
    }
    Some(direction_from_angles(pitch, yaw))
}

fn target_head(target: &TargetCandidate) -> [f64; 3] {
    [
        f64::from(target.target_origin[0]),
        f64::from(target.target_origin[1]),
        f64::from(target.target_origin[2]) + 64.0 - 18.0 * f64::from(target.target_duck),
    ]
}

fn direction_from_angles(pitch_degrees: f64, yaw_degrees: f64) -> [f64; 3] {
    let pitch = pitch_degrees.to_radians();
    let yaw = yaw_degrees.to_radians();
    [
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    ]
}

fn angular_distance(first: [f64; 3], second: [f64; 3]) -> f64 {
    dot(first, second).clamp(-1.0, 1.0).acos().to_degrees()
}

fn weapon(item_definition: i32) -> Option<SprayWeapon> {
    WEAPONS
        .iter()
        .find_map(|(id, weapon)| (*id == item_definition).then_some(*weapon))
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn sub(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn add_scaled(origin: [f64; 3], direction: [f64; 3], scale: f64) -> [f64; 3] {
    [
        origin[0] + direction[0] * scale,
        origin[1] + direction[1] * scale,
        origin[2] + direction[2] * scale,
    ]
}

fn length(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = length(vector);
    (length > 1e-9).then(|| [vector[0] / length, vector[1] / length, vector[2] / length])
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10_f64.powi(places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::{PlayerFact, RoundFact, Side};
    use crate::ticks::{TickObservations, TickSample};

    const PLAYER: PlayerId = PlayerId(76_561_198_000_000_001);
    const ENEMY: PlayerId = PlayerId(76_561_198_000_000_002);

    fn round() -> RoundFact {
        RoundFact {
            number: 1,
            start_tick: 1,
            freeze_end_tick: None,
            end_tick: 200,
            official_end_tick: 200,
            winner: Side::CounterTerrorist,
        }
    }

    fn sample(
        tick: i32,
        player: PlayerId,
        side: Side,
        origin: [f32; 3],
        yaw: f32,
        spotted_by: Vec<PlayerId>,
    ) -> TickSample {
        TickSample {
            tick,
            player,
            side,
            health: 100,
            origin,
            pitch: 0.0,
            yaw,
            duck_amount: 0.0,
            velocity: 0.0,
            shots_fired: 0,
            weapon: "ak47".to_owned(),
            spotted_by,
        }
    }

    fn bullet(tick: i32, yaw: f32) -> BulletFact {
        BulletFact {
            tick,
            player: Some(PLAYER),
            item_definition: Some(7),
            origin: [0.0, 0.0, 64.0],
            angles: [0.0, yaw, 0.0],
        }
    }

    fn fixture() -> MatchFacts {
        let rounds = vec![round()];
        let mut ticks = Vec::new();
        let mut bullets = Vec::new();
        for (number, tick) in [100, 106, 112, 118, 124, 130].into_iter().enumerate() {
            ticks.push(sample(
                tick,
                PLAYER,
                Side::CounterTerrorist,
                [0.0, 0.0, 0.0],
                0.0,
                vec![],
            ));
            ticks.push(sample(
                tick,
                ENEMY,
                Side::Terrorist,
                [100.0, 0.0, 0.0],
                180.0,
                vec![PLAYER],
            ));
            bullets.push(bullet(tick, number as f32));
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
            damages: vec![],
            blinds: vec![],
            shots: vec![],
            bullets,
            ticks: TickObservations::from_samples(ticks, &[round()]),
            tick_rows: 0,
        }
    }

    #[test]
    fn projects_shot_onto_enemy_head_plane() {
        let target = TargetCandidate {
            viewer_velocity: 0.0,
            target_origin: [100.0, 0.0, 0.0],
            target_duck: 0.0,
            visible: true,
        };
        let centred = bullet(100, 0.0);
        let right = bullet(100, 5.0);

        assert_eq!(projected_offset(&centred, &target), Some((0.0, 0.0)));
        let (horizontal, vertical) = projected_offset(&right, &target).expect("offset");
        assert_eq!((horizontal * 10.0).round() / 10.0, 8.7);
        assert_eq!((vertical * 10.0).round() / 10.0, 0.0);
    }

    #[test]
    fn matches_python_target_relative_spray_fixture() {
        let sprays = calculate(&fixture(), PLAYER);
        assert_eq!(sprays.len(), 1);
        assert_eq!(sprays[0].weapon, "ak47");
        assert_eq!(
            sprays[0]
                .shots
                .iter()
                .map(|shot| shot.number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );
        assert_eq!(sprays[0].shots[0].x, 0.0);
        assert!(sprays[0].shots[5].x > 8.0);
    }

    #[test]
    fn returns_no_sprays_without_tick_observations() {
        let mut facts = fixture();
        facts.ticks = TickObservations::from_samples([], &facts.rounds);
        assert_eq!(calculate(&facts, PLAYER), Vec::<SprayBurst>::new());
    }
}
