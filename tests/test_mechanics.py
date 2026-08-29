import subprocess
from types import SimpleNamespace

import polars as pl
import trimesh
from trimesh.ray.ray_pyembree import RayMeshIntersector

from omarcs import geometry
from omarcs.geometry import visible_rows
from omarcs.mechanics import calculate_mechanics

PLAYER = 76561198000000001
ENEMY = 76561198000000002


def test_calculates_engagement_mechanics() -> None:
    ticks = []
    for tick in range(1, 31):
        ticks.append(
            {
                "tick": tick,
                "round_num": 1,
                "steamid": PLAYER,
                "side": "ct",
                "health": 100,
                "pitch": 0.0,
                "yaw": 0.0,
                "approximate_spotted_by": [],
            }
        )
        ticks.append(
            {
                "tick": tick,
                "round_num": 1,
                "steamid": ENEMY,
                "side": "t",
                "health": 100,
                "pitch": 0.0,
                "yaw": 180.0,
                "approximate_spotted_by": [PLAYER] if tick >= 10 else [],
            }
        )
    shots = pl.DataFrame(
        [
            {
                "tick": 16,
                "round_num": 1,
                "player_steamid": PLAYER,
                "weapon": "ak47",
                "player_velocity": 20.0,
                "player_duck_amount": 0.0,
            }
        ]
    )
    damages = pl.DataFrame(
        [
            {
                "tick": 20,
                "round_num": 1,
                "attacker_steamid": PLAYER,
                "victim_steamid": ENEMY,
                "weapon": "ak47",
                "attacker_pitch": 0.0,
                "attacker_yaw": 5.0,
            }
        ]
    )
    demo = SimpleNamespace(ticks=pl.DataFrame(ticks), shots=shots, damages=damages)
    stats = calculate_mechanics(demo, str(PLAYER))

    assert stats["mechanicsExposures"] == 1
    assert stats["mechanicsEngagements"] == 1
    assert stats["crosshairPlacement"] == 5.0
    assert stats["horizontalAdjustment"] == 5.0
    assert stats["verticalAdjustment"] == 0.0
    assert stats["reactionTimeMs"] == 94.0
    assert stats["timeToDamageMs"] == 156.0
    assert stats["spottedAccuracy"] == 100.0
    assert stats["counterStrafePercent"] == 100.0


def test_returns_empty_metrics_without_tick_properties() -> None:
    demo = SimpleNamespace(
        ticks=pl.DataFrame({"tick": [1]}), shots=pl.DataFrame(), damages=pl.DataFrame()
    )
    stats = calculate_mechanics(demo, str(PLAYER))
    assert stats["crosshairPlacement"] is None
    assert stats["mechanicsEngagements"] == 0


def test_map_geometry_blocks_visibility() -> None:
    wall = trimesh.creation.box(extents=[1, 4, 100])
    wall.apply_translation([5, 0, 64])
    rows = [
        {
            "viewer_x": 0,
            "viewer_y": 0,
            "viewer_z": 0,
            "viewer_duck": 0,
            "viewer_pitch": 0,
            "viewer_yaw": 0,
            "target_x": 10,
            "target_y": 0,
            "target_z": 0,
            "target_duck": 0,
        },
        {
            "viewer_x": 0,
            "viewer_y": 0,
            "viewer_z": 0,
            "viewer_duck": 0,
            "viewer_pitch": 0,
            "viewer_yaw": 90,
            "target_x": 0,
            "target_y": 10,
            "target_z": 0,
            "target_duck": 0,
        },
    ]
    assert visible_rows(rows, RayMeshIntersector(wall)) == [False, True]


def test_map_geometry_failure_falls_back(monkeypatch) -> None:
    def fail_export(_map_name: str):
        raise subprocess.TimeoutExpired("vrf", 120)

    monkeypatch.setattr(geometry, "geometry_path", fail_export)
    assert geometry.load_map_mesh("de_nuke") is None
