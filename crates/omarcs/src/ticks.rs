use std::collections::BTreeMap;

use ahash::AHashMap;
use parser::first_pass::prop_controller::{
    NAME_ID, PITCH_ID, PLAYER_X_ID, PLAYER_Y_ID, PLAYER_Z_ID, STEAMID_ID, TICK_ID, VELOCITY_ID,
    WEAPON_NAME_ID, YAW_ID,
};
use parser::maps::FRIENDLY_NAMES_MAPPING;
use parser::parse_demo::DemoOutput;
use parser::second_pass::variants::{PropColumn, VarVec};

use crate::match_facts::{PlayerId, RoundFact, Side, round_index_for_tick};

struct TickRow<'a> {
    round_index: u16,
    player: PlayerId,
    side: Side,
    tick: i32,
    health: i32,
    origin: [f32; 3],
    pitch: f32,
    yaw: f32,
    duck_amount: f32,
    velocity: f32,
    shots_fired: i32,
    weapon: &'a str,
    spotted_by: &'a [u64],
}

/// Compact per-tick player state extracted from the parser dataframe.
///
/// Columns are stored separately, weapons are interned, and spotted-by lists are
/// flattened so Match Facts can drop the generic parser output.
#[derive(Debug)]
pub struct TickObservations {
    tick: Vec<i32>,
    round_index: Vec<u16>,
    player: Vec<PlayerId>,
    side: Vec<Side>,
    health: Vec<u8>,
    origin_x: Vec<f32>,
    origin_y: Vec<f32>,
    origin_z: Vec<f32>,
    pitch: Vec<f32>,
    yaw: Vec<f32>,
    duck_amount: Vec<f32>,
    velocity: Vec<f32>,
    shots_fired: Vec<u16>,
    weapon: Vec<u16>,
    weapons: Vec<String>,
    spotted_start: Vec<u32>,
    spotted_ids: Vec<PlayerId>,
}

