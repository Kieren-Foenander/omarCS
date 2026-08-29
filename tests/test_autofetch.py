import bz2
from io import BytesIO
from pathlib import Path

import pytest

from omarcs import autofetch
from omarcs.autofetch import AutoFetcher, GameState, extract_replay_urls


def test_extracts_unique_replay_urls_in_order() -> None:
    first = b"https://replay423.valve.net/730/one.dem.bz2"
    second = b"https://replay171.valve.net/730/two.dem.bz2"
    insecure = b"http://replay999.valve.net/730/unsafe.dem.bz2"
    assert extract_replay_urls(
        b"x" + insecure + b"\x00" + first + b"\x00" + second + b"\x00" + first
    ) == [
        first.decode(),
        second.decode(),
    ]


@pytest.mark.parametrize(
    "url",
    [
        "http://replay423.valve.net/730/match.dem.bz2",
        "https://replay423.valve.net.evil.test/730/match.dem.bz2",
        "https://replay423.valve.net/731/match.dem.bz2",
        "https://replay423.valve.net/730/../match.dem.bz2",
    ],
)
def test_download_rejects_untrusted_replay_urls(url: str, tmp_path, monkeypatch) -> None:
    monkeypatch.setattr(autofetch, "demos_root", lambda: tmp_path)
    with pytest.raises(ValueError, match="trusted Valve HTTPS replay URL"):
        autofetch.download_demo(url, lambda: True)


class ReplayResponse(BytesIO):
    status = 200

    def __init__(self, payload: bytes, url: str) -> None:
        super().__init__(payload)
        self.url = url

    def geturl(self) -> str:
        return self.url

    def __enter__(self):
        return self

    def __exit__(self, *args) -> None:
        self.close()


def test_download_rejects_redirect_outside_valve(tmp_path, monkeypatch) -> None:
    url = "https://replay423.valve.net/730/match.dem.bz2"
    response = ReplayResponse(bz2.compress(b"demo"), "https://example.test/match.dem.bz2")
    monkeypatch.setattr(autofetch, "demos_root", lambda: tmp_path)
    monkeypatch.setattr(autofetch.urllib.request, "urlopen", lambda *args, **kwargs: response)
    with pytest.raises(RuntimeError, match="redirected outside"):
        autofetch.download_demo(url, lambda: True)


def test_decompression_enforces_an_expansion_limit(tmp_path, monkeypatch) -> None:
    compressed = tmp_path / "match.dem.bz2"
    compressed.write_bytes(bz2.compress(b"demo contents"))
    monkeypatch.setattr(autofetch, "MAX_DEMO_BYTES", 4)
    with pytest.raises(RuntimeError, match="decompressed size limit"):
        autofetch.decompress_demo(compressed, lambda: True)
    assert not (tmp_path / "match.dem.part").exists()


def test_decompression_rejects_non_cs2_content(tmp_path) -> None:
    compressed = tmp_path / "match.dem.bz2"
    compressed.write_bytes(bz2.compress(b"not a CS2 demo"))
    with pytest.raises(RuntimeError, match="CS2 demo header"):
        autofetch.decompress_demo(compressed, lambda: True)
    assert not (tmp_path / "match.dem.part").exists()


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
