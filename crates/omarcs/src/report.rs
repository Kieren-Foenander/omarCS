use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::coaching;
use crate::geometry::{self, Mesh};
use crate::match_facts::{MatchFacts, PlayerId};
use crate::mechanics::{self, MechanicsMetrics};
use crate::metrics::{self, PlayerMetrics};
use crate::parser_adapter;
use crate::spray::{self, SprayBurst};

pub const ANALYSIS_VERSION: u32 = 4;

pub struct ReportMeta {
    pub path: String,
    pub checksum: String,
    pub played_at: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPlayer {
    pub steam_id: String,
    pub name: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReport {
    pub analysis_version: u32,
    pub id: String,
    pub checksum: String,
    pub path: String,
    pub played_at: String,
    pub map: String,
    pub player: ReportPlayer,
    pub stats: ReportStats,
    pub sprays: Vec<SprayBurst>,
    pub insights: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ReportStats {
    #[serde(flatten)]
    metrics: CoreStats,
    #[serde(flatten)]
    mechanics: MechanicsMetrics,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreStats {
    kills: usize,
    deaths: usize,
    assists: usize,
    kd: f64,
    adr: f64,
    kast: f64,
    rating: f64,
    headshot_percent: f64,
    opening_kills: usize,
    opening_deaths: usize,
    trade_kills: usize,
    traded_deaths: usize,
    utility_damage: i32,
    enemies_flashed: usize,
    friends_flashed: usize,
    enemy_flash_seconds: f64,
    rounds: usize,
    rounds_for: usize,
    rounds_against: usize,
    result: &'static str,
}

pub fn generate(demo: &Path, player_selector: &str) -> Result<MatchReport> {
    let checksum = checksum_path(demo)?;
    let played_at = played_at(demo)?;
    let path = demo
        .canonicalize()
        .unwrap_or_else(|_| demo.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let parsed = parser_adapter::parse(demo)?;
    let facts = MatchFacts::from_output(parsed.output);
    let player = metrics::resolve_player(&facts, player_selector)?;
    let mesh = geometry::load_map_mesh(&facts.map);
    Ok(assemble(
        &facts,
        player,
        ReportMeta {
            path,
            checksum,
            played_at,
        },
        mesh.as_ref(),
    ))
}

pub fn assemble(
    facts: &MatchFacts,
    player: PlayerId,
    meta: ReportMeta,
    mesh: Option<&Mesh>,
) -> MatchReport {
    let metrics = metrics::calculate(facts, player);
    let mechanics = mechanics::calculate(facts, player, mesh);
    let sprays = spray::calculate(facts, player, mesh);
    let insights = coaching::calculate(&metrics, &mechanics);
    let name = metrics.name.clone();

    MatchReport {
        analysis_version: ANALYSIS_VERSION,
        id: meta.checksum.chars().take(16).collect(),
        checksum: meta.checksum,
        path: meta.path,
        played_at: meta.played_at,
        map: facts.map.clone(),
        player: ReportPlayer {
            steam_id: player.0.to_string(),
            name,
        },
        stats: ReportStats::from_parts(metrics, mechanics),
        sprays,
        insights,
    }
}

pub fn checksum_path(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn played_at(path: &Path) -> Result<String> {
    let modified = path
        .metadata()
        .with_context(|| format!("statting {}", path.display()))?
        .modified()
        .with_context(|| format!("mtime for {}", path.display()))?;
    Ok(DateTime::<Utc>::from(modified).to_rfc3339_opts(SecondsFormat::Secs, false))
}

impl ReportStats {
    fn from_parts(metrics: PlayerMetrics, mechanics: MechanicsMetrics) -> Self {
        Self {
            metrics: CoreStats {
                kills: metrics.kills,
                deaths: metrics.deaths,
                assists: metrics.assists,
                kd: metrics.kd,
                adr: metrics.adr,
                kast: metrics.kast,
                rating: metrics.rating,
                headshot_percent: metrics.headshot_percent,
                opening_kills: metrics.opening_kills,
                opening_deaths: metrics.opening_deaths,
                trade_kills: metrics.trade_kills,
                traded_deaths: metrics.traded_deaths,
                utility_damage: metrics.utility_damage,
                enemies_flashed: metrics.enemies_flashed,
                friends_flashed: metrics.friends_flashed,
                enemy_flash_seconds: metrics.enemy_flash_seconds,
                rounds: metrics.rounds,
                rounds_for: metrics.rounds_for,
                rounds_against: metrics.rounds_against,
                result: metrics.result,
            },
            mechanics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::{BlindFact, DamageFact, PlayerFact, Side};
    use crate::ticks::TickObservations;
    use serde_json::json;

    const PLAYER: PlayerId = PlayerId(76_561_198_000_000_001);
    const TEAMMATE: PlayerId = PlayerId(76_561_198_000_000_002);
    const ENEMY: PlayerId = PlayerId(76_561_198_000_000_003);

    fn quiet_facts() -> MatchFacts {
        MatchFacts {
            map: "de_mirage".to_owned(),
            players: vec![PlayerFact {
                steam_id: PLAYER,
                name: "Kieren".to_owned(),
            }],
            rounds: vec![],
            kills: vec![],
            damages: vec![DamageFact {
                tick: 1,
                attacker: Some(PLAYER),
                victim: Some(ENEMY),
                attacker_side: Side::Terrorist,
                victim_side: Side::CounterTerrorist,
                weapon: "hegrenade".to_owned(),
                health_damage: 20,
            }],
            blinds: vec![],
            shots: vec![],
            bullets: vec![],
            ticks: TickObservations::empty(),
            tick_rows: 0,
        }
    }

    fn meta() -> ReportMeta {
        ReportMeta {
            path: "/tmp/match.dem".to_owned(),
            checksum: "abcdef0123456789deadbeefcafebabe".to_owned(),
            played_at: "2026-01-02T03:04:05.000000+00:00".to_owned(),
        }
    }

    #[test]
    fn matches_python_analyze_demo_shape() {
        let report = assemble(&quiet_facts(), PLAYER, meta(), None);
        let value = serde_json::to_value(&report).expect("json");

        assert_eq!(value["analysisVersion"], 4);
        assert_eq!(value["id"], "abcdef0123456789");
        assert_eq!(value["checksum"], "abcdef0123456789deadbeefcafebabe");
        assert_eq!(value["path"], "/tmp/match.dem");
        assert_eq!(value["playedAt"], "2026-01-02T03:04:05.000000+00:00");
        assert_eq!(value["map"], "de_mirage");
        assert_eq!(
            value["player"],
            json!({
                "steamId": "76561198000000001",
                "name": "Kieren",
            })
        );
        assert!(value["player"]["steamId"].is_string());
        assert!(value["stats"].get("steamId").is_none());
        assert!(value["stats"].get("name").is_none());
        assert_eq!(value["stats"]["result"], "D");
        assert_eq!(value["stats"]["mechanicsQuality"], "radar-beta");
        assert_eq!(value["sprays"], json!([]));
        assert_eq!(
            value["insights"],
            json!(["No obvious outlier this match; compare it with your next few games."])
        );

        let stats = value["stats"].as_object().expect("stats object");
        for key in [
            "kills",
            "deaths",
            "assists",
            "kd",
            "adr",
            "kast",
            "rating",
            "headshotPercent",
            "openingKills",
            "openingDeaths",
            "tradeKills",
            "tradedDeaths",
            "utilityDamage",
            "enemiesFlashed",
            "friendsFlashed",
            "enemyFlashSeconds",
            "rounds",
            "roundsFor",
            "roundsAgainst",
            "result",
            "crosshairPlacement",
            "horizontalAdjustment",
            "verticalAdjustment",
            "reactionTimeMs",
            "timeToDamageMs",
            "spottedAccuracy",
            "counterStrafePercent",
            "mechanicsEngagements",
            "mechanicsExposures",
            "spottedShots",
            "counterStrafeShots",
            "mechanicsQuality",
        ] {
            assert!(stats.contains_key(key), "missing stats.{key}");
        }
    }

    #[test]
    fn includes_coaching_notes_from_merged_stats() {
        let mut facts = quiet_facts();
        facts.blinds = vec![
            BlindFact {
                tick: 10,
                attacker: Some(PLAYER),
                victim: Some(TEAMMATE),
                attacker_side: Side::Terrorist,
                victim_side: Side::Terrorist,
                duration_seconds: 1.0,
            },
            BlindFact {
                tick: 11,
                attacker: Some(PLAYER),
                victim: Some(TEAMMATE),
                attacker_side: Side::Terrorist,
                victim_side: Side::Terrorist,
                duration_seconds: 1.0,
            },
        ];

        let report = assemble(&facts, PLAYER, meta(), None);
        assert_eq!(
            report.insights,
            vec!["You flashed teammates 2 times; tighten flash timing and calls.".to_owned()]
        );
    }

    #[test]
    fn checksum_matches_python_sha256_hex() {
        let directory = std::env::temp_dir();
        let path = directory.join("omarcs-report-checksum.dem");
        std::fs::write(&path, b"hello").expect("write fixture");
        let digest = checksum_path(&path).expect("checksum");
        std::fs::remove_file(&path).ok();
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn played_at_matches_python_isoformat_seconds() {
        let directory = std::env::temp_dir();
        let path = directory.join("omarcs-report-played-at.dem");
        std::fs::write(&path, b"hello").expect("write fixture");
        let stamp = played_at(&path).expect("played_at");
        std::fs::remove_file(&path).ok();
        assert!(
            stamp.ends_with("+00:00"),
            "expected UTC offset, got {stamp}"
        );
        assert!(
            !stamp.contains('.'),
            "Python isoformat omits microseconds when they are zero: {stamp}"
        );
        assert_eq!(stamp.len(), "2026-01-02T03:04:05+00:00".len());
    }

    #[test]
    #[ignore = "requires the golden Demo in OMARCS_REPORT_FIXTURE (or OMARCS_DEMO_FIXTURE)"]
    fn real_demo_matches_golden_match_report() {
        let path = report_fixture_path();
        let digest = checksum_path(&path).expect("checksum");
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/golden-match-report.json"))
                .expect("golden json");
        assert_eq!(
            digest,
            golden["checksum"].as_str().expect("golden checksum"),
            "fixture is not the committed golden Demo"
        );

        let report = generate(&path, "76561198959939965").expect("generate report");
        let mut actual = serde_json::to_value(&report).expect("json");
        assert_eq!(actual["stats"]["mechanicsQuality"], "geometry");
        let actual_object = actual.as_object_mut().expect("object");
        actual_object.remove("path");
        actual_object.remove("playedAt");

        for key in [
            "kills",
            "deaths",
            "assists",
            "kd",
            "adr",
            "kast",
            "rating",
            "headshotPercent",
            "openingKills",
            "openingDeaths",
            "tradeKills",
            "tradedDeaths",
            "utilityDamage",
            "enemiesFlashed",
            "friendsFlashed",
            "enemyFlashSeconds",
            "rounds",
            "roundsFor",
            "roundsAgainst",
            "result",
        ] {
            assert_eq!(
                actual["stats"][key], golden["stats"][key],
                "core stat {key} drifted from the golden / Python report"
            );
        }
        assert_eq!(actual, golden);
    }

    fn report_fixture_path() -> std::path::PathBuf {
        std::env::var_os("OMARCS_REPORT_FIXTURE")
            .or_else(|| std::env::var_os("OMARCS_DEMO_FIXTURE"))
            .map(std::path::PathBuf::from)
            .expect("OMARCS_REPORT_FIXTURE or OMARCS_DEMO_FIXTURE")
    }
}
