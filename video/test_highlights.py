import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('video_highlights', Path(__file__).with_name('highlights.py'))
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


class RenderingLifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.patches = [patch.object(v, 'ROOT', self.root / 'state'),
                        patch.object(v, 'DATA', self.root / 'data'),
                        patch.object(v, 'STEAM', self.root / 'Steam'),
                        patch.object(v, 'GAME', self.root / 'Steam/steamapps/common/CS2/game')]
        for p in self.patches: p.start()
        v.ROOT.mkdir()

    def tearDown(self):
        for p in reversed(self.patches): p.stop()
        self.temp.cleanup()

    def test_stale_batch_is_interrupted_without_launching_any_process(self):
        v.atomic(v.ROOT / 'status.json', {'state': 'rendering', 'jobs': [{'state': 'rendering'}]})
        with patch.object(v.subprocess, 'run', side_effect=AssertionError('status launched a process')):
            result = v.status()
        self.assertEqual(result['state'], 'interrupted')
        self.assertEqual(result['jobs'][0]['state'], 'interrupted')

    def test_live_lease_prevents_a_second_batch(self):
        with v.locked(v.ROOT / 'batch.lock'):
            with self.assertRaises(BlockingIOError): v.locked(v.ROOT / 'batch.lock')

    def test_game_setup_refuses_host_namespace_before_writing_files(self):
        with self.assertRaisesRegex(RuntimeError, 'outside the offline'):
            v.prepare({}, {}, self.root / 'work', self.root, self.root,
                      v.isolation.namespaces())
        self.assertFalse((self.root / 'work').exists())

    def test_legacy_recovery_never_restores_old_settings(self):
        v.atomic(v.ROOT / 'recovery.json', {'unit': 'old-renderer'})
        with patch.object(v, 'stop_service', side_effect=AssertionError('unexpected process control')):
            with self.assertRaisesRegex(RuntimeError, 'left untouched'):
                v.recover()

    def test_new_recovery_only_discards_private_overlay(self):
        folder = v.ROOT / 'work/batch'
        (folder / 'upper').mkdir(parents=True)
        (folder / 'upper/settings').write_text('private fps 30')
        real = self.root / 'real-settings'; real.write_text('fps 253')
        v.atomic(v.ROOT / 'recovery.json', dict(unit='owned', folder=str(folder), isolationVersion=1))
        with patch.object(v, 'stop_service') as stop:
            v.recover()
            stop.assert_called_once_with('owned')
        self.assertEqual(real.read_text(), 'fps 253')
        self.assertFalse((folder / 'upper').exists())

    def test_retry_recovers_owned_service_before_checking_for_a_running_game(self):
        calls = []
        with patch.object(v, 'require_safe_rendering'), \
             patch.object(v, 'recover', side_effect=lambda: calls.append('recover')), \
             patch.object(v, 'game_closed', side_effect=lambda: calls.append('check-game')), \
             patch.object(v, 'load_match', side_effect=RuntimeError('missing demo')):
            with self.assertRaisesRegex(RuntimeError, 'missing demo'):
                v.start('match', None)
        self.assertEqual(calls, ['recover', 'check-game'])
        with v.locked(v.ROOT / 'batch.lock'):
            pass  # Failed starts release their lease.

    def test_low_disk_is_rejected_before_touching_game_files(self):
        from collections import namedtuple
        Usage = namedtuple('Usage', 'total used free')
        demo = self.root / 'demo.dem'; demo.write_bytes(b'demo')
        with patch.object(v.shutil, 'disk_usage', return_value=Usage(100, 99, 1)), \
             patch.object(v, 'prepare', side_effect=AssertionError('game was modified')):
            with self.assertRaisesRegex(RuntimeError, 'free disk space'):
                v.render_clip({'path': str(demo)}, {'startTick': 1000, 'endTick': 1200},
                              self.root / 'work', self.root, self.root, 'batch')

    def test_existing_game_is_refused(self):
        class Running:
            returncode = 0
        with patch.object(v.subprocess, 'run', return_value=Running()):
            with self.assertRaisesRegex(RuntimeError, 'Close CS2'):
                v.game_closed()


if __name__ == '__main__':
    unittest.main()
