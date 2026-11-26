# Highlights: requirements and rendering investigation

Status: first version implemented and installed locally; native rendering remains beta.

## Agreed behaviour

- Detect highlights for the player selected in the Match Report: aces, strong
  multi-kills, AWP flick candidates, and eventually clutches.
- The computer must remain usable while producing videos. Rendering must not
  take over the real display, focus, keyboard, mouse, or speakers.
- Video batches start only through an explicit user action. Steam may show the
  user as playing CS2 during that batch. Never launch CS2 from an idle timer.
- Preserve local-first operation if local rendering can meet these requirements.
- Manual Steam refresh is separate: a button-triggered query, followed by
  download and analysis. Startup and timed scans never query Steam.

## First milestone

Produce one short, correctly timed video of the selected player's POV, including
only game audio, on an isolated display. Verify that the real desktop remains
responsive. Detection and the Highlights UI follow this feasibility check.

Investigate a Weston headless compositor with a private Wayland socket and its
own Xwayland display. On the development machine, Intel UHD 770 rendering is
available alongside the NVIDIA RTX 3070; using the Intel GPU may reduce
contention with desktop work. Sharing CPU, memory and storage still requires
measurement. A headless window alone does not prove desktop responsiveness.

Use a separate temporary PipeWire/PulseAudio server with a policy-only session
manager and no hardware devices. Capture its null-sink monitor, never the desktop
speaker monitor or microphone. A sink on the desktop server is insufficient:
CS2 restored its previously selected headset in an early test. Bound each experiment and clean up the whole
process tree, audio sink and temporary game configuration on success, failure,
or cancellation. Never kill an existing user-owned CS2 instance.

## Integration after proof

1. Add deterministic highlight detection downstream of Match Facts. Retain kill
   weapons in normalized facts and correlate shots with aim/target observations
   for flick candidates. Tune thresholds against manually reviewed demos.
2. Store tick intervals, player ID, labels and supporting evidence in Match
   Reports. Merge overlaps; preserve enough context for complete aces/clutches.
3. Retain source demos while video jobs need them. The existing automatic and
   manual download pipeline currently deletes demos after successful analysis.
4. Persist video jobs separately from Match Reports, with cancellation, retry,
   output validation and disk limits. A recording failure must not invalidate
   the Match Report.
5. Add a Highlights section with render, play, save and dismiss actions. Begin
   with individual clips; montage editing can follow.

## Evidence and open questions

2026-09-06/07, development machine:

- Weston 15.0.1 headless GL startup succeeded on Intel UHD 770 at 1280x720.
  Actual CS2 playback was subsequently established using native SDL Wayland
  on NVIDIA. Intel CS2 playback remains unproven.
- `--no-xwm-decorations` and `--fake-seat` resolved the initial Xwayland
  assertion. Steam must already be running and signed in for playback.
- Actual Inferno demo playback was captured: 10.032 seconds, H.264 1280x720,
  AAC with non-silent game audio. Local artifact:
  `/tmp/omarcs-video-spike/cs2-private-sample.mp4`.
- Audio used a separate PipeWire server, WirePlumber's `policy` profile and
  pipewire-pulse. Its sole sink was `omarcs_private_default`; no hardware
  monitors were enabled. A generated tone also verified this capture path.
- Weston exported video through its PipeWire backend. Explicit port linking
  connected GStreamer to that output. GStreamer muxed video/audio with their
  timestamps before FFmpeg encoding; raw-frame piping had lost timing.
- The successful test ran in a transient user service with 400% CPU quota,
  12 GiB memory ceiling, zero swap allowance, nice 10 and a runtime limit.
  The user confirmed that the September 7 short rendering tests felt normal,
  with a responsive desktop and no unexpected game window or game audio.
  Sustained usability during longer batches remains unverified.
- The saved `capture-demo.py` completed a full playback test on September 7,
  including game audio and config/process cleanup. It remains a wall-clock
  preview tool; exact intervals use the native recording path below.

### Playback control and native recording proof

`experiments/video/control-probe.py` temporarily loads CS Demo Manager's library
around an isolated capture batch. The library is removed afterward, with the
original `gameinfo.gi` restored. It refuses an existing CSDM installation or
backup and checks the supplied binary hash before modifying game files.

Reference revision: `eaffbbfa0114ddc9c24ae474c54afea686f51d10` of
`akiver/cs-demo-manager` (MIT licence, copyright AkiVer).
`static/cs2/libserver.so`: 1,767,144 bytes, Git blob
`ded52242e4900cb9484efd411b64315be6f6f66b`, SHA-256
`c6bab67a072f83d7ec8e57c1fe243179e267bd9631cb4ee2d9cbf192cea86780`.
Downloaded to the scratch workspace, not vendored into this repository.

Results on the installed CS2 build:

