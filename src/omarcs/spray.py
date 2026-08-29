from __future__ import annotations

import math
import statistics
from collections import defaultdict
from typing import Any

MIN_SPRAY_SHOTS = 5
MAX_SPRAY_SHOTS = 10

WEAPONS = (
    {"id": "ak47", "name": "AK-47", "shortName": "AK"},
    {"id": "galilar", "name": "Galil AR", "shortName": "GALIL"},
    {"id": "m4a4", "name": "M4A4", "shortName": "M4A4"},
    {"id": "m4a1_silencer", "name": "M4A1-S", "shortName": "M4A1-S"},
)
WEAPON_ORDER = tuple(weapon["id"] for weapon in WEAPONS)
WEAPONS_BY_ID = {weapon["id"]: weapon for weapon in WEAPONS}


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
