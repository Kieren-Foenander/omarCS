"""Disposable offline filesystem/process/network boundary for CS2 and Steam."""
import os
from pathlib import Path
import shutil

NAMESPACES = ('mnt', 'pid', 'net', 'ipc')


def namespaces():
    return {name: os.readlink('/proc/self/ns/' + name) for name in NAMESPACES}


def require_private(original):
    current = namespaces()
    if set(original) != set(NAMESPACES) or any(current[n] == original[n] for n in NAMESPACES):
        raise RuntimeError('Refusing game setup outside the offline filesystem sandbox')


def command(home, folder, argv):
    home, folder = Path(home).resolve(), Path(folder).resolve()
    # Only output is writable on the host. Steam, CS2, HOME and all aliases
    # below HOME use overlay copy-up; neither normal exit nor SIGKILL restores files.
    if not folder.is_relative_to(home) or folder == home:
        raise RuntimeError('Rendering work must be in a dedicated directory under HOME')
    for name in ['upper', 'overlay-work', 'output']:
        (folder / name).mkdir(parents=True, exist_ok=True)
    runtime = '/run/user/' + str(os.getuid())
    devices = []
    for device in [Path('/dev/dri'), *sorted(Path('/dev').glob('nvidia*'))]:
        if device.exists(): devices += ['--dev-bind', str(device), str(device)]
    return ['bwrap', '--ro-bind', '/', '/', '--unshare-all', '--die-with-parent',
            '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/dev/shm', *devices,
            '--tmpfs', '/tmp', '--dir', '/tmp/.X11-unix',
            '--tmpfs', '/run', '--dir', runtime, '--chmod', '0700', runtime,
            '--overlay-src', str(home), '--overlay', str(folder / 'upper'),
            str(folder / 'overlay-work'), str(home),
            # A FIFO/socket is IPC even on a read-only/overlay mount. Never
            # expose the live Steam command pipe, token, PID or client sockets.
            '--tmpfs', str(home / '.steam'),
            '--bind', str(folder / 'output'), str(folder / 'output'),
            '--unsetenv', 'DBUS_SESSION_BUS_ADDRESS', '--unsetenv', 'DISPLAY',
            '--unsetenv', 'WAYLAND_DISPLAY', '--unsetenv', 'SSH_AUTH_SOCK',
            '--setenv', 'XDG_RUNTIME_DIR', runtime, '--', *argv]


def discard(folder):
    # Overlayfs creates mode-000 internal directories. They belong to this user.
    def retry(function, path, exc):
        os.chmod(path, 0o700)
        if function is os.open:
            shutil.rmtree(path, onexc=retry)
        else:
            function(path)
    for name in ('upper', 'overlay-work'):
        path = Path(folder) / name
        if path.exists():
            shutil.rmtree(path, onexc=retry)
