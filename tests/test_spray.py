from types import SimpleNamespace

import polars as pl

from omarcs.spray import aggregate_sprays, calculate_spray_bursts, projected_offset

PLAYER = 76561198000000001
ENEMY = 76561198000000002


def test_projects_shot_onto_enemy_plane() -> None:
    target = {"target_x": 100, "target_y": 0, "target_z": 0, "target_duck": 0}
    centred = {
        "origin_x": 0,
        "origin_y": 0,
        "origin_z": 64,
        "angles_x": 0,
        "angles_y": 0,
    }
    right = {**centred, "angles_y": 5}

    assert projected_offset(centred, target) == (0.0, 0.0)
    horizontal, vertical = projected_offset(right, target)
    assert round(horizontal, 1) == 8.7
    assert round(vertical, 1) == 0.0


def test_extracts_numbered_target_relative_spray() -> None:
    ticks = []
    bullets = []
    for number, tick in enumerate((100, 106, 112, 118, 124, 130), start=1):
        ticks.extend(
            [
                {
                    "tick": tick,
                    "steamid": PLAYER,
                    "side": "ct",
                    "health": 100,
                    "pitch": 0.0,
                    "yaw": 0.0,
                    "X": 0.0,
                    "Y": 0.0,
                    "Z": 0.0,
                    "duck_amount": 0.0,
                    "velocity": 0.0,
                    "approximate_spotted_by": [],
                },
                {
                    "tick": tick,
                    "steamid": ENEMY,
                    "side": "t",
                    "health": 100,
                    "pitch": 0.0,
                    "yaw": 180.0,
                    "X": 100.0,
                    "Y": 0.0,
                    "Z": 0.0,
                    "duck_amount": 0.0,
                    "velocity": 0.0,
                    "approximate_spotted_by": [PLAYER],
                },
            ]
        )
        bullets.append(
            {
                "tick": tick,
                "user_steamid": PLAYER,
                "item_def_index": 7,
                "origin_x": 0.0,
                "origin_y": 0.0,
                "origin_z": 64.0,
                "angles_x": 0.0,
                "angles_y": number - 1,
            }
        )
    demo = SimpleNamespace(
        ticks=pl.DataFrame(ticks),
        events={"fire_bullets": pl.DataFrame(bullets)},
    )

    sprays = calculate_spray_bursts(demo, str(PLAYER))

    assert len(sprays) == 1
    assert sprays[0]["weapon"] == "ak47"
    assert [shot["number"] for shot in sprays[0]["shots"]] == [1, 2, 3, 4, 5, 6]
    assert sprays[0]["shots"][0]["x"] == 0.0
    assert sprays[0]["shots"][5]["x"] > 8.0


def test_aggregates_each_bullet_number_across_matches() -> None:
    bursts = [
        {
            "weapon": "ak47",
            "shots": [
                {"number": number, "x": offset, "y": -offset}
                for number, offset in enumerate(offsets, start=1)
            ],
        }
        for offsets in (
            [0, 1, 2, 3, 4],
            [0, 2, 4, 6, 8],
            [0, 3, 6, 9, 12],
            [0, 4, 8, 12, 16],
        )
    ]
    summary = aggregate_sprays([{"sprays": bursts}])
    ak = summary["weapons"][0]

    assert ak["sprays"] == 4
    assert ak["confidence"] == "LOW"
    assert ak["shots"][4]["x"] == 10.0
    assert ak["shots"][4]["y"] == -10.0
    assert ak["shots"][4]["samples"] == 4


def test_aggregation_preserves_missing_bullet_numbers() -> None:
    burst = {
        "weapon": "ak47",
        "shots": [
            {"number": number, "x": number, "y": -number} for number in (1, 3, 4, 5, 6)
        ],
    }

    ak = aggregate_sprays([{"sprays": [burst]}])["weapons"][0]

    assert [shot["number"] for shot in ak["shots"]] == [1, 3, 4, 5, 6]
