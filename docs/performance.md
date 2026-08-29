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
- Native parse plus core player statistics: approximately 0.13 seconds wall time on the same fixture before tick compaction; compacting observations is now part of the Match Facts path

These wall-clock figures describe the development machine and are not portable CI assertions. Regression gates should compare revisions on the same machine and fixture.

## Parity gates

1. The vendored parser must retain the upstream fixture checksum before its output construction is changed.
2. Existing Python semantic fixtures must pass against each ported Match Facts calculation.
3. At least one Awpy-compatible real Demo must produce a normalized golden Match Report before the launcher changes to Rust.
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
Map-geometry visibility is still future work; without a mesh the native path
uses spotted-by observations the same way Python does.

The compact adapter reads the parser dataframe once, then drops spectators,
steamid 0, and ticks outside reconstructed rounds. Weapons are interned and
spotted-by lists are flattened. In-parser accumulation is still future work.

Set `OMARCS_DEMO_FIXTURE` to a compatible local Demo when running real-demo parity tests. Large Demo files are deliberately not committed to this repository.

Run the native real-demo normalization gate with:

```bash
OMARCS_DEMO_FIXTURE=/path/to/test_demo.dem \
  cargo test -p omarcs-native upstream_fixture_normalizes_expected_match_facts -- --ignored
```