#[cfg(test)]
pub struct TickSample {
    pub tick: i32,
    pub player: PlayerId,
    pub side: Side,
    pub health: i32,
    pub origin: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub duck_amount: f32,
    pub velocity: f32,
    pub shots_fired: i32,
    pub weapon: String,
    pub spotted_by: Vec<PlayerId>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl TickObservations {
    pub fn empty() -> Self {
        Self {
            tick: Vec::new(),
            round_index: Vec::new(),
            player: Vec::new(),
            side: Vec::new(),
            health: Vec::new(),
            origin_x: Vec::new(),
            origin_y: Vec::new(),
            origin_z: Vec::new(),
            pitch: Vec::new(),
            yaw: Vec::new(),
            duck_amount: Vec::new(),
            velocity: Vec::new(),
            shots_fired: Vec::new(),
            weapon: Vec::new(),
            weapons: Vec::new(),
            spotted_start: vec![0],
            spotted_ids: Vec::new(),
        }
    }

    pub fn from_demo(
        output: &DemoOutput,
        rounds: &[RoundFact],
        players: &mut BTreeMap<PlayerId, String>,
    ) -> Self {
        let ticks = Column::named(output, "tick");
        let steamids = Column::named(output, "steamid");
        let names = Column::named(output, "name");
        let team_nums = Column::named(output, "team_num");
        let health = Column::named(output, "health");
        let xs = Column::named(output, "X");
        let ys = Column::named(output, "Y");
        let zs = Column::named(output, "Z");
        let pitches = Column::named(output, "pitch");
        let yaws = Column::named(output, "yaw");
        let ducks = Column::named(output, "duck_amount");
        let velocities = Column::named(output, "velocity");
        let shots = Column::named(output, "shots_fired");
        let weapons = Column::named(output, "active_weapon_name");
        let spotted = Column::named(output, "approximate_spotted_by");
        let rows = ticks.len();
        let mut observations = Self::empty();
        observations.reserve(rows);
        let mut intern = AHashMap::new();

        for index in 0..rows {
            let steam_id = steamids.u64_at(index).unwrap_or(0);
            if steam_id == 0 {
                continue;
            }
            let side = Side::from_team_num(team_nums.u32_at(index).unwrap_or(0));
            if side == Side::Unknown {
                continue;
            }
            let tick = ticks.i32_at(index).unwrap_or(0);
            let Some(round_index) = round_index_for_tick(rounds, tick) else {
                continue;
            };
            let player = PlayerId(steam_id);
            if let Some(name) = names.str_at(index).filter(|name| !name.is_empty()) {
                players.entry(player).or_insert_with(|| name.to_owned());
            }
            observations.push(
                &mut intern,
                TickRow {
                    round_index: round_index as u16,
                    player,
                    side,
                    tick,
                    health: health.i32_at(index).unwrap_or(0),
                    origin: [
                        xs.f32_at(index).unwrap_or(0.0),
                        ys.f32_at(index).unwrap_or(0.0),
                        zs.f32_at(index).unwrap_or(0.0),
                    ],
                    pitch: pitches.f32_at(index).unwrap_or(0.0),
                    yaw: yaws.f32_at(index).unwrap_or(0.0),
                    duck_amount: ducks.f32_at(index).unwrap_or(0.0),
                    velocity: velocities.f32_at(index).unwrap_or(0.0),
                    shots_fired: shots.i32_at(index).unwrap_or(0),
                    weapon: weapons.str_at(index).unwrap_or(""),
                    spotted_by: spotted.u64s_at(index),
                },
            );
        }
        observations
    }

    #[cfg(test)]
    pub fn from_samples(
        samples: impl IntoIterator<Item = TickSample>,
        rounds: &[RoundFact],
    ) -> Self {
        let mut observations = Self::empty();
        let mut intern = AHashMap::new();
        for sample in samples {
            if sample.player.0 == 0 || sample.side == Side::Unknown {
                continue;
            }
            let Some(round_index) = round_index_for_tick(rounds, sample.tick) else {
                continue;
            };
            let spotted = sample
                .spotted_by
                .iter()
                .map(|player| player.0)
                .collect::<Vec<_>>();
            observations.push(
                &mut intern,
                TickRow {
                    round_index: round_index as u16,
                    player: sample.player,
                    side: sample.side,
                    tick: sample.tick,
                    health: sample.health,
                    origin: sample.origin,
                    pitch: sample.pitch,
                    yaw: sample.yaw,
                    duck_amount: sample.duck_amount,
                    velocity: sample.velocity,
                    shots_fired: sample.shots_fired,
                    weapon: &sample.weapon,
                    spotted_by: &spotted,
                },
            );
        }
        observations
    }

    pub fn len(&self) -> usize {
        self.tick.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tick.is_empty()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tick(&self, index: usize) -> i32 {
        self.tick[index]
    }

    pub fn round_index(&self, index: usize) -> usize {
        usize::from(self.round_index[index])
    }

    pub fn player(&self, index: usize) -> PlayerId {
        self.player[index]
    }

    pub fn side(&self, index: usize) -> Side {
        self.side[index]
    }

    pub fn health(&self, index: usize) -> i32 {
        i32::from(self.health[index])
    }

    pub fn origin(&self, index: usize) -> [f32; 3] {
        [
            self.origin_x[index],
            self.origin_y[index],
            self.origin_z[index],
        ]
    }

    pub fn pitch(&self, index: usize) -> f32 {
        self.pitch[index]
    }

    pub fn yaw(&self, index: usize) -> f32 {
        self.yaw[index]
    }

    pub fn duck_amount(&self, index: usize) -> f32 {
        self.duck_amount[index]
    }

    pub fn velocity(&self, index: usize) -> f32 {
        self.velocity[index]
    }

    pub fn shots_fired(&self, index: usize) -> i32 {
        i32::from(self.shots_fired[index])
    }

    pub fn weapon(&self, index: usize) -> &str {
        &self.weapons[usize::from(self.weapon[index])]
    }

    pub fn spotted_by(&self, index: usize) -> &[PlayerId] {
        let start = self.spotted_start[index] as usize;
        let end = self.spotted_start[index + 1] as usize;
        &self.spotted_ids[start..end]
    }

    pub fn unique_players(&self) -> Vec<PlayerId> {
        let mut players = self.player.clone();
        players.sort();
        players.dedup();
        players
    }

    pub fn majority_sides(&self, player: PlayerId) -> BTreeMap<usize, Side> {
        let mut counts = BTreeMap::<(usize, Side), usize>::new();
        for index in 0..self.len() {
            if self.player[index] != player {
                continue;
            }
            let side = self.side[index];
            if side == Side::Unknown {
                continue;
            }
            *counts
                .entry((usize::from(self.round_index[index]), side))
                .or_default() += 1;
        }

        let mut best = BTreeMap::<usize, (Side, usize)>::new();
        for ((round_index, side), count) in counts {
            let replace = best
                .get(&round_index)
                .is_none_or(|(_, best_count)| count > *best_count);
            if replace {
                best.insert(round_index, (side, count));
            }
        }
        best.into_iter()
            .map(|(round_index, (side, _))| (round_index, side))
            .collect()
    }

    fn reserve(&mut self, rows: usize) {
        self.tick.reserve(rows);
        self.round_index.reserve(rows);
        self.player.reserve(rows);
        self.side.reserve(rows);
        self.health.reserve(rows);
        self.origin_x.reserve(rows);
        self.origin_y.reserve(rows);
        self.origin_z.reserve(rows);
        self.pitch.reserve(rows);
        self.yaw.reserve(rows);
        self.duck_amount.reserve(rows);
        self.velocity.reserve(rows);
        self.shots_fired.reserve(rows);
        self.weapon.reserve(rows);
        self.spotted_start.reserve(rows + 1);
        self.spotted_ids.reserve(rows);
    }

    fn push(&mut self, intern: &mut AHashMap<String, u16>, row: TickRow<'_>) {
        self.tick.push(row.tick);
        self.round_index.push(row.round_index);
        self.player.push(row.player);
        self.side.push(row.side);
        self.health
            .push(row.health.clamp(0, i32::from(u8::MAX)) as u8);
        self.origin_x.push(row.origin[0]);
        self.origin_y.push(row.origin[1]);
        self.origin_z.push(row.origin[2]);
        self.pitch.push(row.pitch);
        self.yaw.push(row.yaw);
        self.duck_amount.push(row.duck_amount);
        self.velocity.push(row.velocity);
        self.shots_fired
            .push(row.shots_fired.clamp(0, i32::from(u16::MAX)) as u16);
        let weapon_id = intern.get(row.weapon).copied().unwrap_or_else(|| {
            let id = self.weapons.len() as u16;
            intern.insert(row.weapon.to_owned(), id);
            self.weapons.push(row.weapon.to_owned());
            id
        });
        self.weapon.push(weapon_id);
        for steam_id in row.spotted_by {
            if *steam_id != 0 {
                self.spotted_ids.push(PlayerId(*steam_id));
            }
        }
        self.spotted_start.push(self.spotted_ids.len() as u32);
    }
}

struct Column<'a> {
    data: Option<&'a VarVec>,
}

impl<'a> Column<'a> {
    fn named(output: &'a DemoOutput, name: &str) -> Self {
        Self {
            data: column(output, name).and_then(|column| column.data.as_ref()),
        }
    }

