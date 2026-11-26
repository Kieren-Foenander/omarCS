#!/usr/bin/env python3
"""Throwaway feasibility probe: capture synthetic video/audio on a private display.

Never launches Steam or CS2. Does not change desktop configuration.
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def inside(output):
    env = os.environ.copy()
    if not env.get('DISPLAY') or env['DISPLAY'] == env.get('OMARCS_REAL_DISPLAY'):
        raise RuntimeError('Refusing capture: X display is not isolated')
    if env.get('WAYLAND_DISPLAY') != env['OMARCS_PRIVATE_SOCKET']:
        raise RuntimeError('Refusing capture: Wayland display is not isolated')
    env.pop('LD_LIBRARY_PATH', None)
    env.pop('WESTON_MODULE_MAP', None)
    env['SDL_VIDEODRIVER'] = 'x11'
    env['SDL_AUDIODRIVER'] = 'pulseaudio'
    children = []
    tone = None
    try:
        children.append(subprocess.Popen([
            'ffplay', '-loglevel', 'error', '-f', 'lavfi', '-i',
            'testsrc2=size=1280x720:rate=30', '-an', '-noborder',
            '-window_title', 'omarcs-capture-probe', '-t', '15'], env=env))
        fd, tone_path = tempfile.mkstemp(prefix='omarcs-tone-', suffix='.pcm')
        os.close(fd)
        tone = Path(tone_path)
        subprocess.run(['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i',
                        'sine=frequency=440:sample_rate=48000', '-t', '30',
                        '-f', 's16le', '-y', str(tone)], check=True, timeout=5)
        children.append(subprocess.Popen([
            'pacat', '--playback', '--device=' + env['PULSE_SINK'],
            '--format=s16le', '--rate=48000', '--channels=1'], env=env, stdin=tone.open('rb')))
        window = None
        for _ in range(50):
            found = subprocess.run(['xdotool', 'search', '--name', '^omarcs-capture-probe$'],
                                   env=env, capture_output=True, text=True)
            if found.returncode == 0:
                window = found.stdout.splitlines()[0]
                break
            time.sleep(0.1)
        if window is None:
            raise RuntimeError('The isolated test window did not appear')
        time.sleep(1)  # Let PulseAudio establish the private playback stream.
        if any(child.poll() is not None for child in children):
            raise RuntimeError('A test player exited before capture')
        subprocess.run([
            'ffmpeg', '-hide_banner', '-loglevel', 'error', '-thread_queue_size', '512',
            '-f', 'pulse', '-i', env['PULSE_SINK'] + '.monitor', '-thread_queue_size', '512', '-f', 'x11grab',
            '-window_id', window, '-video_size', '1280x720', '-framerate', '30',
            '-i', env['DISPLAY'],
            '-t', '4', '-c:v', 'libx264', '-preset', 'ultrafast', '-threads', '2',
            '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-n', str(output)],
            env=env, check=True, timeout=15)
    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        if tone is not None:
            tone.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--weston-prefix', type=Path,
                        help='Optional extracted distro package root (containing usr/bin/weston)')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    output = args.output.resolve()
    if args.inside:
        inside(output)
        return
    if output.exists():
        parser.error('output already exists; choose a new filename')
    for tool in ['ffplay', 'ffmpeg', 'ffprobe', 'pactl', 'pacat', 'xdotool']:
        if not shutil.which(tool):
            parser.error(f'missing tool: {tool}')
    env = os.environ.copy()
    weston = shutil.which('weston')
    shell = 'kiosk-shell.so'
    if args.weston_prefix:
        prefix = args.weston_prefix.resolve()
        weston = str(prefix / 'usr/bin/weston')
        lib = prefix / 'usr/lib'
        env['LD_LIBRARY_PATH'] = f'{lib}:{lib}/weston'
        env['WESTON_MODULE_MAP'] = ';'.join(f'{p.name}={p}' for p in lib.rglob('*.so'))
        shell = str(lib / 'weston/kiosk-shell.so')
    if not weston or not Path(weston).is_file():
        parser.error('Weston is required')
    output.parent.mkdir(parents=True, exist_ok=True)
    job = f'omarcs-probe-{os.getpid()}'
    env.update(OMARCS_REAL_DISPLAY=env.get('DISPLAY', ''),
               OMARCS_PRIVATE_SOCKET=job, PULSE_SINK=job)
    # Pin the compositor to Mesa/Intel on the development machine. The log is
    # authoritative; do not infer actual GPU selection from environment alone.
    if Path('/dev/dri/by-path/pci-0000:00:02.0-render').exists():
        env['DRI_PRIME'] = 'pci-0000_00_02_0'
        env['__EGL_VENDOR_LIBRARY_FILENAMES'] = '/usr/share/glvnd/egl_vendor.d/50_mesa.json'
    module = subprocess.check_output([
        'pactl', 'load-module', 'module-null-sink', f'sink_name={job}',
        'sink_properties=device.description=omarCS-capture-probe'], text=True).strip()
    weston_process = None
    try:
        with tempfile.TemporaryDirectory(prefix=job) as scratch:
            log = output.with_suffix('.weston.log')
            weston_process = subprocess.Popen([
                weston, '--backend=headless', '--renderer=gl', f'--shell={shell}',
                f'--socket={job}', '--no-config', '--width=1280', '--height=720',
                '--idle-time=0', '--xwayland', '--no-xwm-decorations', '--fake-seat',
                f'--log={log}', '--', sys.executable, str(Path(__file__).resolve()),
                '--inside', '--output', str(output)], env=env, start_new_session=True)
            status = weston_process.wait(timeout=30)
            if status != 0:
                raise RuntimeError(f'Weston failed ({status}); see {log}')
            probe = subprocess.check_output([
                'ffprobe', '-v', 'error', '-show_entries',
                'stream=codec_type,codec_name,width,height:format=duration',
                '-of', 'json', str(output)], text=True)
            streams = json.loads(probe)['streams']
            if {s['codec_type'] for s in streams} != {'audio', 'video'}:
                raise RuntimeError('Capture is missing video or audio')
            volume = subprocess.run([
                'ffmpeg', '-hide_banner', '-i', str(output), '-af', 'volumedetect',
                '-vn', '-f', 'null', os.devnull], capture_output=True, text=True,
                check=True, timeout=10)
            peak = re.search(r'max_volume: ([-\d.]+) dB', volume.stderr)
            if peak is None or float(peak.group(1)) < -60:
                raise RuntimeError('Private-sink capture is silent')
            print(probe)
            print(f'Private test tone peak: {peak.group(1)} dB')
    finally:
        if weston_process is not None:
            # Also terminate orphaned test clients if the compositor failed.
            try:
                os.killpg(weston_process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                weston_process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(weston_process.pid, signal.SIGKILL)
                weston_process.wait()
        subprocess.run(['pactl', 'unload-module', module], check=True)


if __name__ == '__main__':
    main()
