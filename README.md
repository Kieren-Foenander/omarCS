# omarCS

omarCS is a local-first CS2 match dashboard for the Omarchy shell. It imports Counter-Strike 2 `.dem` files, calculates personal match statistics with the native Rust backend, stores a small local history, and presents the latest result and recent trends in the bar.

## MVP features

- Result and round score
- K/D/A, ADR, KAST, rating and headshot percentage
- Opening duels, trade kills and traded deaths
- Utility damage and flash impact
- Deterministic coaching notes
- Browsable five-match popup and ten-match averages
- Automatic retrieval and parsing of new Valve Premier/Competitive demos
- Automatic scanning of `~/Downloads`, `~/.local/share/omarcs/demos`, and the local CS2 folder
- Map-aware crosshair correction, first-shot time, time-to-damage, spotted
  accuracy, and proper counter-strafing
- Interactive AK-47, Galil, M4A4, and M4A1-S spray-control targets with
  numbered bullets, consistency halos, confidence, and coaching

All demos and derived data stay on this computer.

## Install on Omarchy

```bash
omarchy plugin add https://github.com/Kieren-Foenander/omarCS.git --enable
```

On a typical Omarchy installation, that is all users need to do. When enabled,
the widget creates an isolated Python environment, installs the CS2 Game State
Integration file, starts a user-level fetcher, and scans the usual demo folders.
The next Premier or Competitive match is picked up when it ends. A `.dem` file
placed in `~/Downloads` is found by the five-minute scan, or immediately with
**Refresh demos** in the widget.

`uv` and `cargo` are the prerequisites. `uv` is commonly already installed; if
the widget reports that it is missing, run this once and reopen the widget:

```bash
omarchy pkg add uv
```

The initial run creates a small Python environment for the launcher, installs
verified CS2 helper tools, and builds `omarcs-native` when that binary is
missing. It happens only after the user enables the plugin: Omarchy
intentionally never runs plugin install hooks during `omarchy plugin add`.

## Local development

Keep uv's environment outside the plugin directory because Omarchy rejects
symlinks inside plugin folders:

```bash
UV_PROJECT_ENVIRONMENT="$XDG_CACHE_HOME/omarcs/venv" uv sync
UV_PROJECT_ENVIRONMENT="$XDG_CACHE_HOME/omarcs/venv" uv run pytest
```

Import a demo directly (also useful for testing):

```bash
omarcs import ~/Downloads/match.dem
```

The plugin launcher still owns scanning, storage, and the Dashboard Summary. Match Report calculation now runs through `omarcs-native report`. First-time setup builds that binary when it is missing (`cargo` required). When a CS2 physics mesh is cached (or can be extracted), Engagements, Sprays, insights, and the Match Report use map-geometry visibility instead of spotted-by radar:

```bash
cargo build --release -p omarcs-native
target/release/omarcs-native probe ~/Downloads/match.dem --pretty
target/release/omarcs-native facts ~/Downloads/match.dem --pretty
target/release/omarcs-native stats ~/Downloads/match.dem "Player name" --pretty
target/release/omarcs-native mechanics ~/Downloads/match.dem "Player name" --pretty
target/release/omarcs-native sprays ~/Downloads/match.dem "Player name" --pretty
target/release/omarcs-native insights ~/Downloads/match.dem "Player name" --pretty
target/release/omarcs-native report ~/Downloads/match.dem "Player name" --pretty
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

Aim mechanics use collision geometry from the locally installed CS2 map to
reconstruct when an enemy enters your field of view. Crosshair placement is the
median view-angle correction from first visibility to first damage.
Time-to-damage excludes engagements lasting one second or longer;
counter-strafing counts uncrouched rifle shots below 34% of that weapon's
maximum movement speed. Static geometry cannot perfectly model smoke edges or
moving props, so trends across several matches are more meaningful than a
single duel.

Spray control groups bursts of at least five bullets that begin while settled
and have a plausible visible target. Each recorded shot ray is projected onto
the enemy's head plane at that tick. The dashboard shows the median position
for each bullet number across the latest ten matches; the halo is the middle
50% of those positions. Spray transfers are target-relative at every shot,
while wall spam and shots without a visible enemy are excluded. Low sample
counts are labelled rather than presented as reliable coaching.

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

## Update and remove

```bash
omarchy plugin update omarcs.stats
omarchy plugin remove omarcs.stats
```

Removing the plugin stops the dashboard. To remove automatic match fetching as
well, run `systemctl --user disable --now omarcs-autofetch.service` and delete
`~/.config/systemd/user/omarcs-autofetch.service`. Local match history remains
in `~/.local/state/omarcs/` until the user removes it.
