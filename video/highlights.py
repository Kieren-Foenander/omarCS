#!/usr/bin/env python3
"""Explicit local highlight batches. Status/cancel never start Steam or CS2."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import platform
import uuid
import isolation

HOME = Path.home()
STATE = Path(os.environ.get('XDG_STATE_HOME', HOME / '.local/state')) / 'omarcs'
DATA = Path(os.environ.get('XDG_DATA_HOME', HOME / '.local/share')) / 'omarcs'
ROOT = STATE / 'highlights'
HERE = Path(__file__).resolve().parent
STEAM = Path(os.environ.get('OMARCS_STEAM', HOME / '.local/share/Steam')).resolve()
GAME = STEAM / 'steamapps/common/Counter-Strike Global Offensive/game'
REV = 'eaffbbfa0114ddc9c24ae474c54afea686f51d10'
LIB_HASH = 'c6bab67a072f83d7ec8e57c1fe243179e267bd9631cb4ee2d9cbf192cea86780'


def require_safe_rendering():
    if not shutil.which('bwrap'):
        raise RuntimeError('Rendering requires bubblewrap for offline settings isolation.')
    if not STEAM.is_relative_to(HOME) or not GAME.is_relative_to(STEAM):
        raise RuntimeError('Offline rendering currently requires Steam and CS2 under HOME.')



def atomic(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(path.name + '.' + uuid.uuid4().hex)
    temp.write_text(json.dumps(value, indent=2))
    os.replace(temp, path)


def read(path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


def digest(path):
    with path.open('rb') as handle:
        return hashlib.file_digest(handle, 'sha256').hexdigest()


def game_closed():
    if subprocess.run(['pgrep', '-x', 'cs2'], capture_output=True).returncode != 1:
        raise RuntimeError('Close CS2 before rendering highlights. Your running game has been left alone.')


def run(cmd, **kwargs):
    return subprocess.run(cmd, check=True, **kwargs)


def locked(path, blocking=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    handle = path.open('a+')
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
    except BaseException:
        handle.close()
        raise
    return handle


def status():
    value = read(ROOT / 'status.json', {'state': 'idle', 'jobs': []})
    try:
        with locked(ROOT / 'batch.lock'):
            if value.get('state') in ('queued', 'rendering', 'cancelling'):
                value['state'] = 'interrupted'
                value['message'] = 'Batch interrupted. Start a new batch to recover and retry.'
                for job in value.get('jobs', []):
                    if job['state'] in ('queued', 'rendering'):
                        job['state'] = 'interrupted'
    except BlockingIOError:
        pass
    value['clips'] = {key: clip for key, clip in read(ROOT / 'clips.json', {}).items() if Path(clip['path']).is_file()}
    if value.get('state') in ('queued', 'rendering') and cancelled(value['batchId']):
        value.update(state='cancelling', message='Stopping the private recording session…')
    value['toolsReady'] = (DATA / 'video/libserver.so').is_file() and (DATA / 'video/weston/usr/bin/weston').is_file()
    return value


def load_match(match_id):
    with sqlite3.connect(f'file:{STATE / "omarcs.db"}?mode=ro', uri=True) as db:
        row = db.execute('SELECT payload FROM matches WHERE id = ?', (match_id,)).fetchone()
    if not row:
        raise RuntimeError('Match not found. Refresh demos first.')
    report = json.loads(row[0])
    if not report.get('highlights'):
        raise RuntimeError('No highlights for this match. Refresh demos to analyse retained files.')
    if not Path(report['path']).is_file():
        raise RuntimeError('The source demo is missing. Download or import it again, then refresh demos.')
    name = report['player']['name']
    if not name or any(ord(c) < 32 or c in '\";\\' for c in name) or not report.get('highlightRenderNameUnique', False):
        raise RuntimeError('This demo has an ambiguous or unsupported player name; safe POV selection is unavailable.')
    return report


def dependencies():
    for tool in ['systemd-run', 'systemctl', 'pgrep', 'ffmpeg', 'ffprobe', 'pipewire',
                 'wireplumber', 'pipewire-pulse', 'pactl', 'Xwayland', 'bwrap']:
        if not shutil.which(tool):
            raise RuntimeError(f'Rendering needs {tool}. See the Highlights setup instructions.')
    prefix = Path(os.environ.get('OMARCS_WESTON_PREFIX', DATA / 'video/weston'))
    weston = prefix / 'usr/bin/weston'
    if not weston.is_file():
        raise RuntimeError('Video tools are not set up. Run omarcs-plugin highlights-setup once.')
    library = DATA / 'video/libserver.so'
    if not library.is_file() or digest(library) != LIB_HASH:
        raise RuntimeError('The verified playback library is missing. Run highlights-setup.')
    return prefix, library


def stop_service(unit):
    subprocess.run(['systemctl', '--user', 'stop', unit + '.service'], capture_output=True)
    state = subprocess.run(['systemctl', '--user', 'is-active', unit + '.service'], capture_output=True, text=True).stdout.strip()
    if state not in ('inactive', 'failed', 'unknown'):
        raise RuntimeError('Could not confirm rendering stopped; recovery backups have been retained.')


def recover():
    recovery = ROOT / 'recovery.json'
    record = read(recovery, None)
    if not record:
        return
    if record.get('isolationVersion') != 1:
        raise RuntimeError('Legacy recovery needs manual review. Current CS2 settings have been left untouched.')
    stop_service(record['unit'])
    folder = Path(record['folder']).resolve()
    if folder.parent != (ROOT / 'work').resolve():
        raise RuntimeError('Unexpected rendering recovery directory')
    isolation.discard(folder)
    recovery.unlink()


def prepare(report, highlight, folder, prefix, library, original_namespaces):
    isolation.require_private(original_namespaces)
    info = GAME / 'csgo/gameinfo.gi'
    plugin = GAME / 'csgo/csdm'
    if plugin.exists() or (GAME / 'csgo/gameinfo.gi.backup').exists():
        raise RuntimeError('An existing CS Demo Manager installation must be closed and removed before rendering.')
    original = info.read_bytes()
    if b'csgo/csdm' in original or b'Game\tcsgo' not in original:
        raise RuntimeError('Game loading configuration is not recognised; it has been left unchanged.')
    modified = original.replace(b'Game\tcsgo', b'Game\tcsgo/csdm\n\t\t\tGame\tcsgo', 1)
    folder.mkdir(parents=True)
    (folder / 'raw').mkdir()
    (folder / 'gameinfo.original').write_bytes(original)
    (folder / 'gameinfo.modified').write_bytes(modified)
    movie = 'omarcs_' + uuid.uuid4().hex
    unit = 'omarcs-highlight-' + uuid.uuid4().hex
    record = dict(unit=unit, folder=str(folder), game=str(GAME), movie=movie,
                  isolationVersion=1)
    start, end = highlight['startTick'], highlight['endTick']
    if highlight['tickRate'] != 64 or not 129 <= start < end or end - start > 180 * 64:
        raise RuntimeError('This highlight exceeds supported recording limits (64 Hz, up to 180 seconds).')
    demo = folder / 'source.dem'
    try:
        os.link(report['path'], demo)
    except OSError:
        shutil.copyfile(report['path'], demo)
    setup = start - 64
    actions = [{'tick': 1, 'cmd': f'demo_gototick {setup - 1}'}]
    for tick in [setup, start - 8]:
        actions += [{'tick': tick, 'cmd': 'spec_mode 1'},
                    {'tick': tick, 'cmd': 'spec_player "' + report['player']['name'] + '"'}]
    actions += [
        {'tick': setup, 'cmd': 'sv_cheats 1'},
        {'tick': setup, 'cmd': 'host_framerate 30'},
        {'tick': setup, 'cmd': 'cl_hud_telemetry_frametime_show 0'},
        {'tick': setup, 'cmd': 'cl_trueview_show_status 0'},
        {'tick': setup, 'cmd': 'r_show_build_info 0'},
        {'tick': setup, 'cmd': 'demo_ui_mode 0'},
        {'tick': start - 4, 'cmd': 'pause_playback'},
        {'tick': start, 'cmd': f'startmovie {movie}'},
        {'tick': end, 'cmd': 'endmovie'},
        {'tick': end + 4, 'cmd': 'quit'},
    ]
    atomic(Path(str(demo) + '.json'), [{'actions': sorted(actions, key=lambda a: a['tick'])}])
    plugin.mkdir()
    (plugin / '.omarcs-owner').write_text(unit)
    target = plugin / 'bin/linuxsteamrt64/libserver.so'
    target.parent.mkdir(parents=True)
    shutil.copy2(library, target)
    # Keep native frames outside the temporary game installation and memory-backed /tmp.
    (plugin / 'movie').symlink_to(folder / 'raw', target_is_directory=True)
    (GAME / 'csgo/gameinfo.gi.backup').write_bytes(original)
    info.write_bytes(modified)
    job = dict(record, demo=str(demo), socket=unit, prefix=str(prefix),
               start=start, end=end, desktop=os.environ.get('DISPLAY', ''))
    atomic(folder / 'job.json', job)
    return job


def cancelled(batch):
    return (ROOT / ('cancel-' + batch)).exists()


def render_clip(report, highlight, folder, prefix, library, batch):
    seconds = (highlight['endTick'] - highlight['startTick']) / 64
    required_space = seconds * 1280 * 720 * 3 * 30 * 1.2 + Path(report['path']).stat().st_size + 2 * 1024**3
    if shutil.disk_usage(ROOT).free < required_space:
        raise RuntimeError('Not enough free disk space for the demo and temporary video frames.')
    folder.mkdir(parents=True)
    output_folder = folder / 'output'
    output_folder.mkdir()
    unit = 'omarcs-highlight-' + uuid.uuid4().hex
    request = dict(report=report, highlight=highlight, folder=str(folder),
                   prefix=str(prefix), library=str(library), namespaces=isolation.namespaces())
    atomic(output_folder / 'request.json', request)
    atomic(ROOT / 'recovery.json', dict(unit=unit, folder=str(folder), isolationVersion=1))
    process = None
    try:
        with (folder / 'session.log').open('w') as log:
            process = subprocess.Popen([
                'systemd-run', '--user', '--wait', '--pipe', '--collect', '--unit=' + unit,
                '--property=RuntimeMaxSec=' + str(int(seconds * 5 + 150)),
                '--property=TimeoutStopSec=5', '--property=KillMode=control-group',
                '--property=CPUQuota=400%', '--property=MemoryMax=12G',
                '--property=MemorySwapMax=0', '--property=Nice=10',
                *isolation.command(HOME, folder, [
                    '/usr/bin/python3', str(HERE / 'private-media.py'), '--', '/usr/bin/python3',
                    str(Path(__file__).resolve()), '--stage', 'session', '--job', str(output_folder / 'request.json')])],
                stdout=log, stderr=subprocess.STDOUT)
            while process.poll() is None:
                if cancelled(batch):
                    raise InterruptedError('Batch cancelled')
                if shutil.disk_usage(folder).free < 2 * 1024**3:
                    raise RuntimeError('Rendering stopped to preserve free disk space.')
                time.sleep(0.25)
            if process.returncode != 0 or not (output_folder / 'capture/recorded').exists():
                raise RuntimeError(f'CS2 recording did not finish. Logs: {folder}')
    finally:
        stop_service(unit)
        if process is not None:
            process.wait(timeout=10)
        recover()
    folder = output_folder / 'capture'
    job = read(folder / 'job.json', {})
    raw = folder / 'raw'
    frames = sorted(raw.rglob(job['movie'] + '*.tga'))
    wav = raw / (job['movie'] + '.wav')
    expected = seconds * 30
    if abs(len(frames) - expected) > 3 or not frames or not wav.exists():
        raise RuntimeError(f'Incomplete clip: expected about {expected:.0f} frames, received {len(frames)}.')
    log = (folder / 'csdm.log').read_text()
    a = re.search(r'\[(\d+)\] Executed: startmovie ' + job['movie'], log)
    b = re.search(r'\[(\d+)\] Executed: endmovie', log)
    if not a or not b or abs(int(a[1]) - job['start']) > 2 or abs(int(b[1]) - job['end']) > 2:
        raise RuntimeError('The game did not record the requested demo ticks.')
    output = ROOT / 'clips' / (report['id'] + '-' + highlight['id'] + '.mp4')
    output.parent.mkdir(exist_ok=True)
    temp = folder / 'encoded.mp4'
    with (folder / 'encode.log').open('w') as log:
        encoder = subprocess.Popen(['ffmpeg', '-v', 'error', '-framerate', '30',
            '-start_number', str(int(frames[0].stem[-8:])),
            '-i', str(frames[0].parent / (job['movie'] + '%08d.tga')), '-i', str(wav),
            '-c:v', 'libx264', '-preset', 'fast', '-threads', '2', '-pix_fmt', 'yuv420p',
            '-c:a', 'aac', '-movflags', '+faststart', '-n', str(temp)], stderr=log)
        try:
            deadline = time.monotonic() + seconds * 5 + 60
            while encoder.poll() is None:
                if cancelled(batch): raise InterruptedError('Batch cancelled')
                if time.monotonic() > deadline: raise RuntimeError('Video encoding timed out.')
                time.sleep(0.25)
            if encoder.returncode: raise RuntimeError('Video encoding failed; see encode.log.')
        finally:
            if encoder.poll() is None:
                encoder.terminate()
                try: encoder.wait(timeout=3)
                except subprocess.TimeoutExpired: encoder.kill(); encoder.wait()
    probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(temp)], text=True))
    if {s['codec_type'] for s in probe['streams']} != {'video', 'audio'} or abs(float(probe['format']['duration']) - seconds) > 0.2:
        raise RuntimeError('Encoded video failed duration or audio/video validation.')
    atomic(folder / 'validation.json', dict(requested=[job['start'], job['end']], observed=[int(a[1]), int(b[1])], frames=len(frames), probe=probe))
    os.replace(temp, output)
    return str(output)


def session(request):
    isolation.require_private(request['namespaces'])
    # Edits below are only visible in the disposable overlay, including Steam's
    # cached offline preference. The network namespace cannot reach Steam Cloud.
    login = STEAM / 'config/loginusers.vdf'
    text = login.read_text()
    for key in ['WantsOfflineMode', 'SkipOfflineModeWarning']:
        pattern = r'("' + key + r'"\s+)"[01]"'
        text, count = re.subn(pattern, r'\g<1>"1"', text)
        if not count:
            raise RuntimeError('Steam has no cached offline login. Sign in to Steam normally first.')
    login.write_text(text)
    folder = Path(request['folder']) / 'output/capture'
    job = prepare(request['report'], request['highlight'], folder,
                  Path(request['prefix']), Path(request['library']), request['namespaces'])
    job['namespaces'] = request['namespaces']
    guard = socket.socket()
    guard.bind(('127.0.0.1', 0))
    job['wsPort'] = guard.getsockname()[1]
    atomic(folder / 'job.json', job)
    try:
        compositor(job)
    finally:
        log = GAME / 'bin/linuxsteamrt64/csdm.log'
        if log.exists(): shutil.copy2(log, folder / 'csdm.log')
        for wav in (GAME / 'csgo/movie').rglob(job['movie'] + '*.wav'):
            shutil.move(str(wav), folder / 'raw' / wav.name)
        guard.close()


def compositor(job):
    if os.environ.get('OMARCS_PRIVATE_MEDIA') != '1':
        raise RuntimeError('Private media server is required')
    prefix = Path(job['prefix']); lib = prefix / 'usr/lib'; folder = Path(job['folder'])
    env = os.environ.copy()
    env.update(LD_LIBRARY_PATH=f'{lib}:{lib}/weston', WESTON_MODULE_MAP=';'.join(f'{p.name}={p}' for p in lib.rglob('*.so')))
    config = folder / 'weston.ini'; config.write_text('[output]\nname=pipewire\nmode=1280x720@30\n')
    run([str(prefix / 'usr/bin/weston'), '--backends=headless,pipewire', '--no-outputs', '--fake-seat',
         '--renderer=gl', f'--shell={lib}/weston/kiosk-shell.so', '--socket=' + job['socket'],
         '--config=' + str(config), '--width=1280', '--height=720', '--idle-time=0',
         '--xwayland', '--no-xwm-decorations', '--log=' + str(folder / 'weston.log'), '--',
         sys.executable, str(__file__), '--stage', 'game', '--job', str(folder / 'job.json')], env=env)


def play(job):
    require_safe_rendering()
    env = os.environ.copy()
    if (env.get('OMARCS_PRIVATE_MEDIA') != '1' or env.get('WAYLAND_DISPLAY') != job['socket']
            or not env.get('DISPLAY') or env['DISPLAY'] == job['desktop']):
        raise RuntimeError('Refusing to start outside the private display/audio session')
    sinks = json.loads(subprocess.check_output(['pactl', '-f', 'json', 'list', 'sinks'], text=True))
    if [s['name'] for s in sinks] != ['omarcs_private_default']:
        raise RuntimeError('Unexpected output on the private media server')
    isolation.require_private(job['namespaces'])
    steam_env = os.environ.copy()
    for key in ['LD_LIBRARY_PATH', 'WESTON_MODULE_MAP']:
        steam_env.pop(key, None)
    with (Path(job['folder']) / 'steam.log').open('w') as steam_log:
        subprocess.Popen(['/usr/bin/steam', '-offline', '-silent', '-noverifyfiles',
                          '-skipinitialbootstrap'], env=steam_env, stdout=steam_log, stderr=subprocess.STDOUT)
    # Offline cached login and the local Steam IPC server must initialize first.
    time.sleep(25)
    for key in ['LD_LIBRARY_PATH', 'WESTON_MODULE_MAP']:
        env.pop(key, None)
    env.update(SDL_VIDEO_DRIVER='wayland', SDL_VIDEODRIVER='wayland', SDL_AUDIO_DRIVER='pulseaudio',
               SDL_AUDIODRIVER='pulseaudio', SteamAppId='730', SteamGameId='730', CSDM_WS_PORT=str(job['wsPort']))
    folder = Path(job['folder']); game = Path(job['game'])
    with (folder / 'game.log').open('w') as log:
        child = subprocess.Popen([str(game.parent.parent / 'SteamLinuxRuntime_sniper/_v2-entry-point'),
            '--verb=waitforexitandrun', '--', str(game / 'cs2.sh'), '-insecure', '-novid',
            '-sw', '-width', '1280', '-height', '720', '+snd_mute_losefocus', '0',
            '+playdemo', job['demo']], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            log_path = game / 'bin/linuxsteamrt64/csdm.log'
            deadline = time.monotonic() + (job['end'] - job['start']) / 64 * 5 + 85
            while time.monotonic() < deadline:
                text = log_path.read_text(errors='replace') if log_path.exists() else ''
                if ('Executed: startmovie ' + job['movie']) in text and '] Executed: endmovie' in text:
                    # Let the scheduled quit release Steam/GPU/audio normally
                    # before the next clip starts. The cgroup remains the fallback.
                    try:
                        child.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        print('CS2 graceful exit timed out; using service cleanup', flush=True)
                    (folder / 'recorded').touch()
                    return
                if child.poll() is not None: raise RuntimeError('CS2 exited during demo playback')
                time.sleep(0.1)
            raise RuntimeError('CS2 did not finish the highlight before the time limit')
        finally:
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait()


def worker(fd, request):
    require_safe_rendering()
    lease = os.fdopen(fd, 'a+')
    value = read(request, {})
    batch = value['batchId']
    def interrupted(signum, frame):
        (ROOT / ('cancel-' + batch)).touch()
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        recover()
        prefix, library = dependencies()
        report = value.pop('report')
        # Hash once per batch; never render a replaced file using old highlight ticks.
        if digest(Path(report['path'])) != report['checksum']:
            raise RuntimeError('The source demo changed. Refresh its analysis before rendering.')
        value['state'] = 'rendering'
        for job in value['jobs']:
            if cancelled(batch): raise InterruptedError('Batch cancelled')
            job['state'] = 'rendering'; value['message'] = 'Rendering ' + ', '.join(job['highlight']['labels'])
            atomic(ROOT / 'status.json', value)
            folder = ROOT / 'work' / (batch + '-' + job['highlight']['id'])
            try:
                output = render_clip(report, job['highlight'], folder, prefix, library, batch)
                job.update(state='complete', path=output)
                clips = read(ROOT / 'clips.json', {})
                clips[job['key']] = {'path': output, 'createdAt': time.time()}
                atomic(ROOT / 'clips.json', clips)
            finally:
                recover()
                # Keep small logs/recovery backups, not multi-GB raw frames/demos.
                if not (ROOT / 'recovery.json').exists():
                    shutil.rmtree(folder / 'output/capture/raw', ignore_errors=True)
                    (folder / 'output/capture/source.dem').unlink(missing_ok=True)
            atomic(ROOT / 'status.json', value)
        value.update(state='complete', message='Highlights ready')
    except BaseException as error:
        value.pop('report', None)
        value.update(state='cancelled' if isinstance(error, InterruptedError) else 'failed', message=str(error))
        for job in value['jobs']:
            if job['state'] in ('queued', 'rendering'): job.update(state=value['state'], error=str(error))
    finally:
        atomic(ROOT / 'status.json', value)
        request.unlink(missing_ok=True)
        (ROOT / ('cancel-' + batch)).unlink(missing_ok=True)
        lease.close()


def start(match_id, highlight_id):
    require_safe_rendering()
    lock = locked(ROOT / 'batch.lock')
    try:
        # An interrupted worker may have left its bounded service alive. Stop
        # only that recorded service before checking for a user-owned game.
        recover()
        game_closed()
        report = load_match(match_id)
        hs = report['highlights'][:5] if highlight_id is None else [h for h in report['highlights'] if h['id'] == highlight_id]
        if not hs: raise RuntimeError('Highlight not found')
        batch = uuid.uuid4().hex
        value = dict(batchId=batch, state='queued', message='Preparing rendering batch', jobs=[
            dict(key=report['id'] + ':' + h['id'], matchId=report['id'], highlight=h, state='queued') for h in hs])
        atomic(ROOT / 'status.json', value)
        request = ROOT / ('request-' + batch + '.json')
        atomic(request, dict(value, report=report))
        with (ROOT / 'worker.log').open('a') as log:
            subprocess.Popen([sys.executable, str(__file__), '--worker', str(lock.fileno()), '--job', str(request)],
                             pass_fds=[lock.fileno()], start_new_session=True, stdout=log, stderr=subprocess.STDOUT)
        print(json.dumps(value))
    finally:
        lock.close()


def setup():
    # Explicit tooling setup only. No game launch, system package changes or idle hooks.
    if platform.machine() != 'x86_64':
        raise RuntimeError('Local video tools currently support x86_64 Linux only.')
    target = DATA / 'video'
    target.mkdir(parents=True, exist_ok=True)
    with locked(target / 'setup.lock'):
        def download(url, path, expected=None):
            temp = path.with_suffix('.download')
            run(['curl', '--fail', '--silent', '--show-error', '--location', '--proto', '=https',
                 '--tlsv1.2', '--connect-timeout', '15', '--max-time', '180', '--retry', '2',
                 '--output', str(temp), url])
            if expected and digest(temp) != expected:
                temp.unlink()
                raise RuntimeError('Downloaded video tool failed checksum verification.')
            os.replace(temp, path)
        base = 'https://raw.githubusercontent.com/akiver/cs-demo-manager/' + REV + '/'
        download(base + 'static/cs2/libserver.so', target / 'libserver.so', LIB_HASH)
        download(base + 'LICENSE', target / 'CSDM-LICENSE')
        if not (target / 'weston/usr/bin/weston').exists():
            archive = target / 'weston.pkg.tar.zst'
            download('https://stable-mirror.omarchy.org/extra/os/x86_64/weston-15.0.1-3-x86_64.pkg.tar.zst',
                     archive, '92fe0bfe2cf1ae02105b0e6573f6695c6bbd4637c9a6b20299a6fea8c3d23b4d')
            stage = target / ('weston-' + uuid.uuid4().hex)
            stage.mkdir()
            # Check members before extracting the hash-verified distribution package.
            listing = subprocess.check_output(['tar', '-tf', str(archive)], text=True).splitlines()
            if any(Path(p).is_absolute() or '..' in Path(p).parts for p in listing):
                raise RuntimeError('Unexpected package paths')
            run(['tar', '-xf', str(archive), '-C', str(stage)])
            os.replace(stage, target / 'weston')
            archive.unlink()
        print('Video tools installed locally. No game was started.')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('command', nargs='?', choices=['status', 'render', 'cancel', 'open', 'folder', 'setup'])
    parser.add_argument('match', nargs='?')
    parser.add_argument('highlight', nargs='?')
    parser.add_argument('--worker', type=int)
    parser.add_argument('--stage', choices=['session', 'compositor', 'game'])
    parser.add_argument('--job', type=Path)
    args = parser.parse_args()
    ROOT.mkdir(parents=True, exist_ok=True)
    if args.stage:
        job = read(args.job, {})
        if args.stage == 'session': return session(job)
        return compositor(job) if args.stage == 'compositor' else play(job)
    if args.worker is not None: return worker(args.worker, args.job)
    if args.command == 'status': print(json.dumps(status()))
    elif args.command == 'render': start(args.match, args.highlight)
    elif args.command == 'cancel':
        current = status()
        if current['state'] in ('queued', 'rendering', 'cancelling'):
            (ROOT / ('cancel-' + current['batchId'])).touch()
        elif current['state'] == 'interrupted':
            with locked(ROOT / 'batch.lock'):
                recover()
                current.update(state='cancelled', message='Interrupted batch cleaned up')
                for job in current.get('jobs', []):
                    if job['state'] == 'interrupted': job['state'] = 'cancelled'
                current.pop('clips', None)
                atomic(ROOT / 'status.json', current)
    elif args.command == 'setup': setup()
    elif args.command in ('open', 'folder'):
        if args.command == 'folder':
            path = ROOT / 'clips'; path.mkdir(exist_ok=True)
        else:
            entry = read(ROOT / 'clips.json', {}).get(args.match + ':' + args.highlight)
            if not entry or not Path(entry['path']).is_file(): raise RuntimeError('Clip file is missing. Render it again.')
            path = Path(entry['path'])
        subprocess.Popen(['xdg-open', str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else: parser.error('choose a command')


if __name__ == '__main__':
    try: main()
    except BlockingIOError:
        print('A rendering batch is already running.', file=sys.stderr); sys.exit(1)
    except Exception as error:
        print(str(error), file=sys.stderr); sys.exit(1)
