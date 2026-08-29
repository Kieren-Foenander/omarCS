from __future__ import annotations

import math
import statistics
from collections import defaultdict
from typing import Any

import polars as pl

from .geometry import load_map_mesh, visible_rows

CS2_TICKS_PER_SECOND = 64
ENGAGEMENT_TICKS = CS2_TICKS_PER_SECOND

RIFLE_MAX_SPEED = {
    "ak47": 215,
    "aug": 220,
    "famas": 220,
    "galilar": 215,
    "m4a1": 225,
    "m4a1_silencer": 225,
    "sg556": 210,
}

NON_GUN_TOKENS = (
    "bayonet",
    "c4",
    "decoy",
    "flashbang",
    "hegrenade",
    "incgrenade",
    "inferno",
    "knife",
    "molotov",
    "smokegrenade",
    "taser",
)


def normalized_weapon(raw: Any) -> str:
    return str(raw or "").casefold().removeprefix("weapon_")


def is_gun(raw: Any) -> bool:
    weapon = normalized_weapon(raw)
    return bool(weapon) and not any(token in weapon for token in NON_GUN_TOKENS)


def player_id(raw: Any) -> str:
    if raw is None:
        return ""
    if isinstance(raw, float) and raw.is_integer():
        return str(int(raw))
    return str(raw)


def angle_delta(first: float, second: float) -> float:
    return (second - first + 180.0) % 360.0 - 180.0


def median(values: list[float], digits: int = 1) -> float | None:
    finite = [value for value in values if math.isfinite(value)]
    return round(statistics.median(finite), digits) if finite else None


def empty_metrics() -> dict[str, Any]:
    return {
        "crosshairPlacement": None,
        "horizontalAdjustment": None,
        "verticalAdjustment": None,
        "reactionTimeMs": None,
        "timeToDamageMs": None,
        "spottedAccuracy": None,
        "counterStrafePercent": None,
        "mechanicsEngagements": 0,
        "mechanicsExposures": 0,
        "spottedShots": 0,
        "counterStrafeShots": 0,
        "mechanicsQuality": "radar-beta",
    }


def frame_rows(frame: Any) -> list[dict[str, Any]]:
    if frame is None:
        return []
    if hasattr(frame, "to_dicts"):
        return frame.to_dicts()
    return list(frame)


def exposure_rows(
    ticks: pl.DataFrame, steam_id: str
) -> tuple[list[dict[str, Any]], set[int]]:
    required = {
        "tick",
        "round_num",
        "steamid",
        "side",
        "health",
        "pitch",
        "yaw",
        "approximate_spotted_by",
    }
    if not required.issubset(ticks.columns):
        return [], set()

    numeric_id = int(steam_id)
    base = ticks.select(required).sort(["steamid", "round_num", "tick"])
    viewer = base.filter(pl.col("steamid").cast(pl.String) == steam_id).select(
        "tick",
        "round_num",
        pl.col("side").alias("viewer_side"),
        pl.col("health").alias("viewer_health"),
        pl.col("pitch").alias("viewer_pitch"),
        pl.col("yaw").alias("viewer_yaw"),
    )
    enemies = base.filter(pl.col("steamid").cast(pl.String) != steam_id).with_columns(
        pl.col("approximate_spotted_by").list.contains(numeric_id).alias("seen")
    )
    enemies = enemies.with_columns(
        pl.col("seen")
        .shift(1)
        .over(["steamid", "round_num"])
        .fill_null(False)
        .alias("previous_seen"),
        pl.col("tick").shift(1).over(["steamid", "round_num"]).alias("previous_tick"),
    )
    joined = enemies.join(viewer, on=["tick", "round_num"], how="inner").filter(
        (pl.col("health") > 0)
        & (pl.col("viewer_health") > 0)
        & (pl.col("side") != pl.col("viewer_side"))
    )
    visible_ticks = set(
        joined.filter(pl.col("seen")).get_column("tick").unique().to_list()
    )
    starts = joined.filter(
        pl.col("seen")
        & (
            ~pl.col("previous_seen")
            | pl.col("previous_tick").is_null()
            | ((pl.col("tick") - pl.col("previous_tick")) > 1)
        )
    ).select(
        "tick",
        "round_num",
        pl.col("steamid").alias("enemy_steamid"),
        "viewer_pitch",
        "viewer_yaw",
    )
    return starts.to_dicts(), visible_ticks


