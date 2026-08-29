# omarCS

omarCS is a local-first CS2 match dashboard for the Omarchy shell. It imports Counter-Strike 2 `.dem` files, calculates personal match statistics with Awpy, stores a small local history, and presents the latest result and recent trends in the bar.

## MVP features

- Result and round score
- K/D/A, ADR, KAST, rating and headshot percentage
- Opening duels, trade kills and traded deaths
- Utility damage and flash impact
- Deterministic coaching notes
- Browsable five-match popup and ten-match averages
- Automatic retrieval and parsing of new Valve Premier/Competitive demos
- Automatic scanning of `~/Downloads`, `~/.local/share/omarcs/demos`, and the local CS2 folder

All demos and derived data stay on this computer.

## Local setup

```bash
uv tool install --editable --force .
mkdir -p ~/.config/omarchy/plugins
mkdir -p ~/.config/omarchy/plugins/omarcs.stats
install -m 0644 manifest.json Panel.qml ~/.config/omarchy/plugins/omarcs.stats/
omarchy plugin validate ~/.config/omarchy/plugins/omarcs.stats
omarchy-shell shell rescanPlugins
omarchy plugin enable omarcs.stats --section right
```

For development, keep uv's environment outside the plugin directory because
Omarchy intentionally rejects symlinks inside plugin folders:

```bash
UV_PROJECT_ENVIRONMENT="$XDG_CACHE_HOME/omarcs/venv" uv sync
UV_PROJECT_ENVIRONMENT="$XDG_CACHE_HOME/omarcs/venv" uv run pytest
```

Import a demo directly:

```bash
omarcs import ~/Downloads/match.dem
```

omarCS detects the most recently used local Steam account. If the demo belongs to another account, select it by SteamID64 or exact in-demo name:

```bash
omarcs import ~/Downloads/match.dem --player 76561198000000000
omarcs import ~/Downloads/match.dem --player "Player name"
```

Use **Older** / **Newer**, click a recent-match row, or press Left/Right (`H`/`L`)
to browse the five most recent games. Press `R`, click **Refresh demos**, or
middle-click the bar pill to scan again.

## Automatic matches

Enable the background fetcher once:

```bash
omarcs setup-auto
omarcs auto-status --pretty
```

CS2's Game State Integration schedules a replay check 30 seconds after the
match ends. It checks every 30 seconds for up to ten minutes, then returns to a
15-minute idle check. Existing matches are recorded as a baseline during setup,
so they are not downloaded again.

Downloads, decompression, and parsing pause or stop whenever CS2 reports
warmup, live play, or intermission. The user service also uses idle I/O
scheduling, a nice value of 10, and low systemd CPU/I/O weights. Partial
downloads resume later. Valve currently exposes the latest eight
Premier/Competitive replays; FACEIT is not yet automatic.

## Optional configuration

Create `~/.config/omarcs/config.toml`:

```toml
[player]
steam_id = "76561198000000000"

[import]
paths = ["~/Downloads", "~/.local/share/omarcs/demos"]

[history]
keep_recent = 20
```

Match history is stored in `~/.local/state/omarcs/omarcs.db`; the shell watches `summary.json` in the same directory.

## Current limitation

Steam permits only one active CS2 Game Coordinator session. If the helper
cannot query while CS2 still owns that session, omarCS keeps retrying during the
post-match window and again when the game closes. FACEIT demos currently require
manual download or privileged FACEIT Downloads API access.
