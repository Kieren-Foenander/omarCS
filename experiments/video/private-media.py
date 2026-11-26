#!/usr/bin/env python3
"""Run a development probe on its own PipeWire/PulseAudio graph, without hardware."""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time


def stop(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('provide a command after --')
    children = []
    with tempfile.TemporaryDirectory(prefix='omarcs-media-') as directory:
        root = Path(directory)
        env = os.environ.copy()
        env.update(OMARCS_PRIVATE_MEDIA='1', PIPEWIRE_RUNTIME_DIR=directory,
                   PULSE_RUNTIME_PATH=str(root / 'pulse'),
                   PULSE_SERVER='unix:' + str(root / 'pulse/native'),
                   XDG_CONFIG_HOME=str(root / 'config'),
                   XDG_STATE_HOME=str(root / 'state'),
                   PIPEWIRE_CONFIG_DIR='/usr/share/pipewire')
        (root / 'pulse').mkdir()
        try:
            for cmd in [
                ['pipewire'],
                # The installed policy profile contains no hardware monitors.
                ['wireplumber', '--profile=policy'],
                ['pipewire-pulse'],
            ]:
                children.append(subprocess.Popen(cmd, env=env, start_new_session=True,
                                                 stdout=subprocess.DEVNULL,
                                                 stderr=subprocess.DEVNULL))
                time.sleep(0.3)
            for _ in range(50):
                result = subprocess.run(['pactl', 'info'], env=env, capture_output=True)
                if result.returncode == 0:
                    break
                if any(p.poll() is not None for p in children):
                    raise RuntimeError('A private media service failed to start')
                time.sleep(0.1)
            else:
                raise RuntimeError('Private PulseAudio server did not become ready')
            # No real hardware or desktop audio exists on this graph. An app
            # ignoring a requested sink can only use another private sink.
            sink = subprocess.check_output([
                'pactl', 'load-module', 'module-null-sink',
                'sink_name=omarcs_private_default'], env=env, text=True).strip()
            subprocess.run(['pactl', 'set-default-sink', 'omarcs_private_default'],
                           env=env, check=True)
            env['PULSE_SINK'] = 'omarcs_private_default'
            child = subprocess.Popen(command, env=env, start_new_session=True)
            children.append(child)
            raise SystemExit(child.wait())
        finally:
            for child in reversed(children):
                stop(child)


if __name__ == '__main__':
    main()