def geometry_pairs(ticks: pl.DataFrame, steam_id: str) -> pl.DataFrame:
    required = {
        "tick",
        "round_num",
        "steamid",
        "side",
        "health",
        "pitch",
        "yaw",
        "X",
        "Y",
        "Z",
        "duck_amount",
    }
    if not required.issubset(ticks.columns):
        return pl.DataFrame()
    base = ticks.select(required)
    viewer = base.filter(pl.col("steamid").cast(pl.String) == steam_id).select(
        "tick",
        "round_num",
        pl.col("side").alias("viewer_side"),
        pl.col("health").alias("viewer_health"),
        pl.col("pitch").alias("viewer_pitch"),
        pl.col("yaw").alias("viewer_yaw"),
        pl.col("X").alias("viewer_x"),
        pl.col("Y").alias("viewer_y"),
        pl.col("Z").alias("viewer_z"),
        pl.col("duck_amount").alias("viewer_duck"),
    )
    enemies = base.filter(pl.col("steamid").cast(pl.String) != steam_id).select(
        "tick",
        "round_num",
        pl.col("steamid").alias("enemy_steamid"),
        pl.col("side").alias("target_side"),
        pl.col("health").alias("target_health"),
        pl.col("X").alias("target_x"),
        pl.col("Y").alias("target_y"),
        pl.col("Z").alias("target_z"),
        pl.col("duck_amount").alias("target_duck"),
    )
    return enemies.join(viewer, on=["tick", "round_num"], how="inner").filter(
        (pl.col("target_health") > 0)
        & (pl.col("viewer_health") > 0)
        & (pl.col("target_side") != pl.col("viewer_side"))
    )


def geometry_exposures(
    ticks: pl.DataFrame,
    steam_id: str,
    damages: list[dict[str, Any]],
    shot_ticks: set[int],
    mesh: Any,
) -> tuple[list[dict[str, Any]], set[int]]:
    pairs = geometry_pairs(ticks, steam_id)
    if pairs.is_empty():
        return [], set()

    shot_pairs = pairs.filter(pl.col("tick").is_in(shot_ticks)).to_dicts()
    shot_visibility = visible_rows(shot_pairs, mesh)
    visible_shot_ticks = {
        int(row["tick"])
        for row, is_visible in zip(shot_pairs, shot_visibility)
        if is_visible
    }

    exposures: list[dict[str, Any]] = []
    seen_keys: set[tuple[str, int, int]] = set()
    for damage in damages:
        damage_tick = int(damage.get("tick") or 0)
        round_num = int(damage.get("round_num") or 0)
        enemy_id = player_id(damage.get("victim_steamid"))
        window = (
            pairs.filter(
                (pl.col("enemy_steamid").cast(pl.String) == enemy_id)
                & (pl.col("round_num") == round_num)
                & (pl.col("tick") >= damage_tick - ENGAGEMENT_TICKS)
                & (pl.col("tick") <= damage_tick)
            )
            .sort("tick")
            .to_dicts()
        )
        visibility = visible_rows(window, mesh)
        visible_indices = [
            index for index, is_visible in enumerate(visibility) if is_visible
        ]
        if not visible_indices:
            continue
        index = visible_indices[-1]
        onset = index
        while onset > 0:
            current_tick = int(window[onset]["tick"])
            previous_tick = int(window[onset - 1]["tick"])
            if not visibility[onset - 1] or current_tick - previous_tick > 1:
                break
            onset -= 1
        row = window[onset]
        key = (enemy_id, round_num, int(row["tick"]))
        if key in seen_keys:
            continue
        seen_keys.add(key)
        exposures.append(
            {
                "tick": int(row["tick"]),
                "round_num": round_num,
                "enemy_steamid": enemy_id,
                "viewer_pitch": row["viewer_pitch"],
                "viewer_yaw": row["viewer_yaw"],
            }
        )
    return exposures, visible_shot_ticks


