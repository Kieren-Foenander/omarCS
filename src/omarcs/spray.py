from __future__ import annotations

import math
import statistics
from collections import defaultdict
from typing import Any

import polars as pl

from .geometry import visible_rows

SPRAY_GAP_TICKS = 16
MIN_SPRAY_SHOTS = 5
MAX_SPRAY_SHOTS = 10
MAX_TARGET_ANGLE = 15.0
MAX_PROJECTED_OFFSET = 220.0

WEAPONS = {
    7: {"id": "ak47", "name": "AK-47", "shortName": "AK", "maxSpeed": 215},
    13: {"id": "galilar", "name": "Galil AR", "shortName": "GALIL", "maxSpeed": 215},
    16: {"id": "m4a4", "name": "M4A4", "shortName": "M4A4", "maxSpeed": 225},
    60: {
        "id": "m4a1_silencer",
        "name": "M4A1-S",
        "shortName": "M4A1-S",
        "maxSpeed": 225,
    },
}
WEAPON_ORDER = tuple(weapon["id"] for weapon in WEAPONS.values())
WEAPONS_BY_ID = {weapon["id"]: weapon for weapon in WEAPONS.values()}


def rows(frame: Any) -> list[dict[str, Any]]:
    if frame is None:
        return []
    if hasattr(frame, "to_dicts"):
        return frame.to_dicts()
    return list(frame)


def player_id(raw: Any) -> str:
    if raw is None:
        return ""
    if isinstance(raw, float) and raw.is_integer():
        return str(int(raw))
    return str(raw)


def vector_dot(
    first: tuple[float, float, float], second: tuple[float, float, float]
) -> float:
    return sum(left * right for left, right in zip(first, second))


def vector_subtract(
    first: tuple[float, float, float], second: tuple[float, float, float]
) -> tuple[float, float, float]:
    return tuple(left - right for left, right in zip(first, second))  # type: ignore[return-value]


def vector_add_scaled(
    origin: tuple[float, float, float],
    direction: tuple[float, float, float],
    scale: float,
) -> tuple[float, float, float]:
    return tuple(
        start + component * scale for start, component in zip(origin, direction)
    )  # type: ignore[return-value]


def vector_length(vector: tuple[float, float, float]) -> float:
    return math.sqrt(vector_dot(vector, vector))


def normalize(vector: tuple[float, float, float]) -> tuple[float, float, float] | None:
    length = vector_length(vector)
    if length <= 1e-9:
        return None
    return tuple(component / length for component in vector)  # type: ignore[return-value]


def cross(
    first: tuple[float, float, float], second: tuple[float, float, float]
) -> tuple[float, float, float]:
    return (
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    )


def direction_from_angles(
    pitch_degrees: float, yaw_degrees: float
) -> tuple[float, float, float]:
    pitch = math.radians(pitch_degrees)
    yaw = math.radians(yaw_degrees)
    return (
        math.cos(pitch) * math.cos(yaw),
        math.cos(pitch) * math.sin(yaw),
        -math.sin(pitch),
    )


def angular_distance(
    first: tuple[float, float, float], second: tuple[float, float, float]
) -> float:
    return math.degrees(math.acos(max(-1.0, min(1.0, vector_dot(first, second)))))


def shot_origin(shot: dict[str, Any]) -> tuple[float, float, float] | None:
    try:
        origin = (
            float(shot["origin_x"]),
            float(shot["origin_y"]),
            float(shot["origin_z"]),
        )
    except (KeyError, TypeError, ValueError):
        return None
    return origin if all(math.isfinite(value) for value in origin) else None


def shot_direction(shot: dict[str, Any]) -> tuple[float, float, float] | None:
    try:
        pitch = float(shot["angles_x"])
        yaw = float(shot["angles_y"])
    except (KeyError, TypeError, ValueError):
        return None
    if not math.isfinite(pitch) or not math.isfinite(yaw):
        return None
    return direction_from_angles(pitch, yaw)


