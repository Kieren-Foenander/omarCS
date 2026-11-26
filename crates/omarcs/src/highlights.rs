//! Event-based highlight candidates for the analysed player. Never launches a game.
use crate::match_facts::{KillFact, MatchFacts, PlayerId, Side};
use serde::Serialize;
use std::collections::BTreeSet;

pub const TICK_RATE: i32 = 64;
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub round: usize,
    pub start_tick: i32,
    pub end_tick: i32,
    pub tick_rate: i32,
    pub labels: Vec<String>,
    pub kill_ticks: Vec<i32>,
    pub kills: usize,
    pub score: u32,
}

fn enemy_kill(k: &KillFact, player: PlayerId) -> bool {
    k.attacker == Some(player)
        && k.victim.is_some()
        && k.victim != Some(player)
        && k.attacker_side != Side::Unknown
        && k.victim_side != Side::Unknown
        && k.attacker_side != k.victim_side
}
fn angular_distance(pitch: f32, yaw: f32, other_pitch: f32, other_yaw: f32) -> f32 {
    let y = (yaw - other_yaw + 180.0).rem_euclid(360.0) - 180.0;
    (y * y + (pitch - other_pitch).powi(2)).sqrt()
}
fn is_awp(weapon: &str) -> bool {
    weapon.eq_ignore_ascii_case("awp") || weapon.eq_ignore_ascii_case("weapon_awp")
}
fn flick(facts: &MatchFacts, player: PlayerId, kill: &KillFact, rows: &[usize]) -> bool {
    if !is_awp(&kill.weapon) {
        return false;
    }
    let Some(shot) = facts.shots.iter().rev().find(|s| {
        s.player == Some(player)
            && is_awp(&s.weapon)
            && s.tick <= kill.tick
            && s.tick >= kill.tick - 8
    }) else {
        return false;
    };
    let ticks = &facts.ticks;
    let end = rows.partition_point(|i| ticks.tick(*i) <= shot.tick);
    let Some(&last) = rows.get(end.wrapping_sub(1)) else {
        return false;
    };
    if shot.tick - ticks.tick(last) > 2 {
        return false;
    }
    rows[..end]
        .iter()
        .rev()
        .take_while(|i| shot.tick - ticks.tick(**i) <= 12)
        .filter(|i| ticks.health(**i) > 0 && is_awp(ticks.weapon(**i)))
        .any(|i| {
            angular_distance(
                ticks.pitch(*i),
                ticks.yaw(*i),
                ticks.pitch(last),
                ticks.yaw(last),
            ) >= 8.0
        })
}

