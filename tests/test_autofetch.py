from omarcs.autofetch import GameState, extract_replay_urls


def test_extracts_unique_replay_urls_in_order() -> None:
    first = b"http://replay423.valve.net/730/one.dem.bz2"
    second = b"http://replay171.valve.net/730/two.dem.bz2"
    assert extract_replay_urls(b"x" + first + b"\x00" + second + b"\x00" + first) == [
        first.decode(),
        second.decode(),
    ]


def test_game_state_blocks_heavy_work_during_a_match() -> None:
    state = GameState()
    state.process_checked_at = 10**12
    assert state.heavy_work_allowed()
    state.process_running = True
    state.update({"map": {"phase": "live"}}, now=1.0)
    assert not state.heavy_work_allowed()
    state.update({"map": {"phase": "gameover"}}, now=2.0)
    assert state.heavy_work_allowed()
    assert state.snapshot()[2] == 2.0
