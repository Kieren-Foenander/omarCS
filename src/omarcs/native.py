from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any

from .config import data_home


def plugin_root() -> Path:
    return Path(__file__).resolve().parents[2]


def installed_binary() -> Path:
    return data_home() / "omarcs/omarcs-native"


def resolve_native_binary() -> Path:
    override = os.environ.get("OMARCS_NATIVE")
    if override:
        path = Path(override)
        if path.is_file() and os.access(path, os.X_OK):
            return path
        raise RuntimeError(f"OMARCS_NATIVE is not an executable file: {path}")

    located = shutil.which("omarcs-native")
    if located:
        return Path(located)

    for candidate in (installed_binary(), plugin_root() / "target/release/omarcs-native"):
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate

    raise RuntimeError(
        "Could not find omarcs-native. Build it with "
        "`cargo build --release -p omarcs-native` or set OMARCS_NATIVE."
    )


def generate_report(demo: Path, player: str) -> dict[str, Any]:
    binary = resolve_native_binary()
    result = subprocess.run(
        [str(binary), "report", str(demo), player],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip().splitlines()
        raise RuntimeError(detail[-1] if detail else f"omarcs-native exited {result.returncode}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"omarcs-native returned invalid JSON: {error}") from error
    if not isinstance(payload, dict) or "stats" not in payload or "player" not in payload:
        raise RuntimeError("omarcs-native report was missing Match Report fields")
    return payload


def ensure_native_binary() -> Path:
    try:
        found = resolve_native_binary()
    except RuntimeError:
        found = _build_native_binary()
    installed = installed_binary()
    if found.resolve() != installed.resolve():
        installed.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(found, installed)
        installed.chmod(0o755)
        return installed
    return found


def _build_native_binary() -> Path:
    root = plugin_root()
    if not (root / "Cargo.toml").is_file():
        raise RuntimeError("Could not find omarcs-native or the Rust workspace to build it")
    cargo = shutil.which("cargo")
    if not cargo:
        raise RuntimeError("cargo is required to build omarcs-native")
    result = subprocess.run(
        [cargo, "build", "--release", "-p", "omarcs-native"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip().splitlines()
        raise RuntimeError(detail[-1] if detail else "cargo build failed")
    built = root / "target/release/omarcs-native"
    if not built.is_file():
        raise RuntimeError("cargo build did not produce omarcs-native")
    return built
