use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::config;

const WEAPONS: &[(&str, &str, &str)] = &[
    ("ak47", "AK-47", "AK"),
    ("galilar", "Galil AR", "GALIL"),
    ("m4a4", "M4A4", "M4A4"),
    ("m4a1_silencer", "M4A1-S", "M4A1-S"),
];

pub struct Store {
    root: PathBuf,
    summary_path: PathBuf,
    connection: Connection,
}

impl Store {
    pub fn open(root: Option<&Path>) -> Result<Self> {
        let root = root
            .map(Path::to_path_buf)
            .unwrap_or_else(|| config::state_home().join("omarcs"));
        fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        let database_path = root.join("omarcs.db");
        let connection = Connection::open(&database_path)
            .with_context(|| format!("opening {}", database_path.display()))?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS matches (
                id TEXT PRIMARY KEY,
                checksum TEXT NOT NULL UNIQUE,
                played_at TEXT NOT NULL,
                map TEXT NOT NULL,
                player_steam_id TEXT NOT NULL,
                player_name TEXT NOT NULL,
                result TEXT NOT NULL,
                rounds_for INTEGER NOT NULL,
                rounds_against INTEGER NOT NULL,
                rating REAL NOT NULL,
                adr REAL NOT NULL,
                kast REAL NOT NULL,
                kd REAL NOT NULL,
                payload TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            summary_path: root.join("summary.json"),
            root,
            connection,
        })
    }

    pub fn has_checksum(&self, checksum: &str, analysis_version: u32) -> Result<bool> {
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM matches WHERE checksum = ?1",
                [checksum],
                |row| row.get(0),
            )
            .optional()?;
        Ok(payload
            .and_then(|payload| serde_json::from_str::<Value>(&payload).ok())
            .and_then(|payload| {
                payload
                    .get("analysisVersion")
                    .and_then(Value::as_u64)
                    .or(Some(1))
            })
            .is_some_and(|version| version >= u64::from(analysis_version)))
    }

    pub fn save_match(&self, match_report: &Value) -> Result<()> {
        let stats = required(match_report, "stats")?;
        let player = required(match_report, "player")?;
        let payload = serde_json::to_string(match_report)?;
        self.connection.execute(
            "INSERT INTO matches (
                id, checksum, played_at, map, player_steam_id, player_name,
                result, rounds_for, rounds_against, rating, adr, kast, kd, payload
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(checksum) DO UPDATE SET payload = excluded.payload",
            params![
                string(match_report, "id")?,
                string(match_report, "checksum")?,
                string(match_report, "playedAt")?,
                string(match_report, "map")?,
                string(player, "steamId")?,
                string(player, "name")?,
                string(stats, "result")?,
                integer(stats, "roundsFor")?,
                integer(stats, "roundsAgainst")?,
                number(stats, "rating")?,
                number(stats, "adr")?,
                number(stats, "kast")?,
                number(stats, "kd")?,
                payload,
            ],
        )?;
        Ok(())
    }

    pub fn matches(&self, limit: usize) -> Result<Vec<Value>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM matches ORDER BY played_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit as i64], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    payload.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn write_status(&self, status: &str, message: &str) -> Result<Value> {
        let mut summary = self.build_summary(20)?;
        summary["status"] = json!(status);
        summary["message"] = json!(message);
        self.atomic_json(&summary)?;
        Ok(summary)
    }

    pub fn publish(&self, limit: usize) -> Result<Value> {
        let summary = self.build_summary(limit)?;
        self.atomic_json(&summary)?;
        Ok(summary)
    }

    pub fn current_summary(&self) -> Result<Value> {
        if let Ok(contents) = fs::read_to_string(&self.summary_path)
            && let Ok(summary) = serde_json::from_str(&contents)
        {
            return Ok(summary);
        }
        self.publish(20)
    }

    fn build_summary(&self, limit: usize) -> Result<Value> {
        let matches = self.matches(limit)?;
        let recent = matches.iter().take(5).cloned().collect::<Vec<_>>();
        let trends_source = matches.iter().take(10).collect::<Vec<_>>();
        let count = trends_source.len();
        let wins = trends_source
            .iter()
            .filter(|item| item["stats"]["result"] == "W")
            .count();
        let average = |key: &str, places: u32| -> f64 {
            if count == 0 {
                return 0.0;
            }
            round_to(
                trends_source
                    .iter()
                    .map(|item| item["stats"][key].as_f64().unwrap_or(0.0))
                    .sum::<f64>()
                    / count as f64,
                places,
            )
        };
        Ok(json!({
            "schemaVersion": 2,
            "generatedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false),
            "status": if matches.is_empty() { "empty" } else { "ready" },
            "message": if matches.is_empty() { "Import a CS2 demo to get started." } else { "" },
            "player": matches.first().map(|item| item["player"].clone()).unwrap_or(Value::Null),
            "latest": matches.first().cloned().unwrap_or(Value::Null),
            "recent": recent,
            "trends": { "matches": count, "wins": wins, "rating": average("rating", 2), "adr": average("adr", 1), "kast": average("kast", 1) },
            "sprayControl": aggregate_sprays(&matches.iter().take(10).cloned().collect::<Vec<_>>()),
        }))
    }

    fn atomic_json(&self, payload: &Value) -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = self
            .root
            .join(format!("summary.{}.{nonce}.json", std::process::id()));
        let result = (|| -> Result<()> {
            let mut output = File::create(&temporary)?;
            serde_json::to_writer_pretty(&mut output, payload)?;
            output.write_all(b"\n")?;
            output.sync_all()?;
            fs::rename(&temporary, &self.summary_path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| anyhow!("Match Report missing {key}"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    required(value, key)?
        .as_str()
        .ok_or_else(|| anyhow!("Match Report {key} is not text"))
}

fn integer(value: &Value, key: &str) -> Result<i64> {
    required(value, key)?
        .as_i64()
        .ok_or_else(|| anyhow!("Match Report {key} is not an integer"))
}

fn number(value: &Value, key: &str) -> Result<f64> {
    required(value, key)?
        .as_f64()
        .ok_or_else(|| anyhow!("Match Report {key} is not a number"))
}

fn aggregate_sprays(matches: &[Value]) -> Value {
    let mut bursts: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for match_report in matches.iter().take(10) {
        for burst in match_report
            .get("sprays")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(weapon) = burst.get("weapon").and_then(Value::as_str) else {
                continue;
            };
            let Some(shots) = burst.get("shots").and_then(Value::as_array) else {
                continue;
            };
            if WEAPONS.iter().any(|candidate| candidate.0 == weapon) && shots.len() >= 5 {
                bursts.entry(weapon).or_default().push(burst);
            }
        }
    }
    let weapons = WEAPONS.iter().map(|(id, name, short_name)| {
        let weapon_bursts = bursts.get(id).cloned().unwrap_or_default();
        let mut aggregated = Vec::new();
        for number in 1..=10 {
            let points = weapon_bursts.iter().flat_map(|burst| burst["shots"].as_array().into_iter().flatten().take(10))
                .filter(|shot| shot["number"].as_u64() == Some(number)).collect::<Vec<_>>();
            if points.is_empty() { continue; }
            let xs = points.iter().filter_map(|shot| shot["x"].as_f64()).collect::<Vec<_>>();
            let ys = points.iter().filter_map(|shot| shot["y"].as_f64()).collect::<Vec<_>>();
            aggregated.push(json!({
                "number": number, "x": round_to(median(&xs), 2), "y": round_to(median(&ys), 2),
                "radiusX": round_to(((percentile(&xs, 0.75) - percentile(&xs, 0.25)) / 2.0).max(1.5), 2),
                "radiusY": round_to(((percentile(&ys, 0.75) - percentile(&ys, 0.25)) / 2.0).max(1.5), 2),
                "samples": points.len(),
            }));
        }
        let spray_count = weapon_bursts.len();
        json!({ "id": id, "name": name, "shortName": short_name, "sprays": spray_count,
            "confidence": confidence(spray_count), "coach": coach_spray(name, &aggregated, spray_count), "shots": aggregated })
    }).collect::<Vec<_>>();
    json!({"matches": matches.len().min(10), "weapons": weapons})
}

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    if ordered.is_empty() {
        return 0.0;
    }
    if ordered.len() == 1 {
        return ordered[0];
    }
    let position = fraction * (ordered.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    ordered[lower] * (1.0 - position.fract()) + ordered[upper] * position.fract()
}

fn median(values: &[f64]) -> f64 {
    percentile(values, 0.5)
}

fn confidence(count: usize) -> &'static str {
    match count {
        15.. => "HIGH",
        8..=14 => "GOOD",
        4..=7 => "LOW",
        _ => "MORE DATA NEEDED",
    }
}

fn coach_spray(name: &str, shots: &[Value], count: usize) -> String {
    if count < 4 {
        return format!(
            "Only {count} qualifying {name} spray{}; keep collecting matches.",
            if count == 1 { "" } else { "s" }
        );
    }
    let minimum = 2.max(count.div_ceil(2));
    let qualified = |shot: &&Value| shot["samples"].as_u64().unwrap_or(0) >= minimum as u64;
    let mut late = shots
        .iter()
        .filter(|shot| (6..=10).contains(&shot["number"].as_u64().unwrap_or(0)) && qualified(shot))
        .collect::<Vec<_>>();
    if late.is_empty() {
        late = shots.iter().filter(qualified).collect();
    }
    let horizontal = median(
        &late
            .iter()
            .filter_map(|shot| shot["x"].as_f64())
            .collect::<Vec<_>>(),
    );
    let vertical = median(
        &late
            .iter()
            .filter_map(|shot| shot["y"].as_f64())
            .collect::<Vec<_>>(),
    );
    let prefix = if count < 8 { "Early read: " } else { "" };
    let suffix = if count < 8 {
        " More sprays will firm this up."
    } else {
        ""
    };
    let detail = if horizontal.abs() < 7.0 && vertical.abs() < 7.0 {
        "the later bullets stay centred; this spray shape is controlled."
    } else if horizontal.abs() >= vertical.abs() && horizontal > 0.0 {
        "the later bullets drift right; pull slightly further left after bullet 5."
    } else if horizontal.abs() >= vertical.abs() {
        "the later bullets drift left; ease the leftward pull after bullet 5."
    } else if vertical > 0.0 {
        "the later bullets climb high; pull down more firmly after bullet 5."
    } else {
        "the later bullets land low; ease the downward pull after bullet 5."
    };
    format!("{prefix}{detail}{suffix}")
}

fn round_to(value: f64, places: u32) -> f64 {
    format!("{value:.precision$}", precision = places as usize)
        .parse()
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("omarcs-{name}-{}-{stamp}", std::process::id()))
    }

    fn sample(id: &str, played_at: &str, result: &str, rating: f64) -> Value {
        json!({"id": id, "checksum": id.repeat(4), "path": format!("/{id}.dem"), "playedAt": played_at,
            "map": "de_mirage", "player": {"steamId": "76561198000000001", "name": "Kieren"},
            "stats": {"result": result, "roundsFor": 13, "roundsAgainst": 9, "rating": rating, "adr": 80.0, "kast": 75.0, "kd": 1.2},
            "sprays": [], "insights": ["Test note"]})
    }

    #[test]
    fn publishes_latest_and_trends() {
        let root = root("storage");
        let store = Store::open(Some(&root)).unwrap();
        store
            .save_match(&sample("a", "2026-01-01T00:00:00+00:00", "L", 0.8))
            .unwrap();
        store
            .save_match(&sample("b", "2026-01-02T00:00:00+00:00", "W", 1.2))
            .unwrap();
        let summary = store.publish(20).unwrap();
        assert_eq!(summary["latest"]["id"], "b");
        assert_eq!(summary["trends"]["wins"], 1);
        assert_eq!(summary["trends"]["rating"], 1.0);
        assert!(root.join("summary.json").exists());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn analysis_version_invalidates_old_match() {
        let root = root("version");
        let store = Store::open(Some(&root)).unwrap();
        let mut report = sample("a", "2026-01-01T00:00:00+00:00", "W", 1.1);
        store.save_match(&report).unwrap();
        assert!(store.has_checksum("aaaa", 1).unwrap());
        assert!(!store.has_checksum("aaaa", 2).unwrap());
        report["analysisVersion"] = json!(2);
        store.save_match(&report).unwrap();
        assert!(store.has_checksum("aaaa", 2).unwrap());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn aggregates_sprays_by_bullet_number() {
        let bursts = [[0, 1, 2, 3, 4], [0, 2, 4, 6, 8], [0, 3, 6, 9, 12], [0, 4, 8, 12, 16]]
            .into_iter().map(|offsets| json!({"weapon": "ak47", "shots": offsets.into_iter().enumerate()
                .map(|(index, offset)| json!({"number": index + 1, "x": offset, "y": -offset})).collect::<Vec<_>>() })).collect::<Vec<_>>();
        let summary = aggregate_sprays(&[json!({"sprays": bursts})]);
        let ak = &summary["weapons"][0];
        assert_eq!(ak["sprays"], 4);
        assert_eq!(ak["confidence"], "LOW");
        assert_eq!(ak["shots"][4]["x"], 10.0);
        assert_eq!(ak["shots"][4]["y"], -10.0);
        assert_eq!(ak["shots"][4]["samples"], 4);
    }
}
