from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

ANALYSIS_VERSION = 4


def demo_checksum(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as demo_file:
        for chunk in iter(lambda: demo_file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def analyze_demo(
    path: Path, player_selector: str, checksum: str | None = None
) -> dict[str, Any]:
    from .native import generate_report

    report = generate_report(path, player_selector)
    if checksum and report.get("checksum") != checksum:
        raise RuntimeError("native Match Report checksum did not match the Demo digest")
    return report
