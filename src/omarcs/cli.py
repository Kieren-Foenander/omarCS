from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

from .config import detect_active_steam_id, load_settings
from .storage import Store


def demo_files(paths: list[Path]) -> list[Path]:
    found: set[Path] = set()
    for path in paths:
        path = path.expanduser()
        if path.is_file() and path.suffix.casefold() == ".dem":
            found.add(path.resolve())
        elif path.is_dir():
            found.update(item.resolve() for item in path.rglob("*.dem") if item.is_file())
    return sorted(found, key=lambda item: item.stat().st_mtime)


def import_paths(paths: list[Path], player: str | None, quiet: bool = False) -> int:
    from .analysis import ANALYSIS_VERSION, analyze_demo, demo_checksum

    settings = load_settings()
    selector = player or settings.player or detect_active_steam_id()
    with Store() as store:
        files = demo_files(paths)
        if not files:
            store.publish(settings.keep_recent)
            if not quiet:
                print("No .dem files found.")
            return 0
        if not selector:
            store.write_status("error", "Could not detect your Steam account. Pass --player STEAMID64 or player name.")
            print("Could not detect your Steam account. Pass --player STEAMID64 or player name.", file=sys.stderr)
            return 2

        store.write_status("analyzing", f"Checking {len(files)} demo{'s' if len(files) != 1 else ''}…")
        imported = 0
        failures: list[str] = []
        for path in files:
            try:
                checksum = demo_checksum(path)
                if store.has_checksum(checksum, ANALYSIS_VERSION):
                    continue
                if not quiet:
                    print(f"Analyzing {path.name}…")
                match = analyze_demo(path, selector, checksum)
                store.save_match(match)
                imported += 1
            except BaseException as error:  # Rust parser panics surface as BaseException through pyo3.
                if isinstance(error, (KeyboardInterrupt, SystemExit)):
                    raise
                failures.append(f"{path.name}: {error}")

        if failures and imported == 0 and not store.matches(1):
            message = failures[0]
            store.write_status("error", message)
        else:
            store.publish(settings.keep_recent)

    if not quiet:
        print(f"Imported {imported} new match{'es' if imported != 1 else ''}.")
        for failure in failures:
            print(f"Warning: {failure}", file=sys.stderr)
    return 1 if failures and imported == 0 else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="omarcs", description="Local CS2 match analysis for Omarchy")
    subcommands = parser.add_subparsers(dest="command", required=True)

    import_command = subcommands.add_parser("import", help="Import one demo or a directory of demos")
    import_command.add_argument("paths", nargs="+", type=Path)
    import_command.add_argument("--player", help="SteamID64 or exact player name")
    import_command.add_argument("--quiet", action="store_true")

    refresh = subcommands.add_parser("refresh", help="Scan configured demo directories")
    refresh.add_argument("--player", help="SteamID64 or exact player name")
    refresh.add_argument("--quiet", action="store_true")

    status = subcommands.add_parser("status", help="Print the current dashboard JSON")
    status.add_argument("--pretty", action="store_true")

    subcommands.add_parser("setup-auto", help="Install and enable automatic Valve demo fetching")
    subcommands.add_parser("auto-run", help=argparse.SUPPRESS)
    auto_status = subcommands.add_parser("auto-status", help="Print automatic fetcher state")
    auto_status.add_argument("--pretty", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "import":
        return import_paths(args.paths, args.player, args.quiet)
    if args.command == "refresh":
        settings = load_settings()
        return import_paths(list(settings.import_paths), args.player, args.quiet)
    if args.command == "status":
        with Store() as store:
            summary = store.current_summary()
        print(json.dumps(summary, indent=2 if args.pretty else None))
        return 0
    if args.command == "setup-auto":
        from .autofetch import setup_auto

        setup_auto()
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=True)
        subprocess.run(["systemctl", "--user", "enable", "--now", "omarcs-autofetch.service"], check=True)
        print("Automatic match fetching is enabled.")
        return 0
    if args.command == "auto-run":
        from .autofetch import run_daemon

        return run_daemon()
    if args.command == "auto-status":
        from .autofetch import load_state

        print(json.dumps(load_state(), indent=2 if args.pretty else None))
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
