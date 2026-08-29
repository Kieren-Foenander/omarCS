from __future__ import annotations

import json
import os
import sqlite3
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .config import state_home


class Store:
    def __init__(self, root: Path | None = None) -> None:
        self.root = root or state_home() / "omarcs"
        self.root.mkdir(parents=True, exist_ok=True)
        self.database_path = self.root / "omarcs.db"
        self.summary_path = self.root / "summary.json"
        self.connection = sqlite3.connect(self.database_path)
        self.connection.row_factory = sqlite3.Row
        self.connection.execute(
            """
            CREATE TABLE IF NOT EXISTS matches (
                id TEXT PRIMARY KEY,
                checksum TEXT NOT NULL UNIQUE,
                played_at TEXT NOT NULL,
                map TEXT NOT NULL,
                player_steam_id TEXT NOT NULL,
                player_name TEXT NOT NULL,
                result TEXT NOT NULL,
                rounds_for INTEGER NOT NULL,
                rounds_against INTEGER NOT NULL,
                rating REAL NOT NULL,
                adr REAL NOT NULL,
                kast REAL NOT NULL,
                kd REAL NOT NULL,
                payload TEXT NOT NULL
            )
            """
        )
        self.connection.commit()

    def close(self) -> None:
        self.connection.close()

    def has_checksum(self, checksum: str, analysis_version: int = 1) -> bool:
        row = self.connection.execute(
            "SELECT payload FROM matches WHERE checksum = ?", (checksum,)
        ).fetchone()
        if row is None:
            return False
        try:
            payload = json.loads(row["payload"])
            return int(payload.get("analysisVersion", 1)) >= analysis_version
        except (TypeError, ValueError, json.JSONDecodeError):
            return False

    def save_match(self, match: dict[str, Any]) -> None:
        stats = match["stats"]
        self.connection.execute(
            """
            INSERT INTO matches (
                id, checksum, played_at, map, player_steam_id, player_name,
                result, rounds_for, rounds_against, rating, adr, kast, kd, payload
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(checksum) DO UPDATE SET payload = excluded.payload
            """,
            (
                match["id"],
                match["checksum"],
                match["playedAt"],
                match["map"],
                match["player"]["steamId"],
                match["player"]["name"],
                stats["result"],
                stats["roundsFor"],
                stats["roundsAgainst"],
                stats["rating"],
                stats["adr"],
                stats["kast"],
                stats["kd"],
                json.dumps(match, separators=(",", ":")),
            ),
        )
        self.connection.commit()

    def matches(self, limit: int = 20) -> list[dict[str, Any]]:
        records = self.connection.execute(
            "SELECT payload FROM matches ORDER BY played_at DESC LIMIT ?", (limit,)
        ).fetchall()
        return [json.loads(record["payload"]) for record in records]

    def write_status(self, status: str, message: str = "") -> dict[str, Any]:
        summary = self.build_summary()
        summary["status"] = status
        summary["message"] = message
        self._atomic_json(summary)
        return summary

    def publish(self, limit: int = 20) -> dict[str, Any]:
        summary = self.build_summary(limit)
        self._atomic_json(summary)
        return summary

    def current_summary(self) -> dict[str, Any]:
        if self.summary_path.exists():
            try:
                with self.summary_path.open(encoding="utf-8") as summary_file:
                    return json.load(summary_file)
            except (OSError, json.JSONDecodeError):
                pass
        return self.publish()

    def build_summary(self, limit: int = 20) -> dict[str, Any]:
        from .spray import aggregate_sprays

        matches = self.matches(limit)
        recent = matches[:5]
        trend_matches = matches[:10]
        trends: dict[str, Any] = {
            "matches": len(trend_matches),
            "wins": 0,
            "rating": 0,
            "adr": 0,
            "kast": 0,
        }
        if trend_matches:
            trends["wins"] = sum(
                match["stats"]["result"] == "W" for match in trend_matches
            )
            for key in ("rating", "adr", "kast"):
                trends[key] = round(
                    sum(float(match["stats"][key]) for match in trend_matches)
                    / len(trend_matches),
                    2 if key == "rating" else 1,
                )
        return {
            "schemaVersion": 2,
            "generatedAt": datetime.now(timezone.utc).isoformat(),
            "status": "ready" if matches else "empty",
            "message": "" if matches else "Import a CS2 demo to get started.",
            "player": matches[0]["player"] if matches else None,
            "latest": matches[0] if matches else None,
            "recent": recent,
            "trends": trends,
            "sprayControl": aggregate_sprays(trend_matches),
        }

    def _atomic_json(self, payload: dict[str, Any]) -> None:
        handle, temp_name = tempfile.mkstemp(
            prefix="summary.", suffix=".json", dir=self.root
        )
        try:
            with os.fdopen(handle, "w", encoding="utf-8") as output:
                json.dump(payload, output, indent=2)
                output.write("\n")
            os.replace(temp_name, self.summary_path)
        finally:
            if os.path.exists(temp_name):
                os.unlink(temp_name)