    fn len(&self) -> usize {
        match self.data {
            Some(VarVec::Bool(values)) => values.len(),
            Some(VarVec::I32(values)) => values.len(),
            Some(VarVec::F32(values)) => values.len(),
            Some(VarVec::String(values)) => values.len(),
            Some(VarVec::U32(values)) => values.len(),
            Some(VarVec::U64(values)) => values.len(),
            Some(VarVec::StringVec(values)) => values.len(),
            Some(VarVec::U64Vec(values)) => values.len(),
            Some(VarVec::U32Vec(values)) => values.len(),
            Some(VarVec::XYVec(values)) => values.len(),
            Some(VarVec::XYZVec(values)) => values.len(),
            Some(VarVec::Stickers(values)) => values.len(),
            Some(VarVec::InputHistory(values)) => values.len(),
            Some(VarVec::UserCmdSubtickMoves(values)) => values.len(),
            None => 0,
        }
    }

    fn i32_at(&self, index: usize) -> Option<i32> {
        match self.data {
            Some(VarVec::I32(values)) => values.get(index).copied().flatten(),
            Some(VarVec::U32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .and_then(|value| i32::try_from(value).ok()),
            Some(VarVec::U64(values)) => values
                .get(index)
                .copied()
                .flatten()
                .and_then(|value| i32::try_from(value).ok()),
            Some(VarVec::F32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .map(|value| value as i32),
            Some(VarVec::Bool(values)) => values.get(index).copied().flatten().map(i32::from),
            _ => None,
        }
    }

    fn u32_at(&self, index: usize) -> Option<u32> {
        match self.data {
            Some(VarVec::U32(values)) => values.get(index).copied().flatten(),
            Some(VarVec::I32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .and_then(|value| u32::try_from(value).ok()),
            Some(VarVec::U64(values)) => values
                .get(index)
                .copied()
                .flatten()
                .and_then(|value| u32::try_from(value).ok()),
            _ => None,
        }
    }

    fn u64_at(&self, index: usize) -> Option<u64> {
        match self.data {
            Some(VarVec::U64(values)) => values.get(index).copied().flatten(),
            Some(VarVec::U32(values)) => values.get(index).copied().flatten().map(u64::from),
            Some(VarVec::I32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .and_then(|value| u64::try_from(value).ok()),
            Some(VarVec::String(values)) => values
                .get(index)
                .and_then(|value| value.as_ref())
                .and_then(|value| value.parse().ok()),
            _ => None,
        }
    }

    fn f32_at(&self, index: usize) -> Option<f32> {
        match self.data {
            Some(VarVec::F32(values)) => values.get(index).copied().flatten(),
            Some(VarVec::I32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .map(|value| value as f32),
            Some(VarVec::U32(values)) => values
                .get(index)
                .copied()
                .flatten()
                .map(|value| value as f32),
            _ => None,
        }
    }

    fn str_at(&self, index: usize) -> Option<&'a str> {
        match self.data {
            Some(VarVec::String(values)) => values.get(index).and_then(|value| value.as_deref()),
            _ => None,
        }
    }

    fn u64s_at(&self, index: usize) -> &'a [u64] {
        match self.data {
            Some(VarVec::U64Vec(values)) => values.get(index).map(Vec::as_slice).unwrap_or(&[]),
            _ => &[],
        }
    }
}

fn column<'a>(output: &'a DemoOutput, name: &str) -> Option<&'a PropColumn> {
    output.df.get(&column_id(output, name)?)
}

fn column_id(output: &DemoOutput, name: &str) -> Option<u32> {
    let special = match name {
        "tick" => Some(TICK_ID),
        "steamid" => Some(STEAMID_ID),
        "name" => Some(NAME_ID),
        "X" => Some(PLAYER_X_ID),
        "Y" => Some(PLAYER_Y_ID),
        "Z" => Some(PLAYER_Z_ID),
        "pitch" => Some(PITCH_ID),
        "yaw" => Some(YAW_ID),
        "velocity" => Some(VELOCITY_ID),
        "active_weapon_name" => Some(WEAPON_NAME_ID),
        _ => None,
    };
    special
        .or_else(|| output.prop_controller.name_to_special_id.get(name).copied())
        .or_else(|| {
            output
                .prop_controller
                .prop_infos
                .iter()
                .find(|info| info.prop_friendly_name == name || info.prop_name == name)
                .map(|info| info.id)
        })
        .or_else(|| {
            let real_name = FRIENDLY_NAMES_MAPPING.get(name).copied().unwrap_or(name);
            output.prop_controller.name_to_id.get(real_name).copied()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: PlayerId = PlayerId(76_561_198_000_000_001);
    const TEAMMATE: PlayerId = PlayerId(76_561_198_000_000_002);
    const ENEMY: PlayerId = PlayerId(76_561_198_000_000_003);

    fn rounds() -> Vec<RoundFact> {
        vec![
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
        ]
    }

    fn sample(tick: i32, player: PlayerId, side: Side, weapon: &str) -> TickSample {
        TickSample {
            tick,
            player,
            side,
            health: 100,
            origin: [1.0, 2.0, 3.0],
            pitch: 4.0,
            yaw: 5.0,
            duck_amount: 0.25,
            velocity: 80.0,
            shots_fired: 3,
            weapon: weapon.to_owned(),
            spotted_by: vec![ENEMY],
        }
    }

    #[test]
    fn compact_keeps_in_round_combatants_and_interns_weapons() {
        let mut extra = sample(50, PLAYER, Side::CounterTerrorist, "ak47");
        extra.spotted_by = vec![ENEMY, PlayerId(0)];
        let observations = TickObservations::from_samples(
            [
                extra,
                sample(950, PLAYER, Side::Terrorist, "ak47"),
                sample(40, PlayerId(0), Side::Terrorist, "ak47"),
                sample(40, TEAMMATE, Side::Unknown, "ak47"),
                sample(800, PLAYER, Side::CounterTerrorist, "m4a1"),
                sample(960, TEAMMATE, Side::Terrorist, "m4a1"),
            ],
            &rounds(),
        );

        assert!(!observations.is_empty());
        assert_eq!(observations.len(), 3);
        assert_eq!(observations.tick(0), 50);
        assert_eq!(observations.round_index(0), 0);
        assert_eq!(observations.round_index(1), 1);
        assert_eq!(observations.player(0), PLAYER);
        assert_eq!(observations.side(1), Side::Terrorist);
        assert_eq!(observations.health(0), 100);
        assert_eq!(observations.pitch(0), 4.0);
        assert_eq!(observations.yaw(0), 5.0);
        assert_eq!(observations.duck_amount(0), 0.25);
        assert_eq!(observations.velocity(0), 80.0);
        assert_eq!(observations.shots_fired(0), 3);
        assert_eq!(observations.weapon(0), "ak47");
        assert_eq!(observations.weapon(1), "ak47");
        assert_eq!(observations.weapon(2), "m4a1");
        assert_eq!(observations.weapons.len(), 2);
        assert_eq!(observations.origin(0), [1.0, 2.0, 3.0]);
        assert_eq!(observations.spotted_by(0), &[ENEMY]);
        assert_eq!(observations.unique_players(), vec![PLAYER, TEAMMATE]);
    }

    #[test]
    fn majority_side_uses_tick_observations_per_round() {
        let observations = TickObservations::from_samples(
            [
                sample(10, PLAYER, Side::CounterTerrorist, "ak47"),
                sample(20, PLAYER, Side::CounterTerrorist, "ak47"),
                sample(30, PLAYER, Side::Terrorist, "ak47"),
                sample(910, PLAYER, Side::Terrorist, "ak47"),
            ],
            &rounds(),
        );

        let sides = observations.majority_sides(PLAYER);
        assert_eq!(sides.get(&0), Some(&Side::CounterTerrorist));
        assert_eq!(sides.get(&1), Some(&Side::Terrorist));
    }
}
