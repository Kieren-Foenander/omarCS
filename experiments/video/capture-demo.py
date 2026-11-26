#!/usr/bin/env python3
"""Explicit, bounded CS2 capture experiment. Not a highlight renderer yet."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid


def run(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def no_existing_game():
    result = subprocess.run(['pgrep', '-x', 'cs2'], capture_output=True)
    if result.returncode != 1:
        raise RuntimeError('Refusing to launch: CS2 is running or its state is unknown')


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def capture(job):
    env = os.environ.copy()
    if (env.get('OMARCS_PRIVATE_MEDIA') != '1'
            or env.get('WAYLAND_DISPLAY') != job['socket']
            or not env.get('DISPLAY')
            or env['DISPLAY'] == job['desktop_display']):
        raise RuntimeError('Refusing capture: private display/media environment missing')
    sinks = json.loads(subprocess.check_output(['pactl', '-f', 'json', 'list', 'sinks'], text=True))
    if [sink['name'] for sink in sinks] != ['omarcs_private_default']:
        raise RuntimeError('Refusing capture: unexpected sinks on the private media server')
    no_existing_game()
    root = Path(job['directory'])
    for name in ['LD_LIBRARY_PATH', 'WESTON_MODULE_MAP']:
        env.pop(name, None)
    env.update(SDL_VIDEO_DRIVER='wayland', SDL_VIDEODRIVER='wayland',
               SDL_AUDIO_DRIVER='pulseaudio', SDL_AUDIODRIVER='pulseaudio',
               SteamAppId='730', SteamGameId='730')
    common = Path(job['steam']) / 'steamapps/common'
    children = []
    with (root / 'game.log').open('w') as game_log, (root / 'capture.log').open('w') as log:
        try:
            children.append(subprocess.Popen([
                str(common / 'SteamLinuxRuntime_sniper/_v2-entry-point'),
                '--verb=waitforexitandrun', '--',
                str(common / 'Counter-Strike Global Offensive/game/cs2.sh'),
                '-insecure', '-novid', '-sw', '-width', '1280', '-height', '720',
                '+fps_max', '30', '+snd_mute_losefocus', '0', '+playdemo', job['demo']],
                env=env, stdout=game_log, stderr=subprocess.STDOUT))
            time.sleep(5)
            gst = subprocess.Popen([
                'gst-launch-1.0', '-q', 'matroskamux', 'name=mux', 'streamable=true',
                '!', 'fdsink', 'fd=1', 'pipewiresrc', 'autoconnect=false',
                'stream-properties=props,node.name=omarcs-private-capture', '!',
                'videoconvert', '!', 'videoscale', '!',
                'video/x-raw,format=I420,width=1280,height=720', '!',
                'jpegenc', 'quality=85', '!', 'queue', '!', 'mux.video_0',
                'pulsesrc', 'device=omarcs_private_default.monitor', '!',
                'audioconvert', '!', 'audio/x-raw,format=S16LE,rate=48000,channels=2',
                '!', 'queue', '!', 'mux.audio_0'], env=env, stdout=subprocess.PIPE, stderr=log)
            children.append(gst)
            encoder = subprocess.Popen([
                'ffmpeg', '-hide_banner', '-f', 'matroska', '-i', 'pipe:0',
                '-ss', str(job['skip']), '-t', str(job['duration']),
                '-c:v', 'libx264', '-preset', 'ultrafast', '-threads', '2',
                '-c:a', 'aac', '-n', str(root / 'sample.mp4')],
                stdin=gst.stdout, stdout=log, stderr=log, env=env)
            children.append(encoder)
            gst.stdout.close()
            for _ in range(50):
                ports = subprocess.check_output(['pw-link', '-i'], text=True)
                if 'omarcs-private-capture:input_1' in ports:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('Private video input did not appear')
            run(['pw-link', 'weston.pipewire:output_1', 'omarcs-private-capture:input_1'])
            if encoder.wait(timeout=job['skip'] + job['duration'] + 30) != 0:
                raise RuntimeError('Encoding failed; see capture.log')
            (root / 'capture-complete').touch()
        finally:
            for process in reversed(children):
                stop(process)
            # Steam containers create additional process groups. The outer
            # systemd service owns and cleans those before configs are restored.


def compositor(job, job_file):
    if os.environ.get('OMARCS_PRIVATE_MEDIA') != '1':
        raise RuntimeError('Private media wrapper is required')
    prefix = Path(job['weston_prefix'])
    lib = prefix / 'usr/lib'
    env = os.environ.copy()
    env.update(LD_LIBRARY_PATH=f'{lib}:{lib}/weston',
               WESTON_MODULE_MAP=';'.join(f'{p.name}={p}' for p in lib.rglob('*.so')))
    root = Path(job['directory'])
    config = root / 'weston.ini'
    config.write_text('[output]\nname=pipewire\nmode=1280x720@30\n')
    run([str(prefix / 'usr/bin/weston'), '--backends=headless,pipewire',
         '--no-outputs', '--fake-seat', '--renderer=gl',
         f'--shell={lib}/weston/kiosk-shell.so', f'--socket={job["socket"]}',
         f'--config={config}', '--width=1280', '--height=720', '--idle-time=0',
         '--xwayland', '--no-xwm-decorations', f'--log={root / "weston.log"}',
         '--', sys.executable, str(Path(__file__).resolve()),
         '--stage', 'capture', '--job', str(job_file)], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', type=Path)
    parser.add_argument('--output-dir', type=Path, help='New directory for clip, logs and recovery backups')
    parser.add_argument('--weston-prefix', type=Path)
    parser.add_argument('--steam', type=Path, default=Path.home() / '.local/share/Steam')
    parser.add_argument('--skip', type=int, default=30, help='Seconds of capture to discard; NOT demo ticks')
    parser.add_argument('--duration', type=int, default=10)
    parser.add_argument('--stage', choices=['compositor', 'capture'], help=argparse.SUPPRESS)
    parser.add_argument('--job', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.stage:
        job = json.loads(args.job.read_text())
        return capture(job) if args.stage == 'capture' else compositor(job, args.job)
    if not all([args.demo, args.output_dir, args.weston_prefix]):
        parser.error('--demo, --output-dir and --weston-prefix are required')
    if not 0 <= args.skip <= 120 or not 1 <= args.duration <= 30:
        parser.error('skip must be 0–120 seconds and duration 1–30 seconds')
    for tool in ['systemd-run', 'systemctl', 'pgrep', 'gst-launch-1.0', 'ffmpeg',
                 'ffprobe', 'pw-link', 'pactl', 'pipewire', 'pipewire-pulse', 'wireplumber']:
        if not shutil.which(tool):
            parser.error(f'missing tool: {tool}')
    for path in [args.demo, args.weston_prefix / 'usr/bin/weston',
                 args.steam / 'steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point',
                 args.steam / 'steamapps/common/Counter-Strike Global Offensive/game/cs2.sh']:
        if not path.is_file():
            parser.error(f'missing file: {path}')
    no_existing_game()
    if subprocess.run(['pgrep', '-x', 'steam'], capture_output=True).returncode != 0:
        parser.error('Steam must already be running and signed in')
    # Keep the lock outside the transient service and hold it through restore.
    with (Path(os.environ['XDG_RUNTIME_DIR']) / 'omarcs-video.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        root = args.output_dir.resolve()
        root.mkdir(parents=True, exist_ok=False)
        userdata = args.steam.resolve() / 'userdata'
        configs = [p for p in userdata.glob('*/730/local/cfg/*') if p.is_file()]
        backup = root / 'config-backup'
        for path in configs:
            target = backup / path.relative_to(userdata)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
        unit = 'omarcs-video-' + uuid.uuid4().hex
        job = dict(directory=str(root), demo=str(args.demo.resolve()),
                   steam=str(args.steam.resolve()), weston_prefix=str(args.weston_prefix.resolve()),
                   socket=unit, desktop_display=os.environ.get('DISPLAY', ''),
                   skip=args.skip, duration=args.duration)
        job_file = root / 'job.json'
        job_file.write_text(json.dumps(job, indent=2))
        print(f'Explicit capture batch: {unit}; logs: {root}', flush=True)
        def interrupted(signum, frame):
            raise KeyboardInterrupt
        signal.signal(signal.SIGTERM, interrupted)
        try:
            with (root / 'session.log').open('w') as log:
                run(['systemd-run', '--user', '--wait', '--pipe', '--collect',
                     '--unit=' + unit, '--property=RuntimeMaxSec=' + str(args.skip + args.duration + 60),
                     '--property=TimeoutStopSec=5', '--property=KillMode=control-group',
                     '--property=CPUQuota=400%', '--property=MemoryMax=12G',
                     '--property=MemorySwapMax=0', '--property=Nice=10',
                     sys.executable, str(Path(__file__).with_name('private-media.py').resolve()),
                     '--', sys.executable, str(Path(__file__).resolve()),
                     '--stage', 'compositor', '--job', str(job_file)], stdout=log, stderr=subprocess.STDOUT)
        finally:
            # Synchronous stop kills the entire cgroup, even when a Steam
            # wrapper exited early. Never restore while our game is still alive.
            signal.signal(signal.SIGINT, signal.SIG_IGN)
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            subprocess.run(['systemctl', '--user', 'stop', unit + '.service'], capture_output=True)
            active = subprocess.run(['systemctl', '--user', 'is-active', unit + '.service'],
                                    capture_output=True, text=True)
            if active.stdout.strip() not in ('inactive', 'failed', 'unknown'):
                raise RuntimeError(f'Service cleanup unconfirmed; recovery backups remain at {backup}')
            if subprocess.run(['pgrep', '-x', 'cs2'], capture_output=True).returncode != 1:
                raise RuntimeError(f'CS2 is running; config restoration deferred. Backups: {backup}')
            for path in configs:
                shutil.copy2(backup / path.relative_to(userdata), path)
        if not (root / 'capture-complete').exists():
            raise RuntimeError(f'Capture did not finish; see {root}')
        probe = json.loads(subprocess.check_output([
            'ffprobe', '-v', 'error', '-show_streams', '-show_format', '-of', 'json',
            str(root / 'sample.mp4')], text=True))
        (root / 'probe.json').write_text(json.dumps(probe, indent=2))
        if {s['codec_type'] for s in probe['streams']} != {'audio', 'video'}:
            raise RuntimeError('Output is missing video or audio')
        if abs(float(probe['format']['duration']) - args.duration) > 1:
            raise RuntimeError('Output duration differs from the requested capture')
        print(f'Captured {root / "sample.mp4"}. Review POV, loading screens and audio manually.')


if __name__ == '__main__':
    main()
