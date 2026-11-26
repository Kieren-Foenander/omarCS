import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import unittest
import isolation


class OfflineBoundary(unittest.TestCase):
    def test_real_writes_and_sigkill_cannot_change_originals_or_use_steam_pipe(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory) / 'home'; home.mkdir()
            config = home / 'settings.vcfg'; config.write_text('fps_max 253')
            (home / '.steam').mkdir()
            os.mkfifo(home / '.steam/steam.pipe')
            shared = tempfile.NamedTemporaryFile(prefix='omarcs-test-', dir='/dev/shm')
            server = socket.socket(); server.bind(('127.0.0.1', 0)); server.listen()
            self.addCleanup(shared.close); self.addCleanup(server.close)
            for crash in (False, True):
                folder = home / ('crash' if crash else 'normal')
                original = isolation.namespaces()
                code = '''import os,json,signal,socket
from pathlib import Path
home=Path(os.environ['TEST_HOME'])
original=json.loads(os.environ['TEST_NS'])
assert all(os.readlink('/proc/self/ns/'+n)!=v for n,v in original.items())
assert not (home/'.steam/steam.pipe').exists()
assert not Path(os.environ['TEST_SHM']).exists()
s=socket.socket();s.settimeout(1)
assert s.connect_ex(('127.0.0.1', int(os.environ['TEST_PORT']))) != 0
(home/'settings.vcfg').write_text('fps_max 30')
(home/'new-settings').write_text('private')
(home/'settings.vcfg').unlink()
(home/'settings.vcfg').write_text('another value')
if os.environ['TEST_CRASH']=='1':os.kill(os.getpid(),signal.SIGKILL)
'''
                cmd = isolation.command(home, folder, ['/usr/bin/python3', '-c', code])
                result = subprocess.run(cmd, env=dict(os.environ, TEST_HOME=str(home),
                    TEST_NS=json.dumps(original), TEST_CRASH=str(int(crash)),
                    TEST_SHM=shared.name, TEST_PORT=str(server.getsockname()[1])), capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, not crash, result.stderr)
                self.assertEqual(config.read_text(), 'fps_max 253')
                self.assertFalse((home/'new-settings').exists())
                self.assertTrue((home/'.steam/steam.pipe').exists())
                isolation.discard(folder)

    def test_namespace_guard_rejects_a_partial_boundary(self):
        original = isolation.namespaces()
        with self.assertRaises(RuntimeError): isolation.require_private(original)
        with self.assertRaises(RuntimeError): isolation.require_private({})


if __name__ == '__main__':unittest.main()
