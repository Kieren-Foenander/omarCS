from __future__ import annotations

import hashlib
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .mechanics import CS2_TICKS_PER_SECOND, calculate_mechanics

ANALYSIS_VERSION = 2


def value(row: dict[str, Any], *names: str, default: Any = None) -> Any:
    for name in names:
        if name in row and row[name] is not None:
            return row[name]
    return default


def string_id(raw: Any) -> str:
    if raw is None:
        return ""
    if isinstance(raw, float) and raw.is_integer():
        return str(int(raw))
    return str(raw)


def rows(frame: Any) -> list[dict[str, Any]]:
    if frame is None:
        return []
    if hasattr(frame, "to_dicts"):
        return frame.to_dicts()
    return list(frame)


def demo_checksum(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as demo_file:
        for chunk in iter(lambda: demo_file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def player_ids(demo: Any) -> dict[str, str]:
    found: dict[str, str] = {}
    for row in rows(getattr(demo, "ticks", None)) + rows(getattr(demo, "kills", None)):
        for prefix in ("", "attacker_", "victim_", "assister_", "user_"):
            steam_id = string_id(value(row, f"{prefix}steamid", f"{prefix}steam_id"))
            name = str(value(row, f"{prefix}name", default="") or "")
            if steam_id and steam_id != "0" and name:
                found[steam_id] = name
    return found


def resolve_player(demo: Any, selector: str | None) -> tuple[str, str]:
    players = player_ids(demo)
    if selector:
        selector_folded = selector.casefold()
        if selector in players:
            return selector, players[selector]
        matches = [(steam_id, name) for steam_id, name in players.items() if name.casefold() == selector_folded]
        if len(matches) == 1:
            return matches[0]
    choices = ", ".join(sorted(players.values())) or "no players detected"
    raise ValueError(f"Player {selector or 'is not configured'}; available players: {choices}")


def is_enemy_kill(row: dict[str, Any]) -> bool:
    attacker = string_id(value(row, "attacker_steamid", "attacker_steam_id"))
    victim = string_id(value(row, "victim_steamid", "victim_steam_id", "user_steamid", "user_steam_id"))
    attacker_side = str(value(row, "attacker_side", default="") or "")
    victim_side = str(value(row, "victim_side", "user_side", default="") or "")
    return bool(attacker and victim and attacker != victim and (not attacker_side or not victim_side or attacker_side != victim_side))


def trade_flags(kills: list[dict[str, Any]], tickrate: int, window_seconds: float = 5.0) -> tuple[set[int], set[int]]:
    """Return indices for traded deaths and their corresponding trade kills."""
    traded_deaths: set[int] = set()
    trade_kills: set[int] = set()
    window = max(1, round(tickrate * window_seconds))
    by_round: dict[int, list[tuple[int, dict[str, Any]]]] = defaultdict(list)
    for index, kill in enumerate(kills):
        by_round[int(value(kill, "round_num", default=0) or 0)].append((index, kill))

    for round_kills in by_round.values():
        round_kills.sort(key=lambda item: int(value(item[1], "tick", default=0) or 0))
        for position, (death_index, death) in enumerate(round_kills):
            dead_player = string_id(value(death, "victim_steamid", "victim_steam_id"))
            killer = string_id(value(death, "attacker_steamid", "attacker_steam_id"))
            dead_side = str(value(death, "victim_side", default="") or "")
            death_tick = int(value(death, "tick", default=0) or 0)
            if not dead_player or not killer:
                continue
            for trade_index, trade in round_kills[position + 1 :]:
                trade_tick = int(value(trade, "tick", default=0) or 0)
                if trade_tick - death_tick > window:
                    break
                trade_attacker = string_id(value(trade, "attacker_steamid", "attacker_steam_id"))
                trade_victim = string_id(value(trade, "victim_steamid", "victim_steam_id"))
                trade_side = str(value(trade, "attacker_side", default="") or "")
                if trade_victim == killer and trade_attacker != dead_player and (not dead_side or trade_side == dead_side):
                    traded_deaths.add(death_index)
                    trade_kills.add(trade_index)
                    break
    return traded_deaths, trade_kills


def round_sides(demo: Any, steam_id: str) -> dict[int, str]:
    counts: dict[int, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    for tick in rows(getattr(demo, "ticks", None)):
        if string_id(value(tick, "steamid", "steam_id")) != steam_id:
            continue
        round_num = int(value(tick, "round_num", default=0) or 0)
        side = str(value(tick, "side", "team_name", default="") or "")
        if round_num and side:
            counts[round_num][side] += 1
    return {round_num: max(side_counts, key=side_counts.get) for round_num, side_counts in counts.items()}


def utility_weapon(row: dict[str, Any]) -> bool:
    weapon = str(value(row, "weapon", "weapon_name", default="") or "").casefold()
    return any(token in weapon for token in ("hegrenade", "he grenade", "molotov", "incgrenade", "incendiary"))


def calculate_player_metrics(demo: Any, steam_id: str) -> dict[str, Any]:
    kill_rows = [kill for kill in rows(getattr(demo, "kills", None)) if is_enemy_kill(kill)]
    damage_rows = rows(getattr(demo, "damages", None))
    rounds_rows = rows(getattr(demo, "rounds", None))
    tickrate = CS2_TICKS_PER_SECOND
    traded_deaths, trade_kills = trade_flags(kill_rows, tickrate)

    kills = sum(string_id(value(row, "attacker_steamid", "attacker_steam_id")) == steam_id for row in kill_rows)
    deaths = sum(string_id(value(row, "victim_steamid", "victim_steam_id")) == steam_id for row in kill_rows)
    assists = sum(string_id(value(row, "assister_steamid", "assister_steam_id")) == steam_id for row in kill_rows)
    headshots = sum(
        string_id(value(row, "attacker_steamid", "attacker_steam_id")) == steam_id
        and bool(value(row, "headshot", "is_headshot", default=False))
        for row in kill_rows
    )

    round_events: dict[int, dict[str, bool]] = defaultdict(lambda: {"kill": False, "assist": False, "died": False, "traded": False})
    for index, row in enumerate(kill_rows):
        round_num = int(value(row, "round_num", default=0) or 0)
        if string_id(value(row, "attacker_steamid", "attacker_steam_id")) == steam_id:
            round_events[round_num]["kill"] = True
        if string_id(value(row, "assister_steamid", "assister_steam_id")) == steam_id:
            round_events[round_num]["assist"] = True
        if string_id(value(row, "victim_steamid", "victim_steam_id")) == steam_id:
            round_events[round_num]["died"] = True
            round_events[round_num]["traded"] = index in traded_deaths

    round_count = len(rounds_rows)
    kast_rounds = 0
    for round_num in range(1, round_count + 1):
        event = round_events[round_num]
        if event["kill"] or event["assist"] or not event["died"] or event["traded"]:
            kast_rounds += 1

    damage = 0
    utility_damage = 0
    for row in damage_rows:
        if string_id(value(row, "attacker_steamid", "attacker_steam_id")) != steam_id:
            continue
        victim_id = string_id(value(row, "victim_steamid", "victim_steam_id", "user_steamid", "user_steam_id"))
        if victim_id == steam_id:
            continue
        attacker_side = str(value(row, "attacker_side", default="") or "")
        victim_side = str(value(row, "victim_side", "user_side", default="") or "")
        if attacker_side and victim_side and attacker_side == victim_side:
            continue
        amount = max(0, int(value(row, "dmg_health_real", "damage", "dmg_health", default=0) or 0))
        damage += amount
        if utility_weapon(row):
            utility_damage += amount

    openings_for = openings_against = 0
    by_round: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in kill_rows:
        by_round[int(value(row, "round_num", default=0) or 0)].append(row)
    for round_kills in by_round.values():
        first = min(round_kills, key=lambda row: int(value(row, "tick", default=0) or 0))
        if string_id(value(first, "attacker_steamid", "attacker_steam_id")) == steam_id:
            openings_for += 1
        if string_id(value(first, "victim_steamid", "victim_steam_id")) == steam_id:
            openings_against += 1

    blinds = rows(getattr(demo, "events", {}).get("player_blind") if hasattr(demo, "events") else None)
    enemies_flashed = friends_flashed = 0
    enemy_flash_seconds = 0.0
    for row in blinds:
        if string_id(value(row, "attacker_steamid", "attacker_steam_id")) != steam_id:
            continue
        victim_id = string_id(value(row, "user_steamid", "victim_steamid", "steamid"))
        if not victim_id or victim_id == steam_id:
            continue
        attacker_side = str(value(row, "attacker_side", default="") or "")
        victim_side = str(value(row, "user_side", "victim_side", default="") or "")
        duration = float(value(row, "blind_duration", "duration", default=0.0) or 0.0)
        if attacker_side and victim_side and attacker_side == victim_side:
            friends_flashed += 1
        else:
            enemies_flashed += 1
            enemy_flash_seconds += max(0.0, duration)

    sides = round_sides(demo, steam_id)
    rounds_for = 0
    rounds_against = 0
    for round_row in rounds_rows:
        round_num = int(value(round_row, "round_num", default=0) or 0)
        winner = str(value(round_row, "winner", default="") or "")
        if winner and sides.get(round_num):
            if winner == sides[round_num]:
                rounds_for += 1
            else:
                rounds_against += 1

    if rounds_for > rounds_against:
        result = "W"
    elif rounds_for < rounds_against:
        result = "L"
    else:
        result = "D"

    rounds_denominator = max(1, round_count)
    kpr = kills / rounds_denominator
    apr = assists / rounds_denominator
    dpr = deaths / rounds_denominator
    adr = damage / rounds_denominator
    kast = 100 * kast_rounds / rounds_denominator
    impact = 2.13 * kpr + 0.42 * apr - 0.41
    rating = 0.0073 * kast + 0.3591 * kpr - 0.5329 * dpr + 0.2372 * impact + 0.0032 * adr + 0.1587

    return {
        "kills": kills,
        "deaths": deaths,
        "assists": assists,
        "kd": round(kills / max(1, deaths), 2),
        "adr": round(adr, 1),
        "kast": round(kast, 1),
        "rating": round(rating, 2),
        "headshotPercent": round(100 * headshots / max(1, kills), 1),
        "openingKills": openings_for,
        "openingDeaths": openings_against,
        "tradeKills": sum(index in trade_kills and string_id(value(row, "attacker_steamid", "attacker_steam_id")) == steam_id for index, row in enumerate(kill_rows)),
        "tradedDeaths": sum(index in traded_deaths and string_id(value(row, "victim_steamid", "victim_steam_id")) == steam_id for index, row in enumerate(kill_rows)),
        "utilityDamage": utility_damage,
        "enemiesFlashed": enemies_flashed,
        "friendsFlashed": friends_flashed,
        "enemyFlashSeconds": round(enemy_flash_seconds, 1),
        "rounds": round_count,
        "roundsFor": rounds_for,
        "roundsAgainst": rounds_against,
        "result": result,
        **calculate_mechanics(demo, steam_id),
    }


def coaching_insights(stats: dict[str, Any]) -> list[str]:
    insights: list[str] = []
    if stats.get("mechanicsEngagements", 0) >= 3 and (stats.get("crosshairPlacement") or 0) > 10:
        insights.append(
            f"Crosshair correction averaged {stats['crosshairPlacement']:.1f}°; pre-aim closer to likely head positions."
        )
    if stats.get("mechanicsEngagements", 0) >= 3 and (stats.get("timeToDamageMs") or 0) > 650:
        insights.append(
            f"Time to damage was {stats['timeToDamageMs']:.0f} ms; review whether placement or first-shot accuracy delayed fights."
        )
    if stats.get("counterStrafeShots", 0) >= 5 and (stats.get("counterStrafePercent") or 100) < 70:
        insights.append(
            f"Only {stats['counterStrafePercent']:.0f}% of rifle shots were fully settled; finish the counter-strafe before firing."
        )
    if stats["openingDeaths"] > stats["openingKills"]:
        insights.append("Opening duels cost more rounds than they created; review your first-contact fights.")
    if stats["friendsFlashed"] > 1:
        insights.append(f"You flashed teammates {stats['friendsFlashed']} times; tighten flash timing and calls.")
    if stats["utilityDamage"] < max(10, stats["rounds"] * 2):
        insights.append("Utility damage was quiet; look for earlier HE and molotov value.")
    if stats["tradedDeaths"] < max(1, stats["deaths"] // 3) and stats["deaths"] >= 6:
        insights.append("Few deaths were traded; check spacing and whether teammates could follow your fights.")
    if stats["adr"] >= 90:
        insights.append("High-impact damage game—your ADR was above 90.")
    return insights[:3] or ["No obvious outlier this match; compare it with your next few games."]


def analyze_demo(path: Path, player_selector: str, checksum: str | None = None) -> dict[str, Any]:
    from awpy import Demo

    demo = Demo(path)
    events = list(demo.default_events)
    if "player_blind" not in events:
        events.append("player_blind")
    demo.parse(
        events=events,
        player_props=[
            "pitch",
            "yaw",
            "duck_amount",
            "velocity",
            "approximate_spotted_by",
            "active_weapon_name",
            "shots_fired",
        ],
    )
    steam_id, player_name = resolve_player(demo, player_selector)
    stats = calculate_player_metrics(demo, steam_id)
    digest = checksum or demo_checksum(path)
    played_at = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc).isoformat()
    return {
        "analysisVersion": ANALYSIS_VERSION,
        "id": digest[:16],
        "checksum": digest,
        "path": str(path.resolve()),
        "playedAt": played_at,
        "map": str(getattr(demo, "header", {}).get("map_name") or "Unknown map"),
        "player": {"steamId": steam_id, "name": player_name},
        "stats": stats,
        "insights": coaching_insights(stats),
    }
