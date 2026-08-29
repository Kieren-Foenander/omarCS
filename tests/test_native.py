import hashlib
import json
from pathlib import Path

import pytest

from omarcs import native
from omarcs.analysis import analyze_demo
from omarcs.cli import import_paths
from omarcs.storage import Store

DEMO_BYTES = b"demo"
DEMO_CHECKSUM = hashlib.sha256(DEMO_BYTES).hexdigest()

SAMPLE_REPORT = {
    "analysisVersion": 4,
    "id": DEMO_CHECKSUM[:16],
    "checksum": DEMO_CHECKSUM,
    "path": "/tmp/match.dem",
    "playedAt": "2026-01-02T03:04:05+00:00",
    "map": "de_inferno",
    "player": {"steamId": "76561198000000001", "name": "Kieren"},
    "stats": {
        "result": "W",
        "roundsFor": 13,
        "roundsAgainst": 9,
        "rating": 1.2,
        "adr": 88.0,
        "kast": 75.0,
        "kd": 1.4,
    },
    "sprays": [],
    "insights": ["Native report"],
}


def write_native(path: Path, payload: object | None = None, *, exit_code: int = 0) -> Path:
    if exit_code:
        script = (
            "#!/usr/bin/env python3\n"
            "import sys\n"
            "print('player was not found', file=sys.stderr)\n"
            f"sys.exit({exit_code})\n"
        )
    else:
        report = SAMPLE_REPORT if payload is None else payload
        argv_path = path.with_name(path.name + ".argv")
        script = (
            "#!/usr/bin/env python3\n"
            "import json, sys\n"
            f"open({json.dumps(str(argv_path))}, 'w', encoding='utf-8').write(json.dumps(sys.argv[1:]))\n"
            f"print({json.dumps(json.dumps(report))})\n"
        )
    path.write_text(script, encoding="utf-8")
    path.chmod(0o755)
    return path


def test_generate_report_runs_native_report_command(tmp_path: Path, monkeypatch) -> None:
    binary = write_native(tmp_path / "omarcs-native")
    monkeypatch.setenv("OMARCS_NATIVE", str(binary))
    demo = tmp_path / "match.dem"
    demo.write_bytes(DEMO_BYTES)

    report = native.generate_report(demo, "76561198000000001")

    assert json.loads((binary.with_name(binary.name + ".argv")).read_text(encoding="utf-8")) == [
        "report",
        str(demo),
        "76561198000000001",
    ]
    assert report["map"] == "de_inferno"
    assert report["player"]["steamId"] == "76561198000000001"
    assert report["stats"]["result"] == "W"


def test_generate_report_surfaces_native_errors(tmp_path: Path, monkeypatch) -> None:
    binary = write_native(tmp_path / "omarcs-native", exit_code=2)
    monkeypatch.setenv("OMARCS_NATIVE", str(binary))

    with pytest.raises(RuntimeError, match="player was not found"):
        native.generate_report(tmp_path / "match.dem", "missing")


def test_missing_native_override_is_an_error(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("OMARCS_NATIVE", str(tmp_path / "missing"))

    with pytest.raises(RuntimeError, match="OMARCS_NATIVE is not an executable file"):
        native.resolve_native_binary()


def test_analyze_demo_uses_native_match_report(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("OMARCS_NATIVE", str(write_native(tmp_path / "omarcs-native")))
    demo = tmp_path / "match.dem"
    demo.write_bytes(DEMO_BYTES)

    report = analyze_demo(demo, "Kieren")

    assert report["insights"] == ["Native report"]
    assert report["analysisVersion"] == 4


def test_analyze_demo_rejects_checksum_mismatch(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("OMARCS_NATIVE", str(write_native(tmp_path / "omarcs-native")))

    with pytest.raises(RuntimeError, match="checksum did not match"):
        analyze_demo(tmp_path / "match.dem", "Kieren", checksum="0" * 64)


def test_import_persists_native_match_report(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("OMARCS_NATIVE", str(write_native(tmp_path / "omarcs-native")))
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "config"))
    demo = tmp_path / "match.dem"
    demo.write_bytes(DEMO_BYTES)

    assert import_paths([demo], "76561198000000001", quiet=True) == 0

    with Store() as store:
        matches = store.matches()
    assert len(matches) == 1
    assert matches[0]["map"] == "de_inferno"
    assert matches[0]["insights"] == ["Native report"]


def test_ensure_native_binary_installs_a_stable_copy(tmp_path: Path, monkeypatch) -> None:
    source = write_native(tmp_path / "omarcs-native")
    monkeypatch.setenv("OMARCS_NATIVE", str(source))
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path / "data"))

    installed = native.ensure_native_binary()

    assert installed == tmp_path / "data/omarcs/omarcs-native"
    assert installed.is_file()
    assert installed.stat().st_mode & 0o111