def target_head(target: dict[str, Any]) -> tuple[float, float, float]:
    duck = float(target.get("target_duck") or 0.0)
    return (
        float(target["target_x"]),
        float(target["target_y"]),
        float(target["target_z"]) + 64.0 - 18.0 * duck,
    )


def projected_offset(
    shot: dict[str, Any], target: dict[str, Any]
) -> tuple[float, float] | None:
    origin = shot_origin(shot)
    direction = shot_direction(shot)
    if origin is None or direction is None:
        return None
    head = target_head(target)
    target_vector = vector_subtract(head, origin)
    normal = normalize(target_vector)
    if normal is None:
        return None
    denominator = vector_dot(direction, normal)
    if denominator <= 0.05:
        return None
    impact = vector_add_scaled(
        origin, direction, vector_length(target_vector) / denominator
    )
    offset = vector_subtract(impact, head)
    right = normalize((-normal[1], normal[0], 0.0))
    if right is None:
        return None
    up = normalize(cross(normal, right))
    if up is None:
        return None
    horizontal = vector_dot(offset, right)
    vertical = vector_dot(offset, up)
    if not math.isfinite(horizontal) or not math.isfinite(vertical):
        return None
    if abs(horizontal) > MAX_PROJECTED_OFFSET or abs(vertical) > MAX_PROJECTED_OFFSET:
        return None
    return horizontal, vertical


def fire_bullet_rows(demo: Any, steam_id: str) -> list[dict[str, Any]]:
    events = getattr(demo, "events", {}) or {}
    bullets = rows(events.get("fire_bullets"))
    return sorted(
        (row for row in bullets if player_id(row.get("user_steamid")) == steam_id),
        key=lambda row: int(row.get("tick") or 0),
    )


def group_sprays(
    bullets: list[dict[str, Any]],
) -> list[tuple[dict[str, Any], list[dict[str, Any]]]]:
    grouped: list[tuple[dict[str, Any], list[dict[str, Any]]]] = []
    current: list[dict[str, Any]] = []
    current_weapon: dict[str, Any] | None = None
    previous_tick: int | None = None

    def finish() -> None:
        nonlocal current, current_weapon
        if current_weapon is not None and len(current) >= MIN_SPRAY_SHOTS:
            grouped.append((current_weapon, current[:MAX_SPRAY_SHOTS]))
        current = []
        current_weapon = None

    for bullet in bullets:
        try:
            item_definition = int(bullet.get("item_def_index"))
            tick = int(bullet.get("tick"))
        except (TypeError, ValueError):
            finish()
            previous_tick = None
            continue
        weapon = WEAPONS.get(item_definition)
        if weapon is None:
            finish()
            previous_tick = None
            continue
        continues = (
            current_weapon is not None
            and current_weapon["id"] == weapon["id"]
            and previous_tick is not None
            and tick - previous_tick <= SPRAY_GAP_TICKS
        )
        if not continues:
            finish()
            current_weapon = weapon
        current.append(bullet)
        previous_tick = tick
    finish()
    return grouped