def calculate_mechanics(demo: Any, steam_id: str) -> dict[str, Any]:
    metrics = empty_metrics()
    ticks = getattr(demo, "ticks", None)
    if not isinstance(ticks, pl.DataFrame) or ticks.is_empty():
        return metrics

    shots = [
        row
        for row in frame_rows(getattr(demo, "shots", None))
        if player_id(row.get("player_steamid")) == steam_id
        and is_gun(row.get("weapon"))
    ]
    damages = [
        row
        for row in frame_rows(getattr(demo, "damages", None))
        if player_id(row.get("attacker_steamid")) == steam_id
        and player_id(row.get("victim_steamid")) != steam_id
        and is_gun(row.get("weapon"))
    ]
    map_name = str(getattr(demo, "header", {}).get("map_name") or "")
    mesh = load_map_mesh(map_name)
    try:
        if mesh is not None:
            try:
                exposures, visible_ticks = geometry_exposures(
                    ticks,
                    steam_id,
                    damages,
                    {int(row.get("tick", -1)) for row in shots},
                    mesh,
                )
            except (pl.exceptions.PolarsError, RuntimeError, TypeError, ValueError):
                exposures, visible_ticks = exposure_rows(ticks, steam_id)
            else:
                metrics["mechanicsQuality"] = "geometry"
        else:
            exposures, visible_ticks = exposure_rows(ticks, steam_id)
    except (pl.exceptions.PolarsError, RuntimeError, TypeError, ValueError):
        return metrics
    metrics["mechanicsExposures"] = len(exposures)

    spotted_shots = [row for row in shots if int(row.get("tick", -1)) in visible_ticks]
    metrics["spottedShots"] = len(spotted_shots)
    hit_ticks = {
        int(row["tick"]) for row in damages if int(row.get("tick", -1)) in visible_ticks
    }
    if spotted_shots:
        metrics["spottedAccuracy"] = round(100 * len(hit_ticks) / len(spotted_shots), 1)

    counter_shots: list[dict[str, Any]] = []
    proper_counter_shots = 0
    for shot in spotted_shots:
        weapon = normalized_weapon(shot.get("weapon"))
        max_speed = RIFLE_MAX_SPEED.get(weapon)
        duck = float(shot.get("player_duck_amount") or 0.0)
        velocity = float(shot.get("player_velocity") or 0.0)
        if max_speed is None or duck >= 0.1 or not math.isfinite(velocity):
            continue
        counter_shots.append(shot)
        if velocity < max_speed * 0.34:
            proper_counter_shots += 1
    metrics["counterStrafeShots"] = len(counter_shots)
    if counter_shots:
        metrics["counterStrafePercent"] = round(
            100 * proper_counter_shots / len(counter_shots), 1
        )

    exposures_by_target: dict[tuple[str, int], list[dict[str, Any]]] = defaultdict(list)
    for exposure in exposures:
        key = (player_id(exposure["enemy_steamid"]), int(exposure["round_num"]))
        exposures_by_target[key].append(exposure)

    shots_by_round: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for shot in spotted_shots:
        shots_by_round[int(shot.get("round_num") or 0)].append(shot)

    corrections: list[float] = []
    horizontal: list[float] = []
    vertical: list[float] = []
    reaction_times: list[float] = []
    damage_times: list[float] = []
    used_exposures: set[tuple[str, int, int]] = set()

    for damage in sorted(damages, key=lambda row: int(row.get("tick", 0))):
        round_num = int(damage.get("round_num") or 0)
        damage_tick = int(damage.get("tick") or 0)
        key = (player_id(damage.get("victim_steamid")), round_num)
        candidates = [
            exposure
            for exposure in exposures_by_target.get(key, [])
            if 0 <= damage_tick - int(exposure["tick"]) < ENGAGEMENT_TICKS
        ]
        if not candidates:
            continue
        exposure = max(candidates, key=lambda row: int(row["tick"]))
        exposure_key = (key[0], round_num, int(exposure["tick"]))
        if exposure_key in used_exposures:
            continue
        used_exposures.add(exposure_key)

        elapsed_ticks = damage_tick - int(exposure["tick"])
        damage_times.append(1000 * elapsed_ticks / CS2_TICKS_PER_SECOND)
        start_pitch = float(exposure["viewer_pitch"])
        start_yaw = float(exposure["viewer_yaw"])
        damage_pitch = damage.get("attacker_pitch")
        damage_yaw = damage.get("attacker_yaw")
        if damage_pitch is not None and damage_yaw is not None:
            yaw_change = abs(angle_delta(start_yaw, float(damage_yaw)))
            pitch_change = abs(float(damage_pitch) - start_pitch)
            horizontal.append(yaw_change)
            vertical.append(pitch_change)
            corrections.append(math.hypot(yaw_change, pitch_change))

        first_shot = next(
            (
                shot
                for shot in shots_by_round.get(round_num, [])
                if int(exposure["tick"]) <= int(shot.get("tick", -1)) <= damage_tick
            ),
            None,
        )
        if first_shot is not None:
            reaction_times.append(
                1000
                * (int(first_shot["tick"]) - int(exposure["tick"]))
                / CS2_TICKS_PER_SECOND
            )

    metrics.update(
        {
            "crosshairPlacement": median(corrections, 2),
            "horizontalAdjustment": median(horizontal, 2),
            "verticalAdjustment": median(vertical, 2),
            "reactionTimeMs": median(reaction_times, 0),
            "timeToDamageMs": median(damage_times, 0),
            "mechanicsEngagements": len(used_exposures),
        }
    )
    return metrics