pub fn detect(facts: &MatchFacts, player: PlayerId) -> Vec<Highlight> {
    let mut result: Vec<Highlight> = Vec::new();
    let mut rows: Vec<_> = (0..facts.ticks.len())
        .filter(|i| facts.ticks.player(*i) == player)
        .collect();
    rows.sort_by_key(|i| facts.ticks.tick(*i));
    for round in &facts.rounds {
        let mut kills: Vec<_> = facts
            .kills
            .iter()
            .filter(|k| {
                enemy_kill(k, player)
                    && k.tick >= round.freeze_end_tick.unwrap_or(round.start_tick)
                    && k.tick <= round.end_tick
            })
            .collect();
        kills.sort_by_key(|k| k.tick);
        // A victim can contribute once per round (respawn modes cannot manufacture an ace).
        let mut victims = BTreeSet::new();
        kills.retain(|k| victims.insert(k.victim));
        let mut candidates: Vec<(Vec<&KillFact>, String, u32)> = Vec::new();
        if kills.len() >= 3 {
            let label = if kills.len() == 5 {
                "Ace".to_owned()
            } else {
                format!("{}K", kills.len())
            };
            candidates.push((
                kills.clone(),
                label,
                if kills.len() == 5 {
                    100
                } else {
                    30 + kills.len() as u32 * 10
                },
            ));
        }
        for pair in kills.windows(2) {
            if pair[1].tick - pair[0].tick <= 5 * TICK_RATE {
                candidates.push((pair.to_vec(), "Quick double kill".to_owned(), 40));
            }
        }
        let awp_kills: Vec<_> = kills
            .iter()
            .copied()
            .filter(|k| is_awp(&k.weapon))
            .collect();
        for pair in awp_kills.windows(2) {
            if pair[1].tick - pair[0].tick <= 15 * TICK_RATE {
                candidates.push((pair.to_vec(), "AWP double kill".to_owned(), 55));
            }
        }
        for kill in &kills {
            if flick(facts, player, kill, &rows) {
                candidates.push((vec![*kill], "AWP flick candidate".to_owned(), 60));
            }
        }
        let death = facts
            .kills
            .iter()
            .filter(|k| {
                k.victim == Some(player) && k.tick >= round.start_tick && k.tick <= round.end_tick
            })
            .map(|k| k.tick)
            .min();
        let mut intervals: Vec<Highlight> = candidates
            .into_iter()
            .map(|(ks, label, score)| {
                let start = (ks[0].tick - 3 * TICK_RATE)
                    .max(round.start_tick + 1)
                    .max(129);
                let end = (ks.last().unwrap().tick + 4 * TICK_RATE)
                    .min(round.official_end_tick)
                    .min(death.map(|tick| tick + 2 * TICK_RATE).unwrap_or(i32::MAX));
                Highlight {
                    id: String::new(),
                    round: round.number,
                    start_tick: start,
                    end_tick: end,
                    tick_rate: TICK_RATE,
                    labels: vec![label],
                    kill_ticks: ks.iter().map(|k| k.tick).collect(),
                    kills: ks.len(),
                    score,
                }
            })
            .filter(|h| h.end_tick > h.start_tick)
            .collect();
        intervals.sort_by_key(|h| h.start_tick);
        for h in intervals {
            if let Some(last) = result.last_mut() {
                if last.round == h.round && h.start_tick <= last.end_tick {
                    last.end_tick = last.end_tick.max(h.end_tick);
                    last.score = last.score.max(h.score);
                    last.labels.extend(h.labels);
                    last.labels.sort();
                    last.labels.dedup();
                    last.kill_ticks.extend(h.kill_ticks);
                    last.kill_ticks.sort();
                    last.kill_ticks.dedup();
                    last.kills = last.kill_ticks.len();
                    continue;
                }
            }
            result.push(h);
        }
    }
    for h in &mut result {
        h.id = format!("{}-{}-{}", h.round, h.start_tick, h.end_tick);
    }
    result.sort_by_key(|h| (std::cmp::Reverse(h.score), h.start_tick));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::RoundFact;
    use crate::ticks::TickObservations;
    fn facts(times: &[i32]) -> MatchFacts {
        MatchFacts {
            map: "de_test".into(),
            players: vec![],
            rounds: vec![RoundFact {
                number: 1,
                start_tick: 1000,
                freeze_end_tick: Some(1100),
                end_tick: 9000,
                official_end_tick: 9300,
                winner: Side::Terrorist,
            }],
            kills: times
                .iter()
                .enumerate()
                .map(|(i, t)| KillFact {
                    tick: *t,
                    attacker: Some(PlayerId(1)),
                    victim: Some(PlayerId(i as u64 + 2)),
                    assister: None,
                    attacker_side: Side::Terrorist,
                    victim_side: Side::CounterTerrorist,
                    headshot: false,
                    weapon: "ak47".into(),
                })
                .collect(),
            damages: vec![],
            blinds: vec![],
            shots: vec![],
            bullets: vec![],
            ticks: TickObservations::empty(),
            tick_rows: 0,
        }
    }
    #[test]
    fn ace_merges_overlapping_double_kills() {
        let hs = detect(&facts(&[2000, 2100, 2200, 2300, 2400]), PlayerId(1));
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].kills, 5);
        assert!(hs[0].labels.contains(&"Ace".into()));
        assert_eq!((hs[0].start_tick, hs[0].end_tick), (1808, 2656));
    }
    #[test]
    fn excludes_teamkills_other_players_and_slow_doubles() {
        let mut f = facts(&[2000, 3000]);
        assert!(detect(&f, PlayerId(1)).is_empty());
        f.kills[1].tick = 2100;
        f.kills[1].victim_side = Side::Terrorist;
        assert!(detect(&f, PlayerId(1)).is_empty());
        assert!(detect(&facts(&[2000, 2100]), PlayerId(99)).is_empty());
    }
    #[test]
    fn yaw_wrap_does_not_look_like_a_flick() {
        assert_eq!(angular_distance(0.0, 179.0, 0.0, -179.0), 2.0);
    }
    #[test]
    fn awp_flick_needs_a_recent_shot_and_fast_view_change() {
        use crate::match_facts::ShotFact;
        use crate::ticks::TickSample;
        let mut f = facts(&[2000]);
        f.kills[0].weapon = "awp".into();
        f.shots.push(ShotFact {
            tick: 1999,
            player: Some(PlayerId(1)),
            side: Side::Terrorist,
            weapon: "weapon_awp".into(),
        });
        f.ticks = TickObservations::from_samples(
            [(1988, 0.0), (1999, 12.0)]
                .into_iter()
                .map(|(tick, yaw)| TickSample {
                    tick,
                    player: PlayerId(1),
                    side: Side::Terrorist,
                    health: 100,
                    origin: [0.0; 3],
                    pitch: 0.0,
                    yaw,
                    duck_amount: 0.0,
                    velocity: 0.0,
                    shots_fired: 0,
                    weapon: "AWP".into(),
                    spotted_by: vec![],
                }),
            &f.rounds,
        );
        let hs = detect(&f, PlayerId(1));
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].labels, vec!["AWP flick candidate"]);
        f.shots[0].tick = 1980;
        assert!(detect(&f, PlayerId(1)).is_empty());
    }

    #[test]
    fn awp_double_allows_fifteen_seconds_but_other_weapons_do_not() {
        let mut f = facts(&[2000, 2960]);
        assert!(detect(&f, PlayerId(1)).is_empty());
        for kill in &mut f.kills {
            kill.weapon = "awp".into();
        }
        let hs = detect(&f, PlayerId(1));
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].labels, vec!["AWP double kill"]);
        assert_eq!(hs[0].kill_ticks, vec![2000, 2960]);
        f.kills[1].tick += 1;
        assert!(detect(&f, PlayerId(1)).is_empty());
    }

    #[test]
    fn lingers_after_death_without_crossing_the_round_boundary() {
        let mut f = facts(&[2000, 2100]);
        let death = KillFact {
            tick: 2150,
            attacker: Some(PlayerId(2)),
            victim: Some(PlayerId(1)),
            assister: None,
            attacker_side: Side::CounterTerrorist,
            victim_side: Side::Terrorist,
            headshot: false,
            weapon: "ak47".into(),
        };
        f.kills.push(death);
        assert_eq!(detect(&f, PlayerId(1))[0].end_tick, 2278);
        f.rounds[0].official_end_tick = 2250;
        assert_eq!(detect(&f, PlayerId(1))[0].end_tick, 2250);
    }

    #[test]
    fn repeated_victim_cannot_manufacture_an_ace() {
        let mut f = facts(&[2000, 2100, 2200, 2300, 2400]);
        for k in &mut f.kills {
            k.victim = Some(PlayerId(2));
        }
        assert!(detect(&f, PlayerId(1)).is_empty());
    }
}
