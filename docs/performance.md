# Performance contract

omarCS optimizes elapsed time from discovering a new Demo to publishing its Match Report. Parser throughput, geometry time, peak memory, unchanged-refresh time, and binary/bootstrap size are measured separately so one improvement cannot hide a regression elsewhere.

## Parser reference

- Upstream: `LaihoE/demoparser`
- Revision: `57f24c76776ac176e893833f3a5b4aad718a8196`
- Fixture: upstream `src/parser/test_demo.dem` (60,601,900 bytes)
- Fixture parser checksum: `dd96a2cc68cd6886`
- Local reference, requested properties plus all events: 0.126 seconds median, multithreaded release build
- Local `source2-demo` 0.5.8 comparison: 0.368 seconds median with its `unsafe` feature, entity state, all events, and per-tick player scans
- Vendored omarCS-focused parse: approximately 0.108 seconds median for 579,700 tick rows and 3,279 requested events when only special parser columns were collected
- The same parse with friendly player properties resolved (health, team, spotted-by, weapon, duck, shots): approximately 0.14 seconds, then compacted to 565,963 tick observations
- Match Facts now accumulate those compact observations during the second pass instead of building the generic dataframe and copying it afterwards. `omarcs probe` still materialises the dataframe so the fixture checksum can be compared independently

These wall-clock figures describe the development machine and are not portable CI assertions. Regression gates should compare revisions on the same machine and fixture.

## Parity gates

1. The vendored parser must retain the upstream fixture checksum before its output construction is changed.
2. Rust semantic fixtures must cover each Match Facts calculation that used to live in Python.
3. At least one real Demo must produce a normalized golden Match Report. That golden exists on the geometry path, and `omarcs import` calls the report implementation in-process.
4. The QML-facing Dashboard Summary schema must remain compatible throughout migration.

Core player statistics now have a Rust parity fixture covering K/D/A, ADR,
KAST, rating, headshots, openings, trades, utility, flashes, and score. Round
sides come from compact tick observations assigned to reconstructed rounds;
event-derived sides remain only as a fallback when observations are absent.

Engagement metrics now have a Rust parity fixture covering exposures,
crosshair correction, first-shot time, time-to-damage, spotted accuracy, and
counter-strafing on the radar-beta path. Sprays now have a Rust parity fixture
covering numbered target-relative AK bursts on that same spotted-by path.
Coaching notes now have a Rust parity fixture covering the bounded insight
list from core statistics and radar-beta Engagement metrics.
A Match Report has a Rust parity fixture covering the QML JSON
seam: analysis version, checksum identity, merged stats and Engagement
metrics, Sprays, and insights. SteamID64 is a string so JavaScript keeps
the full value. A real Premier Demo now has a committed golden Match Report
for the Rust geometry path. Core player statistics, Engagement counts,
Sprays, and coaching notes on that Demo match the former Python/Awpy report.
Rust mesh loading keeps VRF Source-inch vertices instead of applying the GLB
metres / Y-up node matrix, so line-of-sight uses the same space as demo ticks.
Crosshair medians can differ by a fraction of a degree because Rust uses
tick observations at the damage tick rather than Awpy's attached attacker
angles.
`omarcs import`, refresh, and bootstrap persist that Match Report through the
Rust store and Dashboard Summary module. The launcher copies `omarcs` into the
local data directory, building it with cargo when it is missing or outdated.
Map-geometry visibility is on the Rust path: FOV
plus ray-mesh tests against cached CS2 physics GLBs, with
`mechanicsQuality: "geometry"` when a mesh loads. Without a mesh the Rust path
uses spotted-by observations. The application no longer requires Python.

The compact adapter still exists as a fallback when a parse materialises the generic dataframe. Match Facts now accumulate compact player ticks during the second pass: the parser writes interned weapons and flattened spotted-by lists instead of Option-filled columns, then round assignment drops spectators, steamid 0, and ticks outside reconstructed rounds. `omarcs probe` still uses the generic dataframe so the upstream fixture checksum can be compared independently of Match Facts output construction.

Set `OMARCS_DEMO_FIXTURE` to a compatible local Demo when running real-demo parity tests. Large Demo files are deliberately not committed to this repository.

Run the real-Demo normalization gate with:

```bash
OMARCS_DEMO_FIXTURE=/path/to/test_demo.dem \
  cargo test -p omarcs upstream_fixture_normalizes_expected_match_facts -- --ignored
```

Run the golden Match Report gate against the committed checksum
`003913afc3a746a4c5c85e60b15922723a1b95286794af3d3adc946ddd07738a` (a local
de_inferno Premier Demo, player SteamID64 `76561198959939965`):

```bash
OMARCS_REPORT_FIXTURE=/path/to/match.dem \
  cargo test -p omarcs real_demo_matches_golden_match_report -- --ignored
```
