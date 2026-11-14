"""Harness-only tests. Passing these is NOT a passing Buildbarn integration run."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

from runtime_smoke import cleanup_processes, marker_indices, stop


class RuntimeHarnessTests(unittest.TestCase):
    def test_all_outputs_require_matching_fresh_nonce(self):
        with tempfile.TemporaryDirectory() as temp:
            cell = Path(temp)
            out = cell / 'buck-out/gen'
            out.mkdir(parents=True)
            for index in range(8):
                (out / f'worker-{index}.txt').write_text(f'host\nbuildbarn-smoke-{index}-fresh\n')
            self.assertEqual(marker_indices(cell, 'fresh'), set(range(8)))
            self.assertEqual(marker_indices(cell, 'other'), set())

    def test_empty_hostname_or_partial_marker_does_not_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            cell = Path(temp)
            out = cell / 'buck-out'
            out.mkdir()
            (out / 'worker-0.txt').write_text('\nbuildbarn-smoke-0-fresh\n')
            (out / 'worker-1.txt').write_text('host\nbuildbarn-smoke-1-fresh-more\n')
            self.assertEqual(marker_indices(cell, 'fresh'), set())

    def test_exit_race_is_reaped(self):
        process = Mock(pid=123, poll=Mock(return_value=None))
        with patch('runtime_smoke.os.killpg', side_effect=ProcessLookupError):
            stop(process)
        process.wait.assert_called_once_with(timeout=5)

    def test_cleanup_failure_does_not_skip_remaining_processes_or_logs(self):
        processes = [Mock(pid=1), Mock(pid=2)]
        log = Mock()
        with patch('runtime_smoke.stop', side_effect=[RuntimeError('failed'), None]) as stopped:
            errors = cleanup_processes(processes, [log])
        self.assertEqual(stopped.call_count, 2)
        self.assertEqual(len(errors), 1)
        log.close.assert_called_once()

    def test_stop_only_owned_group_is_idempotent(self):
        p = subprocess.Popen(['/bin/sleep', '60'], start_new_session=True)
        try:
            stop(p)
            self.assertIsNotNone(p.poll())
            stop(p)
        finally:
            if p.poll() is None:
                p.kill()
                p.wait()


if __name__ == '__main__':
    unittest.main()
