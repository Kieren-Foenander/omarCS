from pathlib import Path

from omarcs.config import detect_active_steam_id


def test_detects_most_recent_steam_account(tmp_path: Path) -> None:
    loginusers = tmp_path / "loginusers.vdf"
    loginusers.write_text(
        '''"users"
{
  "76561198000000001"
  {
    "AccountName" "old"
    "MostRecent" "0"
  }
  "76561198000000002"
  {
    "AccountName" "current"
    "MostRecent" "1"
  }
}
''',
        encoding="utf-8",
    )
    assert detect_active_steam_id((loginusers,)) == "76561198000000002"

