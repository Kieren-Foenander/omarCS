#!/usr/bin/env python3
"""Regenerate CS2 item maps from a local GameTracking-CS2 tree.

Plugin install never runs this helper. The generated maps are already
vendored. To refresh them, put a GameTracking-CS2 directory beside this
crate and run this file; it only builds the local crate.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
TRACKING = ROOT / "GameTracking-CS2"


def main() -> int:
    if not TRACKING.is_dir():
        print(
            "A local GameTracking-CS2 directory is required beside this crate.",
            file=sys.stderr,
        )
        return 1
    subprocess.run(["cargo", "run", "--release"], cwd=ROOT, check=True)
    print("Cargo run completed successfully.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
