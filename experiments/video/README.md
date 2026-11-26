# Isolated video feasibility probe

Throwaway development tooling, not a plugin feature. See
[the requirements and investigation](../../docs/highlights.md).

The smoke test opens an animation in an independent Weston **headless**
compositor, plays a generated tone into a private PulseAudio sink, and captures
four seconds of H.264/AAC video. It never opens Steam or CS2, records your real
desktop, or uses the default microphone/speaker capture source.

Requires Weston 15 (including Xwayland support), FFmpeg/ffplay/ffprobe,
Xwayland, xdotool, Python 3, and pactl/pacat. Run from the repository root:

```bash
python3 experiments/video/headless-smoke.py --output /tmp/omarcs-isolation.mp4
```

For an extracted distro Weston package, with no system installation:

```bash
python3 experiments/video/headless-smoke.py \
  --weston-prefix /path/to/extracted-package \
  --output /tmp/omarcs-isolation.mp4
```

The probe refuses to overwrite the video, checks the isolated display identity,
checks that both media streams exist, and rejects silent audio. It leaves the
video and a sibling Weston log, removes its audio sink, and stops its processes.
Use a new output path for another run. On the development machine it selects
Mesa/Intel for the compositor; check the log to confirm actual GPU selection.

A successful smoke test proves the basic isolated capture path. It does **not**
prove that CS2 loads, that demo POV/tick control works, or that desktop latency
remains acceptable during sustained rendering. CS2 experiments need a separate
transient service with a runtime limit and whole-process-tree cleanup, since
Steam's container runtime creates additional process groups.

## Private media server

The animation smoke test alone uses a sink on the desktop audio server. For
stronger isolation, wrap it in the hardware-free media server:

```bash
python3 experiments/video/private-media.py -- \
  python3 experiments/video/headless-smoke.py \
  --weston-prefix /path/to/extracted-package --output /tmp/private-smoke.mp4
```

Requires PipeWire, pipewire-pulse and WirePlumber with its `policy` profile.
CS2 must always use this separate server: the game can restore an old output
selection and bypass a sink requested on the desktop server.

## Actual demo capture experiment

`capture-demo.py` packages the successful native Wayland/PipeWire capture path.
It is development tooling: it records the default POV and skips loading by
wall-clock time. Player selection and exact tick boundaries are still pending.
The saved CLI has passed a full playback run, syntax checks, existing-game
refusal and private-display guard checks. The native control probe below adds
player selection and tick-bounded recording.

Requires the above tools, systemd user services, GStreamer with `pipewiresrc`,
`pulsesrc`, `jpegenc` and `matroskamux`, and `pw-link`. Steam must be running and
signed in. Close CS2 before explicitly starting a capture:

```bash
python3 experiments/video/capture-demo.py \
  --weston-prefix /path/to/extracted-weston-package \
  --demo /absolute/path/to/match.dem \
  --output-dir /tmp/omarcs-demo-capture
```

Optional `--steam` selects the Steam root. Defaults: 720p, 30 FPS cap, discard
30 capture seconds, retain 10 seconds. `--skip` accepts 0–120 seconds and
`--duration` accepts 1–30. Choose a new output directory for every attempt.

The script owns a transient user service with a runtime limit, 400% CPU quota,
12 GiB memory ceiling, no swap and nice 10. It refuses an existing CS2 process.
Steam may show you as playing during this explicit batch. There are no timers
or startup hooks. The user confirmed normal desktop responsiveness and no
unexpected window or audio during short tests; longer batches still need review.

Outputs include `sample.mp4`, logs, probe metadata and recovery copies of
existing game config files. The parent stops the full service before restoring
those files. If another CS2 session is detected at cleanup, it leaves backups
for manual recovery rather than overwrite a live session's settings. Force
killing the parent or losing power can also require recovery; this is not yet a
production job manager. Backups remain in `config-backup/`, relative to Steam's
`userdata/`. No game library is installed by this capture script.

## Player selection and tick-bounded native recording

`control-probe.py` temporarily installs a hash-pinned CS Demo Manager library,
creates a scratch demo link/copy with tick actions, and invokes the isolated
capture CLI. It removes the library and restores gameinfo after the game stops.
This is still experimental; do not run concurrent probes or start a normal game
while it owns the installation. Recovery originals remain in the output folder.

Supply `static/cs2/libserver.so` from upstream revision
`eaffbbfa0114ddc9c24ae474c54afea686f51d10` of
[akiver/cs-demo-manager](https://github.com/akiver/cs-demo-manager).
SHA-256: `c6bab67a072f83d7ec8e57c1fe243179e267bd9631cb4ee2d9cbf192cea86780`.
Upstream is MIT licensed, copyright AkiVer; retain its LICENSE with any copy.
The probe does not download the library and refuses other binary hashes.

Example for the locally tested demo and its verified roster name:

```bash
python3 experiments/video/control-probe.py \
  --library /path/to/pinned/libserver.so \
  --weston-prefix /path/to/extracted-weston-package \
  --demo /absolute/path/to/match.dem \
  --account-id <steam-account-id> --player-name "<player name>" \
  --start-tick 2572 --native-seconds 3 \
  --output-dir /tmp/omarcs-tick-test
```

`--start-tick` is the setup/seek tick. Native recording begins 128 ticks later
and runs for `--native-seconds` (1–5), assuming this demo's 64 Hz tickrate.
In the example, recording spans ticks 2700–2892 and includes a headshot at 2801.
Account-ID commands alone did not select the correct POV; the verified roster
name is required for native recording. Duplicate names remain an unresolved case.

Native frames and game audio are moved from game-owned movie directories to
`native/`. The CLI requires the expected frame count, audio file and exact logged
start/end ticks, then encodes `tick-bounded.mp4` and writes
`native-validation.json`. It retains the raw files for inspection. A separate
`capture/sample.mp4` is a diagnostic wall-clock preview, not the trimmed clip.

Two three-second, 90-frame native recordings have been visually checked against
the parsed kill event, including correct POV and non-silent audio. The repeat
run passed the saved CLI's automatic validation and config/process cleanup checks. Demo playback
controls remain visible; UI polish, sustained performance, stable player-slot
mapping and full failure/cancellation recovery are still pending. Neither script
is connected to plugin startup, demo refresh, or a timer.