def target_candidates(
    ticks: pl.DataFrame,
    steam_id: str,
    shot_ticks: set[int],
    mesh: Any,
) -> dict[int, list[dict[str, Any]]]:
    required = {
        "tick",
        "steamid",
        "side",
        "health",
        "pitch",
        "yaw",
        "X",
        "Y",
        "Z",
        "duck_amount",
        "velocity",
        "approximate_spotted_by",
    }
    if not required.issubset(ticks.columns) or not shot_ticks:
        return {}
    selected = ticks.filter(pl.col("tick").is_in(shot_ticks)).to_dicts()
    viewers: dict[int, dict[str, Any]] = {}
    enemies: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in selected:
        tick = int(row.get("tick") or 0)
        if player_id(row.get("steamid")) == steam_id:
            viewers[tick] = row
        else:
            enemies[tick].append(row)

    candidates: dict[int, list[dict[str, Any]]] = defaultdict(list)
    visibility_inputs: list[dict[str, Any]] = []
    visibility_targets: list[dict[str, Any]] = []
    numeric_id = int(steam_id)
    for tick, viewer in viewers.items():
        if float(viewer.get("health") or 0) <= 0:
            continue
        for enemy in enemies.get(tick, []):
            if float(enemy.get("health") or 0) <= 0 or enemy.get("side") == viewer.get(
                "side"
            ):
                continue
            candidate = {
                "tick": tick,
                "enemy_steamid": player_id(enemy.get("steamid")),
                "viewer_velocity": float(viewer.get("velocity") or 0.0),
                "target_x": enemy.get("X"),
                "target_y": enemy.get("Y"),
                "target_z": enemy.get("Z"),
                "target_duck": enemy.get("duck_amount"),
                "radar_visible": numeric_id
                in (enemy.get("approximate_spotted_by") or []),
            }
            visibility_inputs.append(
                {
                    **candidate,
                    "viewer_x": viewer.get("X"),
                    "viewer_y": viewer.get("Y"),
                    "viewer_z": viewer.get("Z"),
                    "viewer_duck": viewer.get("duck_amount"),
                    "viewer_pitch": viewer.get("pitch"),
                    "viewer_yaw": viewer.get("yaw"),
                }
            )
            visibility_targets.append(candidate)

    geometry_visibility = (
        visible_rows(visibility_inputs, mesh) if mesh is not None else []
    )
    for index, candidate in enumerate(visibility_targets):
        candidate["visible"] = (
            geometry_visibility[index]
            if mesh is not None
            else candidate["radar_visible"]
        )
        candidates[int(candidate["tick"])].append(candidate)
    return candidates


def closest_target(
    shot: dict[str, Any], candidates: list[dict[str, Any]]
) -> dict[str, Any] | None:
    origin = shot_origin(shot)
    direction = shot_direction(shot)
    if origin is None or direction is None:
        return None
    ranked: list[tuple[float, dict[str, Any]]] = []
    for candidate in candidates:
        if not candidate.get("visible"):
            continue
        target_direction = normalize(vector_subtract(target_head(candidate), origin))
        if target_direction is None:
            continue
        ranked.append((angular_distance(direction, target_direction), candidate))
    if not ranked:
        return None
    angle, target = min(ranked, key=lambda item: item[0])
    return target if angle <= MAX_TARGET_ANGLE else None


def calculate_spray_bursts(
    demo: Any, steam_id: str, mesh: Any = None
) -> list[dict[str, Any]]:
    ticks = getattr(demo, "ticks", None)
    if not isinstance(ticks, pl.DataFrame) or ticks.is_empty():
        return []
    grouped = group_sprays(fire_bullet_rows(demo, steam_id))
    shot_ticks = {
        int(shot.get("tick") or 0) for _weapon, burst in grouped for shot in burst
    }
    candidates = target_candidates(ticks, steam_id, shot_ticks, mesh)
    sprays: list[dict[str, Any]] = []

    for weapon, burst in grouped:
        points: list[dict[str, Any]] = []
        for number, shot in enumerate(burst, start=1):
            tick = int(shot.get("tick") or 0)
            target = closest_target(shot, candidates.get(tick, []))
            if target is None:
                continue
            velocity = float(target.get("viewer_velocity") or 0.0)
            if not points and (
                not math.isfinite(velocity)
                or velocity >= float(weapon["maxSpeed"]) * 0.34
            ):
                break
            offset = projected_offset(shot, target)
            if offset is None:
                continue
            points.append(
                {
                    "number": number,
                    "x": round(offset[0], 2),
                    "y": round(offset[1], 2),
                }
            )
        if len(points) >= MIN_SPRAY_SHOTS:
            sprays.append({"weapon": weapon["id"], "shots": points})
    return sprays


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return 0.0
    if len(ordered) == 1:
        return ordered[0]
    position = fraction * (len(ordered) - 1)
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def confidence_label(spray_count: int) -> str:
    if spray_count >= 15:
        return "HIGH"
    if spray_count >= 8:
        return "GOOD"
    if spray_count >= 4:
        return "LOW"
    return "MORE DATA NEEDED"


