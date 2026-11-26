#!/usr/bin/env python3
"""Temporary CSDM playback-control probe, around the isolated capture CLI.

Uses a locally supplied, pinned upstream binary. Never downloads or persists a
game plugin automatically. See README.md for provenance and remaining limits.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import uuid


BINARY_SHA256 = 'c6bab67a072f83d7ec8e57c1fe243179e267bd9631cb4ee2d9cbf192cea86780'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--demo', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--weston-prefix', type=Path, required=True)
    parser.add_argument('--steam', type=Path, default=Path.home() / '.local/share/Steam')
    parser.add_argument('--account-id', type=int, required=True)
    parser.add_argument('--player-name', help='Demo roster name for direct spectator selection')
    parser.add_argument('--start-tick', type=int, default=5000)
    parser.add_argument('--native-seconds', type=int, default=0,
                        help='Also record 1–5 seconds of tick-bounded native frames, after 128 setup ticks')
    args = parser.parse_args()
    if args.player_name and any(c in args.player_name for c in '\";\n\r\\'):
        parser.error('player name contains unsupported console characters')
    if not 0 < args.account_id < 2**32 or args.start_tick <= 96:
        parser.error('provide a valid account ID and a start tick greater than 96')
    if not 0 <= args.native_seconds <= 5:
        parser.error('native-seconds must be 0–5')
    if args.native_seconds and not args.player_name:
        parser.error('native capture requires the verified demo roster name')
    if subprocess.run(['pgrep', '-x', 'cs2'], capture_output=True).returncode != 1:
        parser.error('CS2 must be closed')
    binary = args.library.read_bytes()
    if hashlib.sha256(binary).hexdigest() != BINARY_SHA256:
        parser.error('library does not match the reviewed upstream revision')
    game = args.steam.resolve() / 'steamapps/common/Counter-Strike Global Offensive/game/csgo'
    info = game / 'gameinfo.gi'
    backup = game / 'gameinfo.gi.backup'
    plugin = game / 'csdm'
    original = info.read_bytes()
    if backup.exists() or plugin.exists() or b'csgo/csdm' in original:
        parser.error('existing CSDM installation or gameinfo backup; refusing to replace it')
    if b'Game\tcsgo' not in original:
        parser.error('gameinfo layout is not recognised')
    modified = original.replace(b'Game\tcsgo', b'Game\tcsgo/csdm\n\t\t\tGame\tcsgo', 1)
    root = args.output_dir.resolve()
    root.mkdir(parents=True, exist_ok=False)
    (root / 'gameinfo.gi.original').write_bytes(original)
    demo = root / 'probe.dem'
    try:
        os.link(args.demo.resolve(), demo)
    except OSError:
        shutil.copyfile(args.demo.resolve(), demo)
    tick = args.start_tick
    movie = 'omarcs_probe_' + uuid.uuid4().hex
    actions = [{'tick': 96, 'cmd': f'demo_gototick {tick}'}]
    for at in (tick, tick + 64):
        actions.extend([
            {'tick': at, 'cmd': 'spec_mode 1'},
            {'tick': at, 'cmd': f'spec_lock_to_accountid {args.account_id}'},
            {'tick': at, 'cmd': f'spec_player_by_accountid {args.account_id}'},
        ])
        if args.player_name:
            actions.append({'tick': at, 'cmd': f'spec_player "{args.player_name}"'})
    actions.append({'tick': tick, 'cmd': 'demoui'})
    if args.native_seconds:
        actions.extend([
            {'tick': tick, 'cmd': 'sv_cheats 1'},
            {'tick': tick, 'cmd': 'host_framerate 30'},
            {'tick': tick + 124, 'cmd': 'pause_playback'},
            {'tick': tick + 128, 'cmd': f'startmovie {movie}'},
            {'tick': tick + 128 + args.native_seconds * 64, 'cmd': 'endmovie'},
            {'tick': tick + 129 + args.native_seconds * 64, 'cmd': 'host_framerate 0'},
        ])
    actions.sort(key=lambda action: action['tick'])
    Path(str(demo) + '.json').write_text(json.dumps([{'actions': actions}], indent=2))
    installed = False
    game_log = game.parent / 'bin/linuxsteamrt64/csdm.log'
    old_log = game_log.read_bytes() if game_log.exists() else None
    def interrupted(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupted)
    try:
        plugin.mkdir()
        installed = True
        target = plugin / 'bin/linuxsteamrt64/libserver.so'
        target.parent.mkdir(parents=True)
        target.write_bytes(binary)
        with backup.open('xb') as handle:
            handle.write(original)
        info.write_bytes(modified)
        child = subprocess.Popen([
            sys.executable, str(Path(__file__).with_name('capture-demo.py').resolve()),
            '--demo', str(demo), '--output-dir', str(root / 'capture'),
            '--weston-prefix', str(args.weston_prefix.resolve()),
            '--steam', str(args.steam.resolve()), '--skip', '30', '--duration', '10'])
        try:
            if child.wait() != 0:
                raise RuntimeError('Isolated capture failed; inspect its logs')
        except BaseException:
            signal.signal(signal.SIGINT, signal.SIG_IGN)
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=30)
            raise
    finally:
        if subprocess.run(['pgrep', '-x', 'cs2'], capture_output=True).returncode != 1:
            raise RuntimeError(f'CS2 is still running; recovery original: {root / "gameinfo.gi.original"}')
        current = info.read_bytes()
        if current not in (original, modified):
            raise RuntimeError(f'gameinfo changed externally; recovery original: {root / "gameinfo.gi.original"}')
        if game_log.exists():
            (root / 'csdm.log').write_bytes(game_log.read_bytes())
            if old_log is None:
                game_log.unlink()
            else:
                game_log.write_bytes(old_log)
        if args.native_seconds:
            raw = root / 'native'
            raw.mkdir(exist_ok=True)
            for folder in [plugin, game / 'movie']:
                if folder.exists():
                    for path in folder.rglob(movie + '*'):
                        if path.is_file():
                            shutil.move(str(path), str(raw / path.name))
        info.write_bytes(original)
        if backup.exists():
            if backup.read_bytes() != original:
                raise RuntimeError('gameinfo backup changed externally; leaving it intact')
            backup.unlink()
        if installed:
            shutil.rmtree(plugin)
        print('Temporary CSDM library removed; original gameinfo restored.', flush=True)
    if args.native_seconds:
        raw = root / 'native'
        frames = sorted(raw.glob(movie + '*.tga'))
        wav = raw / (movie + '.wav')
        expected = args.native_seconds * 30
        if len(frames) != expected or not wav.is_file():
            raise RuntimeError(f'Incomplete native recording: {len(frames)}/{expected} frames; audio={wav.exists()}')
        log = (root / 'csdm.log').read_text()
        start = re.search(r'\[(\d+)\] Executed: startmovie ' + movie, log)
        end = re.search(r'\[(\d+)\] Executed: endmovie', log)
        if not start or not end:
            raise RuntimeError('Native recording boundaries were not confirmed in the game log')
        observed = [int(start[1]), int(end[1])]
        requested = [tick + 128, tick + 128 + args.native_seconds * 64]
        if observed != requested:
            raise RuntimeError(f'Tick boundaries differ: requested {requested}, observed {observed}')
        output = root / 'tick-bounded.mp4'
        subprocess.run([
            'ffmpeg', '-v', 'error', '-framerate', '30', '-i', str(raw / (movie + '%08d.tga')),
            '-i', str(wav), '-c:v', 'libx264', '-preset', 'fast', '-threads', '2',
            '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-n', str(output)], check=True, timeout=30)
        probe = json.loads(subprocess.check_output([
            'ffprobe', '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(output)], text=True))
        if {s['codec_type'] for s in probe['streams']} != {'audio', 'video'}:
            raise RuntimeError('Native output is missing an audio or video stream')
        if abs(float(probe['format']['duration']) - args.native_seconds) > 0.1:
            raise RuntimeError('Native output duration is incorrect')
        (root / 'native-validation.json').write_text(json.dumps(dict(
            requestedTicks=requested, observedTicks=observed, frameCount=len(frames),
            playerName=args.player_name, accountId=args.account_id,
            librarySha256=BINARY_SHA256, probe=probe), indent=2))
        print(f'Tick-bounded native clip: {output}', flush=True)


if __name__ == '__main__':
    main()
