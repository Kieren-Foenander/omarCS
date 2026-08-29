use ahash::AHashMap;

/// Compact per-tick player state accumulated during the second pass.
///
/// Columns are stored separately, weapons are interned, and spotted-by lists are
/// flattened so the generic parser dataframe does not have to be materialised.
#[derive(Debug, Clone)]
pub struct CompactTicks {
    pub tick: Vec<i32>,
    pub steamid: Vec<u64>,
    pub team_num: Vec<u32>,
    pub health: Vec<u8>,
    pub origin_x: Vec<f32>,
    pub origin_y: Vec<f32>,
    pub origin_z: Vec<f32>,
    pub pitch: Vec<f32>,
    pub yaw: Vec<f32>,
    pub duck_amount: Vec<f32>,
    pub velocity: Vec<f32>,
    pub shots_fired: Vec<u16>,
    pub weapon: Vec<u16>,
    pub weapons: Vec<String>,
    pub spotted_start: Vec<u32>,
    pub spotted_ids: Vec<u64>,
    pub names: AHashMap<u64, String>,
    weapon_intern: AHashMap<String, u16>,
}

pub struct CompactTickRow<'a> {
    pub tick: i32,
    pub steamid: u64,
    pub team_num: u32,
    pub health: i32,
    pub origin: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub duck_amount: f32,
    pub velocity: f32,
    pub shots_fired: i32,
    pub weapon: &'a str,
    pub spotted_by: &'a [u64],
    pub name: Option<&'a str>,
}

impl Default for CompactTicks {
    fn default() -> Self {
        Self {
            tick: Vec::new(),
            steamid: Vec::new(),
            team_num: Vec::new(),
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
            names: AHashMap::default(),
            weapon_intern: AHashMap::default(),
        }
    }
}

impl CompactTicks {
    pub fn len(&self) -> usize {
        self.tick.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tick.is_empty()
    }

    pub fn weapon_name(&self, index: usize) -> &str {
        &self.weapons[self.weapon[index] as usize]
    }

    pub fn spotted_by(&self, index: usize) -> &[u64] {
        let start = self.spotted_start[index] as usize;
        let end = self.spotted_start[index + 1] as usize;
        &self.spotted_ids[start..end]
    }

    pub fn push(&mut self, row: CompactTickRow<'_>) {
        self.tick.push(row.tick);
        self.steamid.push(row.steamid);
        self.team_num.push(row.team_num);
        self.health.push(row.health.clamp(0, i32::from(u8::MAX)) as u8);
        self.origin_x.push(row.origin[0]);
        self.origin_y.push(row.origin[1]);
        self.origin_z.push(row.origin[2]);
        self.pitch.push(row.pitch);
        self.yaw.push(row.yaw);
        self.duck_amount.push(row.duck_amount);
        self.velocity.push(row.velocity);
        self.shots_fired.push(row.shots_fired.clamp(0, i32::from(u16::MAX)) as u16);
        let weapon_id = match self.weapon_intern.get(row.weapon) {
            Some(id) => *id,
            None => {
                let id = self.weapons.len() as u16;
                self.weapon_intern.insert(row.weapon.to_owned(), id);
                self.weapons.push(row.weapon.to_owned());
                id
            }
        };
        self.weapon.push(weapon_id);
        for steam_id in row.spotted_by {
            if *steam_id != 0 {
                self.spotted_ids.push(*steam_id);
            }
        }
        self.spotted_start.push(self.spotted_ids.len() as u32);
        if let Some(name) = row.name.filter(|name| !name.is_empty()) {
            self.names.entry(row.steamid).or_insert_with(|| name.to_owned());
        }
    }

    pub fn extend(&mut self, mut other: CompactTicks) {
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = other;
            return;
        }

        let mut remap = vec![0_u16; other.weapons.len()];
        for (index, weapon) in other.weapons.iter().enumerate() {
            remap[index] = match self.weapon_intern.get(weapon) {
                Some(id) => *id,
                None => {
                    let id = self.weapons.len() as u16;
                    self.weapon_intern.insert(weapon.clone(), id);
                    self.weapons.push(weapon.clone());
                    id
                }
            };
        }
        for id in &mut other.weapon {
            *id = remap[*id as usize];
        }

        let spotted_offset = self.spotted_ids.len() as u32;
        self.tick.append(&mut other.tick);
        self.steamid.append(&mut other.steamid);
        self.team_num.append(&mut other.team_num);
        self.health.append(&mut other.health);
        self.origin_x.append(&mut other.origin_x);
        self.origin_y.append(&mut other.origin_y);
        self.origin_z.append(&mut other.origin_z);
        self.pitch.append(&mut other.pitch);
        self.yaw.append(&mut other.yaw);
        self.duck_amount.append(&mut other.duck_amount);
        self.velocity.append(&mut other.velocity);
        self.shots_fired.append(&mut other.shots_fired);
        self.weapon.append(&mut other.weapon);
        self.spotted_ids.append(&mut other.spotted_ids);
        self.spotted_start
            .extend(other.spotted_start.into_iter().skip(1).map(|start| start + spotted_offset));
        for (steamid, name) in other.names {
            self.names.entry(steamid).or_insert(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(tick: i32, steamid: u64, weapon: &'a str, spotted: &'a [u64], name: Option<&'a str>) -> CompactTickRow<'a> {
        CompactTickRow {
            tick,
            steamid,
            team_num: 3,
            health: 100,
            origin: [1.0, 2.0, 3.0],
            pitch: 4.0,
            yaw: 5.0,
            duck_amount: 0.25,
            velocity: 80.0,
            shots_fired: 3,
            weapon,
            spotted_by: spotted,
            name,
        }
    }

    #[test]
    fn interned_weapons_and_flattened_spotted_by_survive_chunk_merge() {
        let mut first = CompactTicks::default();
        first.push(row(10, 1, "ak47", &[2, 0], Some("Alice")));
        first.push(row(11, 1, "ak47", &[], None));

        let mut second = CompactTicks::default();
        second.push(row(20, 1, "ak47", &[3], Some("Alice")));
        second.push(row(21, 2, "m4a1", &[1], Some("Bob")));

        first.extend(second);

        assert_eq!(first.len(), 4);
        assert_eq!(first.weapons, vec!["ak47".to_owned(), "m4a1".to_owned()]);
        assert_eq!(first.weapon_name(0), "ak47");
        assert_eq!(first.weapon_name(3), "m4a1");
        assert_eq!(first.spotted_by(0), &[2]);
        assert_eq!(first.spotted_by(1), &[] as &[u64]);
        assert_eq!(first.spotted_by(2), &[3]);
        assert_eq!(first.spotted_by(3), &[1]);
        assert_eq!(first.names.get(&1).map(String::as_str), Some("Alice"));
        assert_eq!(first.names.get(&2).map(String::as_str), Some("Bob"));
    }
}