- Tick actions successfully seek the demo. Account-ID spectator commands alone
  did not produce the right POV. `spec_mode 1` followed by
  `spec_player "<player name>"` selected the intended player. Name selection needs replacement with a verified stable
  slot mapping before production, particularly for duplicate/changing names.
- `host_framerate 30`, `startmovie` and `endmovie` produced 90 native TGA frames
  and a WAV for ticks 2700–2892. The library log confirms both boundary commands
  executed at their requested ticks. The setup seeks earlier and pauses briefly
  before recording to let the loading overlay clear.
- Encoding yielded exactly 3.000 seconds of 1280x720 H.264 video with AAC audio
  (audio duration 2.995 seconds, peak -0.6 dB). Visual review confirmed the player's
  Glock headshot; an independent demo event parse locates that
  kill at tick 2801. Artifact: `/tmp/omarcs-control-native-1/tick-bounded.mp4`.
- A repeat run passed the saved CLI's automatic frame-count, exact-boundary,
  stream and duration checks. The review clip was kept locally
  under `~/Videos/omarCS/`; detailed
  validation metadata is in
  `/tmp/omarcs-control-native-verified/native-validation.json`.
- Native frames/audio avoid deriving highlight boundaries from capture startup
  time. The current probe still records a separate wall-clock preview alongside
  them for diagnostics. A production renderer should remove that overhead.
- Cleanup verified: no remaining CS2 process, no temporary library directory or
  gameinfo backup, byte-identical restored gameinfo, and all 10 existing game
  config files restored. Raw files belonging to the probe are moved to its output
  directory before the temporary library directory is removed.

The library relies on game internals and may need updating for future CS2
versions. It requires `-insecure`; it is used only during explicit demo batches.
CS Demo Manager documents this Linux approach because CS2 has no VDM support and
its tools console path is Windows-only. Nothing schedules idle launches.

### Remaining feasibility and integration work

1. Extend the successful short-test desktop assessment to longer batches. The
   user confirmed normal responsiveness and no unexpected window or audio during
   the short tests; sustained rendering has not yet been assessed.
2. Test cancellation/failure recovery and sustained rendering. Current tooling
   keeps recovery backups but is not a production job manager; forced parent
   termination or power loss may require manual restoration.
3. Replace name-based spectator selection with roster-verified slot selection;
   handle death, reconnects and duplicate names. Validate more demo/game versions.
4. Move the proven native recording path into an explicit job runner with bounded
   disk use and per-clip validation. Native temporary frames use substantial disk
   space. Read tickrate from the demo instead of the probe's fixed 64 Hz assumption.
5. Implement highlight detection, demo retention, persisted jobs and the Highlights
   section described above. These are not implemented by the experiment scripts.

Sources:

- https://cs-demo-manager.com/docs/guides/video
- https://cs-demo-manager.com/docs/development/cs-server-plugin
- https://github.com/akiver/cs-demo-manager/blob/eaffbbfa0114ddc9c24ae474c54afea686f51d10/cs2-server-plugin/cs2-server-plugin/main.cpp
- https://wayland.pages.freedesktop.org/weston/toc/running-weston.html

## Integrated first version (2026-09-07)

- Match Report analysis version 5 includes deterministic candidates and unique
  roster-name eligibility. Enemy kills retain weapons in Match Facts.
- The panel includes per-match Highlights, individual render/play actions,
  top-five batches, cancellation, job states, tool setup and a clips-folder action.
- `video/highlights.py` owns persistent batch state and an exclusive lease. It
  drives native recording without the experimental wall-clock preview, validates
  observed ticks, frames, media streams and duration, then publishes MP4s.
- Retained demos survive successful automatic/manual download analysis. Existing
  local demos are reanalysed for version 5; missing historical files are not
  silently replaced or re-fetched on idle.
- A real round-15 3K was detected, rendered from the player's POV with game audio,
  and validated. A separate live cancellation test stopped the service, restored
  files and removed temporary frames. No CS2 process remained.
- Experimental scripts above preserve investigation history; the panel invokes
  `video/` via explicit `highlights-*` launcher commands.

Known limits: 64 Hz demos, x86_64 Linux, unique stable player names, up to 180
seconds per clip, and no automatic pruning of retained demos or completed MP4s.
The earlier slot-mapping and sustained-load work remain worthwhile follow-ups.

### Batch validation

The four-highlight Inferno batch completed all jobs (rounds 7, 15, 16 and 13),
including the 42-second clip. One earlier run stalled on the second CS2 startup;
changing end-of-clip handling from immediate termination to a scheduled game
`quit` and a graceful-exit wait allowed the original four-job regression scenario
to pass. The bounded service remains a cleanup fallback. This is evidence for
releasing game resources normally between jobs, not a guarantee against future
CS2 startup failures; such failures remain visible and retryable.

The worker reserves a private, non-listening CSDM port to avoid connecting to an
unrelated local command server. Disk preflight runs before modifying game files.
Tests cover existing-game refusal, exclusive leases, interrupted status without
launching processes, recovery ordering, config restoration, raw-frame preservation
during recovery, and disk refusal before preparation.

