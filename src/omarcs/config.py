from __future__ import annotations

import os
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Settings:
    player: str | None
    import_paths: tuple[Path, ...]
    keep_recent: int = 20


def config_home() -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))


def state_home() -> Path:
    return Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))


def data_home() -> Path:
    return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))


def config_path() -> Path:
    return config_home() / "omarcs/config.toml"


def default_import_paths() -> tuple[Path, ...]:
    return (
        data_home() / "omarcs/demos",
        Path.home() / "Downloads",
        data_home() / "Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo",
    )


def load_settings(path: Path | None = None) -> Settings:
    path = path or config_path()
    raw: dict = {}
    if path.exists():
        with path.open("rb") as config_file:
            raw = tomllib.load(config_file)

    player = raw.get("player", {}).get("steam_id") or raw.get("player", {}).get("name")
    configured_paths = raw.get("import", {}).get("paths")
    paths = default_import_paths() if not configured_paths else tuple(
        Path(os.path.expandvars(os.path.expanduser(str(item)))) for item in configured_paths
    )
    keep_recent = max(1, min(100, int(raw.get("history", {}).get("keep_recent", 20))))
    return Settings(player=str(player) if player else None, import_paths=paths, keep_recent=keep_recent)


def steam_loginusers_paths() -> tuple[Path, ...]:
    return (
        data_home() / "Steam/config/loginusers.vdf",
        Path.home() / ".steam/steam/config/loginusers.vdf",
    )


def detect_active_steam_id(paths: tuple[Path, ...] | None = None) -> str | None:
    """Return the most-recent local SteamID64 without exposing login credentials."""
    for path in paths or steam_loginusers_paths():
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        blocks = re.finditer(r'"(7656\d{13})"\s*\{(.*?)\n\s*\}', text, re.DOTALL)
        fallback: str | None = None
        for block in blocks:
            steam_id, body = block.group(1), block.group(2)
            fallback = fallback or steam_id
            if re.search(r'"MostRecent"\s+"1"', body, re.IGNORECASE):
                return steam_id
        if fallback:
            return fallback
    return None

