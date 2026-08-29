from omarcs.storage import Store


def sample_match(match_id: str, played_at: str, result: str, rating: float) -> dict:
    return {
        "id": match_id,
        "checksum": match_id * 4,
        "path": f"/{match_id}.dem",
        "playedAt": played_at,
        "map": "de_mirage",
        "player": {"steamId": "76561198000000001", "name": "Kieren"},
        "stats": {
            "result": result, "roundsFor": 13, "roundsAgainst": 9, "rating": rating,
            "adr": 80.0, "kast": 75.0, "kd": 1.2,
        },
        "insights": ["Test note"],
    }


def test_publishes_latest_and_trends(tmp_path) -> None:
    store = Store(tmp_path)
    store.save_match(sample_match("a", "2026-01-01T00:00:00+00:00", "L", 0.8))
    store.save_match(sample_match("b", "2026-01-02T00:00:00+00:00", "W", 1.2))
    summary = store.publish()
    store.close()
    assert summary["latest"]["id"] == "b"
    assert summary["trends"]["matches"] == 2
    assert summary["trends"]["wins"] == 1
    assert summary["trends"]["rating"] == 1.0
    assert (tmp_path / "summary.json").exists()


def test_reanalyzes_matches_when_analysis_version_changes(tmp_path) -> None:
    store = Store(tmp_path)
    match = sample_match("a", "2026-01-01T00:00:00+00:00", "W", 1.1)
    store.save_match(match)
    assert store.has_checksum(match["checksum"], 1)
    assert not store.has_checksum(match["checksum"], 2)
    match["analysisVersion"] = 2
    store.save_match(match)
    assert store.has_checksum(match["checksum"], 2)
    store.close()
