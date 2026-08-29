from omarcs.spray import aggregate_sprays


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
