from pathlib import Path

PANEL = Path(__file__).parents[1] / "Panel.qml"


def test_every_local_text_control_forces_plain_text() -> None:
    source = PANEL.read_text(encoding="utf-8")
    assert source.count("Text {") == source.count("textFormat: Text.PlainText")


def test_demo_strings_passed_to_shared_components_are_neutralized() -> None:
    source = PANEL.read_text(encoding="utf-8")
    assert "tooltipText: root.selectedMatch ? root.safeText(root.selectedMatch.map)" in source
    assert "title: root.selectedMatch ? root.safeText(root.selectedMatch.map)" in source
    assert "root.safeText(root.selectedMatch.player.name)" in source
