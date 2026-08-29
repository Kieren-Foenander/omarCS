from types import SimpleNamespace

from omarcs.analysis import calculate_player_metrics, coaching_insights


PLAYER = "76561198000000001"
TEAMMATE = "76561198000000002"
ENEMY_A = "76561198000000003"
ENEMY_B = "76561198000000004"


def fake_demo() -> SimpleNamespace:
    kills = [
        {"tick": 100, "round_num": 1, "attacker_steamid": PLAYER, "victim_steamid": ENEMY_A, "attacker_side": "ct", "victim_side": "t", "headshot": True, "assister_steamid": None},
        {"tick": 300, "round_num": 1, "attacker_steamid": ENEMY_B, "victim_steamid": TEAMMATE, "attacker_side": "t", "victim_side": "ct", "headshot": False, "assister_steamid": PLAYER},
        {"tick": 1000, "round_num": 2, "attacker_steamid": ENEMY_A, "victim_steamid": PLAYER, "attacker_side": "ct", "victim_side": "t", "headshot": False, "assister_steamid": None},
        {"tick": 1100, "round_num": 2, "attacker_steamid": TEAMMATE, "victim_steamid": ENEMY_A, "attacker_side": "t", "victim_side": "ct", "headshot": False, "assister_steamid": None},
        {"tick": 2000, "round_num": 3, "attacker_steamid": PLAYER, "victim_steamid": ENEMY_B, "attacker_side": "t", "victim_side": "ct", "headshot": False, "assister_steamid": None},
    ]
    damages = [
        {"attacker_steamid": PLAYER, "victim_steamid": ENEMY_A, "attacker_side": "ct", "victim_side": "t", "weapon": "ak47", "dmg_health_real": 100},
        {"attacker_steamid": PLAYER, "victim_steamid": ENEMY_B, "attacker_side": "t", "victim_side": "ct", "weapon": "hegrenade", "dmg_health_real": 40},
        {"attacker_steamid": PLAYER, "victim_steamid": PLAYER, "attacker_side": "t", "victim_side": "t", "weapon": "hegrenade", "dmg_health_real": 10},
    ]
    rounds = [
        {"round_num": 1, "winner": "ct"},
        {"round_num": 2, "winner": "t"},
        {"round_num": 3, "winner": "ct"},
    ]
    ticks = []
    for round_num, side in ((1, "ct"), (2, "t"), (3, "t")):
        ticks.extend({"round_num": round_num, "steamid": PLAYER, "name": "Kieren", "side": side} for _ in range(3))
    blinds = [
        {"attacker_steamid": PLAYER, "user_steamid": ENEMY_A, "attacker_side": "t", "user_side": "ct", "blind_duration": 2.4},
        {"attacker_steamid": PLAYER, "user_steamid": TEAMMATE, "attacker_side": "t", "user_side": "t", "blind_duration": 1.0},
    ]
    return SimpleNamespace(kills=kills, damages=damages, rounds=rounds, ticks=ticks, events={"player_blind": blinds}, tickrate=128)


def test_calculates_mvp_metrics() -> None:
    stats = calculate_player_metrics(fake_demo(), PLAYER)
    assert stats["kills"] == 2
    assert stats["deaths"] == 1
    assert stats["assists"] == 1
    assert stats["headshotPercent"] == 50.0
    assert stats["adr"] == 46.7
    assert stats["kast"] == 100.0
    assert stats["openingKills"] == 2
    assert stats["openingDeaths"] == 1
    assert stats["tradedDeaths"] == 1
    assert stats["tradeKills"] == 0
    assert stats["utilityDamage"] == 40
    assert stats["enemiesFlashed"] == 1
    assert stats["friendsFlashed"] == 1
    assert stats["roundsFor"] == 2
    assert stats["roundsAgainst"] == 1
    assert stats["result"] == "W"


def test_coaching_notes_are_bounded() -> None:
    notes = coaching_insights(calculate_player_metrics(fake_demo(), PLAYER))
    assert 1 <= len(notes) <= 3