def coach_spray(weapon_name: str, shots: list[dict[str, Any]], spray_count: int) -> str:
    if spray_count < 4:
        return f"Only {spray_count} qualifying {weapon_name} spray{'s' if spray_count != 1 else ''}; keep collecting matches."
    minimum_samples = max(2, math.ceil(spray_count / 2))
    late = [
        shot
        for shot in shots
        if 6 <= int(shot.get("number") or 0) <= 10
        and int(shot.get("samples") or 0) >= minimum_samples
    ]
    late = late or [
        shot for shot in shots if int(shot.get("samples") or 0) >= minimum_samples
    ]
    horizontal = statistics.median(float(shot["x"]) for shot in late)
    vertical = statistics.median(float(shot["y"]) for shot in late)
    prefix = "Early read: " if spray_count < 8 else ""
    suffix = " More sprays will firm this up." if spray_count < 8 else ""
    if abs(horizontal) < 7.0 and abs(vertical) < 7.0:
        return (
            prefix
            + "the later bullets stay centred; this spray shape is controlled."
            + suffix
        )
    if abs(horizontal) >= abs(vertical):
        if horizontal > 0:
            return (
                prefix
                + "the later bullets drift right; pull slightly further left after bullet 5."
                + suffix
            )
        return (
            prefix
            + "the later bullets drift left; ease the leftward pull after bullet 5."
            + suffix
        )
    if vertical > 0:
        return (
            prefix
            + "the later bullets climb high; pull down more firmly after bullet 5."
            + suffix
        )
    return (
        prefix
        + "the later bullets land low; ease the downward pull after bullet 5."
        + suffix
    )


def aggregate_sprays(matches: list[dict[str, Any]]) -> dict[str, Any]:
    bursts_by_weapon: dict[str, list[list[dict[str, Any]]]] = defaultdict(list)
    for match in matches[:10]:
        for burst in match.get("sprays", []):
            weapon = str(burst.get("weapon") or "")
            shots = burst.get("shots") or []
            if weapon in WEAPONS_BY_ID and len(shots) >= MIN_SPRAY_SHOTS:
                bursts_by_weapon[weapon].append(shots[:MAX_SPRAY_SHOTS])

    weapons: list[dict[str, Any]] = []
    for weapon_id in WEAPON_ORDER:
        weapon = WEAPONS_BY_ID[weapon_id]
        bursts = bursts_by_weapon.get(weapon_id, [])
        aggregated: list[dict[str, Any]] = []
        for number in range(1, MAX_SPRAY_SHOTS + 1):
            points = [
                shot
                for burst in bursts
                for shot in burst
                if int(shot.get("number") or 0) == number
            ]
            if not points:
                continue
            horizontal = [float(point["x"]) for point in points]
            vertical = [float(point["y"]) for point in points]
            aggregated.append(
                {
                    "number": number,
                    "x": round(statistics.median(horizontal), 2),
                    "y": round(statistics.median(vertical), 2),
                    "radiusX": round(
                        max(
                            1.5,
                            (
                                percentile(horizontal, 0.75)
                                - percentile(horizontal, 0.25)
                            )
                            / 2,
                        ),
                        2,
                    ),
                    "radiusY": round(
                        max(
                            1.5,
                            (percentile(vertical, 0.75) - percentile(vertical, 0.25))
                            / 2,
                        ),
                        2,
                    ),
                    "samples": len(points),
                }
            )
        spray_count = len(bursts)
        weapons.append(
            {
                "id": weapon_id,
                "name": weapon["name"],
                "shortName": weapon["shortName"],
                "sprays": spray_count,
                "confidence": confidence_label(spray_count),
                "shots": aggregated,
                "coach": coach_spray(weapon["name"], aggregated, spray_count),
            }
        )
    return {"matches": min(10, len(matches)), "weapons": weapons}
