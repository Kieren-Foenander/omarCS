from pathlib import Path

import pytest

from omarcs import autofetch
from omarcs.autofetch import AutoFetcher, GameState, extract_replay_urls


def test_extracts_unique_replay_urls_in_order() -> None:
    first = b"http://replay423.valve.net/730/one.dem.bz2"
    second = b"http://replay171.valve.net/730/two.dem.bz2"
    assert extract_replay_urls(b"x" + first + b"\x00" + second + b"\x00" + first) == [
        first.decode(),
        second.decode(),
    ]


def test_game_state_blocks_heavy_work_during_a_match() -> None:
    state = GameState()
    state.process_checked_at = 10**12
    assert state.heavy_work_allowed()
    state.process_running = True
    state.update({"map": {"phase": "live"}}, now=1.0)
    assert not state.heavy_work_allowed()
    state.update({"map": {"phase": "gameover"}}, now=2.0)
    assert state.heavy_work_allowed()
    assert state.snapshot()[2] == 2.0


class AlwaysAllowed:
    def heavy_work_allowed(self) -> bool:
        return True


def test_process_queue_deletes_successfully_imported_downloads(tmp_path, monkeypatch) -> None:
    url = "https://replay423.valve.net/730/match.dem.bz2"
    compressed = tmp_path / "match.dem.bz2"
    demo = tmp_path / "match.dem"
    compressed.write_bytes(b"compressed")
    demo.write_bytes(b"demo")
    monkeypatch.setattr(autofetch, "demos_root", lambda: tmp_path)
    monkeypatch.setattr(autofetch, "load_state", lambda: {"knownUrls": [], "queue": [url]})
    monkeypatch.setattr(autofetch, "save_state", lambda state: None)
    monkeypatch.setattr(autofetch, "parse_demo", lambda path, allowed: None)

    fetcher = AutoFetcher(AlwaysAllowed())
    fetcher.process_queue()

    assert not compressed.exists()
    assert not demo.exists()
    assert fetcher.state["queue"] == []


def test_process_queue_retains_downloads_when_import_fails(tmp_path, monkeypatch) -> None:
    url = "https://replay423.valve.net/730/match.dem.bz2"
    compressed = tmp_path / "match.dem.bz2"
    demo = tmp_path / "match.dem"
    compressed.write_bytes(b"compressed")
    demo.write_bytes(b"demo")
    monkeypatch.setattr(autofetch, "demos_root", lambda: tmp_path)
    monkeypatch.setattr(autofetch, "load_state", lambda: {"knownUrls": [], "queue": [url]})
    monkeypatch.setattr(autofetch, "save_state", lambda state: None)

    def fail_parse(path: Path, allowed) -> None:
        raise RuntimeError("parse failed")

    monkeypatch.setattr(autofetch, "parse_demo", fail_parse)
    fetcher = AutoFetcher(AlwaysAllowed())

    with pytest.raises(RuntimeError, match="parse failed"):
        fetcher.process_queue()

    assert compressed.exists()
    assert demo.exists()
    assert fetcher.state["queue"] == [url]


def test_install_helper_is_idempotent_when_the_bundle_is_present(tmp_path, monkeypatch) -> None:
    for name in ("boiler-writter", "libsteam_api.so", "steam_appid.txt"):
        (tmp_path / name).write_bytes(b"installed")
    monkeypatch.setattr(autofetch, "helper_root", lambda: tmp_path)

    def unexpected_download(*args, **kwargs):
        raise AssertionError("the helper bundle should not download again")

    monkeypatch.setattr(autofetch.urllib.request, "urlopen", unexpected_download)
    autofetch.install_helper()