User validation after the integrated four-clip batch: the desktop remained
responsive and quiet, including during the 42-second clip. This extends the
earlier short-test acceptance on this machine. Final validation passed 47 Rust
unit tests, two integration tests, the real-demo golden Match Report test, six
Python lifecycle tests, launcher checks, QML lint and plugin manifest validation.
All four videos passed frame/tick/stream/duration checks; original game settings
were byte-compared after cleanup. The Steam auto-fetch service remains disabled
and inactive, and no rendering CS2 process remains.

## Clip presentation update

Analysis version 6 extends the tail from two to four seconds after the final kill.
If the player dies, keep up to two seconds of the death aftermath instead of
ending on that tick; never cross the round's official end. Native recording sets
`demo_ui_mode 0`, hiding the timeline and kill/death markers without cropping the
video or removing the gameplay HUD. The installed CS2 client's own command help
confirms that mode 0 disables the demo UI. All eight existing highlights were
re-rendered from their demos with these settings; the original files remain
preserved. A rendered Nuke frame confirms the bottom timeline is absent, and
native tick/frame validation confirms the extended durations. The real-demo
golden test passes with analysis version 6. One subsequent CS2 startup stalled
and timed out; explicitly retrying that clip succeeded.

## AWP discovery correction

Analysis version 7 recognizes `awp`, `weapon_awp`, and `AWP` across kill, shot,
and tick data. Previously the flick detector compared all three sources against
`awp`, excluding actual demo shots and held-weapon observations. The existing
eight-degree / twelve-tick flick threshold is unchanged. A separate AWP double
kill category includes two enemy AWP kills in the same round within fifteen
seconds; other weapons retain the five-second quick-double window.

The four recovered Mirage demos contain three AWP doubles within that window:
round 21 of match 25023598364dc92d (643 ticks), and rounds 10 (757 ticks) and 18
(734 ticks) of ae967e2d4648d9b4. Regression tests use the observed weapon spellings
and exercise the fifteen-second boundary.

## Rendering suspended: settings leak

Rendering was disabled after a reported persistent 30 FPS cap (superseded by the offline sandbox below). The old
renderer launched CS2 with `+fps_max 30` and shared the real Steam/CS2 settings.
Backup/restore did not provide a sufficient isolation guarantee. The FPS launch
override has been removed, and render/start/worker/game entry points reject new
work before recovery or process launch. Existing clips remain playable. Do not
re-enable until settings writes and Steam Cloud effects are isolated and verified.
The exact persistence path has not been established; no live game was launched
and no current user settings were changed during this containment fix.

## Offline settings isolation (2026-09-08)

The production renderer now runs a separate offline Steam client and CS2 inside
bubblewrap. The original home directory is a read-only overlay lower layer;
all Steam settings, CS2 loading changes and runtime writes copy into disposable
upper storage. Only the job's output directory is a writable host bind. The rest
of the host filesystem is read-only. Game setup refuses to run unless mount,
PID, network and IPC namespaces all differ from those captured by the driver.

The sandbox has private `/run`, `/tmp`, `/dev/shm` and `~/.steam`, hiding the live
Steam command FIFO, token, PID and IPC paths. Only GPU device nodes are passed
through; no audio hardware is available. Network access is absent, so the private
client cannot write to Steam Cloud. Cached offline-login preferences are changed
only inside the overlay. The original desktop Steam client remains running.

Recording still uses `host_framerate 30` for native output timing inside the
private game. No `fps_max` override is passed. Cleanup stops the whole owned
service and deletes its upper/work directories, including on cancellation.
It never copies old configurations back. Legacy recovery records are rejected
for manual review rather than applied over current settings.

Validation:
- An offline prototype captured 90 frames over three seconds.
- Full Mirage AWP clips were encoded and validated against their requested ticks.
  The stricter device/shared-memory sandbox rendered round 21, observed ticks
  113437–114529, 512 frames, 17.067 seconds, H.264 video and AAC audio.
- A live cancellation during the next CS2 session stopped both private processes,
  removed the overlay/raw data and preserved completed clips.
- 52 original CS2/Steam files (including gameinfo, user settings and loginusers)
  were hashed before and after successful rendering and cancellation; none changed.
  No rendering CS2 remained, and the original desktop Steam PID remained running.
- Ten Python tests pass, including real bubblewrap writes/deletes/recreates,
  forced SIGKILL, hidden Steam FIFO and POSIX shared memory, host-loopback
  inaccessibility, namespace rejection and recovery without configuration writes.

Current limits: bubblewrap overlay support, Steam/CS2 under HOME, cached Steam
offline login and roughly 25 seconds of private Steam startup per clip. The
previous in-place renderer and experiment scripts are historical, not fallback
paths. Do not remove the sandbox to work around startup errors.
