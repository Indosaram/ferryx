"""Pure guard regressions; do not start displays, applications, or sockets."""
import importlib.util
import ast
import base64
import tempfile
import subprocess
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('smoke', Path(__file__).with_name('pr24-native-smoke.py'))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class NamespaceGuardTests(unittest.TestCase):
    def check(self, links, ipv4=(), ipv6=()):
        replies = [SimpleNamespace(stdout=json.dumps(value)) for value in [links, ipv4, ipv6]]
        with patch.object(smoke.os, 'geteuid', return_value=1001), \
             patch.object(smoke.os, 'readlink', return_value='net:[123]'), \
             patch.object(smoke, 'run', side_effect=replies):
            return smoke.assert_offline_namespace()

    def test_signal_handler_raises_for_finally_cleanup(self):
        with self.assertRaisesRegex(smoke.SmokeInterrupted, 'interrupted by signal 15'):
            smoke.interrupted(15, None)

    def test_only_loopback_is_accepted(self):
        self.assertEqual(self.check([{'ifname': 'lo'}], [{'dev': 'lo', 'dst': '127.0.0.0/8'}])['namespace'], 'net:[123]')

    def test_external_interface_rejected_even_without_routes(self):
        with self.assertRaisesRegex(RuntimeError, 'only loopback'):
            self.check([{'ifname': 'lo'}, {'ifname': 'eth0'}])

    def test_external_ipv4_route_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'non-loopback route'):
            self.check([{'ifname': 'lo'}], [{'dev': 'eth0', 'dst': 'default'}])

    def test_gateway_on_loopback_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'non-loopback route'):
            self.check([{'ifname': 'lo'}], [{'dev': 'lo', 'gateway': '1.2.3.4'}])

    def test_external_ipv6_route_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'non-loopback route'):
            self.check([{'ifname': 'lo'}], [], [{'dev': 'eth0', 'dst': '::/0'}])

    def test_root_rejected_before_network_probe(self):
        with patch.object(smoke.os, 'geteuid', return_value=0), patch.object(smoke, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'dropping root'):
                smoke.assert_offline_namespace()
            run.assert_not_called()


class CaptureReadinessTests(unittest.TestCase):
    def test_known_blank_capture_is_rejected(self):
        self.assertFalse(smoke.meaningful_content(smoke.parse_capture_metrics('1022 680 1 0')))

    def test_nonblank_content_is_eligible_for_visual_review(self):
        self.assertTrue(smoke.meaningful_content(smoke.parse_capture_metrics('1022 680 340 0.32')))

    def test_few_colors_or_low_entropy_or_small_image_rejected(self):
        for text in ['1022 680 15 0.5', '1022 680 200 0.001', '50 40 200 0.5']:
            with self.subTest(text=text):
                self.assertFalse(smoke.meaningful_content(smoke.parse_capture_metrics(text)))

    def test_malformed_nonfinite_and_out_of_range_metrics_rejected(self):
        for text in ['', '1 2 3', '1 2 3 nan', '1 2 3 inf', '0 2 3 0.2', '1 2 3 1.1']:
            with self.subTest(text=text), self.assertRaises((ValueError, RuntimeError)):
                smoke.parse_capture_metrics(text)


class PixelChangeTests(unittest.TestCase):
    def test_identical_pixels_ignore_file_metadata(self):
        self.assertEqual(smoke.changed_pixels(bytes([0, 0, 0, 255]) * 600, bytes([0, 0, 0, 255]) * 600), 0)

    def test_counts_changed_pixels_not_channels(self):
        self.assertEqual(smoke.changed_pixels(bytes([0, 0, 0, 255]) * 600, bytes([255, 255, 255, 255]) * 600), 600)

    def test_tiny_color_noise_ignored(self):
        self.assertEqual(smoke.changed_pixels(bytes([0, 0, 0, 255]), bytes([15, 15, 15, 255])), 0)

    def test_mismatched_dimensions_fail(self):
        with self.assertRaises(RuntimeError):
            smoke.changed_pixels(bytes(4), bytes(8))


class TerminalFixtureTests(unittest.TestCase):
    def test_nonce_is_output_only_not_echoed_command(self):
        marker = 'FERRYX-QA-abc123'
        script = Path('/owned fixture/fixture.py')
        command = smoke.fixture_command(script)
        self.assertNotIn(marker, command)
        self.assertEqual(command, "python3 '/owned fixture/fixture.py'")
        payload = smoke.terminal_payload(marker)
        self.assertEqual(payload.count(marker.encode()), 1)

    def test_real_kitty_rgb_quadrant_payload(self):
        payload = smoke.terminal_payload('MARKER')
        encoded = payload.split(b';', 1)[1].split(b'\x1b\\', 1)[0]
        rgb = base64.b64decode(encoded, validate=True)
        self.assertEqual(len(rgb), 16 * 16 * 3)
        for x, y, color in [(0, 0, [255, 0, 0]), (15, 0, [0, 255, 0]),
                            (0, 15, [0, 0, 255]), (15, 15, [255, 255, 0])]:
            offset = (y * 16 + x) * 3
            self.assertEqual(list(rgb[offset:offset + 3]), color)

    def test_generated_source_requires_pty_and_flushes_before_receipt(self):
        source = smoke.fixture_source('MARKER', Path('/owned/pty-receipt.json'))
        compile(source, '<fixture-inspection-only>', 'exec')  # Never execute fixture locally.
        self.assertIn("receipt = Path('/owned/pty-receipt.json')", source)
        self.assertLess(source.index('assert os.isatty(1)'), source.index('sys.stdout.buffer.write'))
        self.assertLess(source.index('sys.stdout.buffer.flush()'), source.index('temporary.replace(receipt)'))

    def test_receipt_requires_exact_nonce_and_real_pty(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipt.json'
            data = {'marker': 'MARKER', 'pty_stdout': True, 'pid': 123, 'image_pixels': 256}
            path.write_text(json.dumps(data))
            self.assertEqual(smoke.validated_receipt(path, 'MARKER'), data)
            with self.assertRaises(RuntimeError):
                smoke.validated_receipt(path, 'wrong')
            data['pty_stdout'] = False
            path.write_text(json.dumps(data))
            with self.assertRaises(RuntimeError):
                smoke.validated_receipt(path, 'MARKER')


class SkipSetupGuardTests(unittest.TestCase):
    def test_reviewed_owned_active_geometry_has_only_skip_target(self):
        self.assertEqual(smoke.guarded_skip_setup_point(1278, 850, 20, 20, 42, 42), (530, 716))

    def test_changed_geometry_is_not_scaled_or_guessed(self):
        for width, height in [(1280, 850), (1278, 849), (640, 480)]:
            with self.subTest(size=(width, height)), self.assertRaises(RuntimeError):
                smoke.guarded_skip_setup_point(width, height, 20, 20, 42, 42)

    def test_foreign_or_unfocused_window_is_rejected(self):
        for expected, owner, window, active in [(20, 21, 42, 42), (20, 20, 42, 43), (0, 0, 42, 42)]:
            with self.subTest(owner=owner, active=active), self.assertRaises(RuntimeError):
                smoke.guarded_skip_setup_point(1278, 850, expected, owner, window, active)


class SparsePostStartupTests(unittest.TestCase):
    def test_reviewed_empty_workspace_is_post_startup_valid_only(self):
        metrics = smoke.parse_capture_metrics('1022 680 329 0.0135897')
        self.assertFalse(smoke.meaningful_content(metrics))
        self.assertTrue(smoke.nonblank_post_startup_content(metrics))

    def test_actual_blank_capture_fails_both_gates(self):
        metrics = smoke.parse_capture_metrics('1022 680 1 0')
        self.assertFalse(smoke.meaningful_content(metrics))
        self.assertFalse(smoke.nonblank_post_startup_content(metrics))

    def test_post_startup_still_rejects_invalid_or_tiny_frames(self):
        for width, height, colors, entropy in [(1022, 680, 15, .01), (100, 60, 329, .01),
                                               (1022, 680, 329, 0), (1022, 680, 329, float('nan')),
                                               (1022, 680, 329, float('inf'))]:
            metrics = dict(width=width, height=height, colors=colors, entropy=entropy)
            with self.subTest(metrics=metrics):
                self.assertFalse(smoke.nonblank_post_startup_content(metrics))


class WindowManagerReadinessTests(unittest.TestCase):
    ROOT = '_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x200001'
    SUPPORTED = '_NET_SUPPORTED(ATOM) = _NET_CLIENT_LIST, _NET_ACTIVE_WINDOW, _NET_WM_NAME'

    def test_self_referenced_wm_and_supported_activation_required(self):
        self.assertTrue(smoke.wm_check_ready(self.ROOT, self.ROOT, self.SUPPORTED))
        self.assertFalse(smoke.wm_check_ready(self.ROOT, self.ROOT.replace('200001', '200002'), self.SUPPORTED))
        self.assertFalse(smoke.wm_check_ready(self.ROOT, self.ROOT, '_NET_SUPPORTED(ATOM) = _NET_ACTIVE_WINDOW_EXTRA'))

    def test_missing_zero_or_stale_check_never_ready(self):
        for value in ['', '_NET_SUPPORTING_WM_CHECK: no such atom on any window.',
                      self.ROOT.replace('200001', '0'), self.ROOT + ', 0x200002']:
            self.assertFalse(smoke.wm_check_ready(value, self.ROOT, self.SUPPORTED))

    def test_managed_window_ids_use_exact_numeric_membership(self):
        ids = smoke.xprop_window_ids('_NET_CLIENT_LIST(WINDOW): window id # 0x400003, 0x500001', '_NET_CLIENT_LIST')
        self.assertIn(4194307, ids)
        self.assertNotIn(0x40000, ids)
        self.assertEqual(smoke.xprop_window_ids('_NET_CLIENT_LIST(WINDOW): window id # 0x400003garbage', '_NET_CLIENT_LIST'), [])
        self.assertEqual(smoke.xprop_window_ids('_OTHER(WINDOW): window id # 0x400003', '_NET_CLIENT_LIST'), [])

    def test_diagnostics_exclude_command_and_bound_stderr(self):
        error = subprocess.CalledProcessError(1, ['secret-argv'], stderr='x' * 5000)
        result = smoke.bounded_command_failure(error)
        self.assertEqual(result['returncode'], 1)
        self.assertEqual(len(result['stderr']), 2048)
        self.assertNotIn('secret-argv', str(result))
        timeout = smoke.bounded_command_failure(subprocess.TimeoutExpired(['secret-argv'], 8, stderr=b'timeout'))
        self.assertTrue(timeout['timed_out'])
        self.assertEqual(timeout['stderr'], 'timeout')


class OwnedDebugDiagnosticsTests(unittest.TestCase):
    def test_bounded_tail_keeps_only_owned_log_suffix(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / smoke.OWNED_DEBUG_LOGS[0]
            path.write_bytes(b'old-record\n' + b'new-record\n' * 10)
            tail = smoke.owned_debug_tail(directory, path.name, 40)
            self.assertLessEqual(len(tail), 40)
            self.assertTrue(tail.endswith(b'new-record\n'))
            self.assertNotIn(b'old-record', tail)

    def test_unapproved_name_and_excess_limit_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            for name, limit in [('../foreign', 10), ('other.log', 10), (smoke.OWNED_DEBUG_LOGS[0], smoke.MAX_DEBUG_TAIL + 1)]:
                with self.assertRaises(ValueError):
                    smoke.owned_debug_tail(directory, name, limit)

    def test_file_and_directory_symlinks_are_not_followed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            foreign = root / 'foreign'
            foreign.mkdir()
            (foreign / smoke.OWNED_DEBUG_LOGS[0]).write_bytes(b'not-owned-scope')
            alias = root / 'alias'
            alias.symlink_to(foreign, target_is_directory=True)
            with self.assertRaises(OSError):
                smoke.owned_debug_tail(alias, smoke.OWNED_DEBUG_LOGS[0])
            (root / smoke.OWNED_DEBUG_LOGS[0]).symlink_to(foreign / smoke.OWNED_DEBUG_LOGS[0])
            with self.assertRaises(OSError):
                smoke.owned_debug_tail(root, smoke.OWNED_DEBUG_LOGS[0])

    def test_hardlinked_log_and_foreign_directory_owner_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / 'other'
            original.write_bytes(b'other')
            os.link(original, root / smoke.OWNED_DEBUG_LOGS[0])
            with self.assertRaises(RuntimeError):
                smoke.owned_debug_tail(root, smoke.OWNED_DEBUG_LOGS[0])
            with patch.object(smoke.os, 'geteuid', return_value=os.geteuid() + 1):
                with self.assertRaises(RuntimeError):
                    smoke.owned_debug_tail(root, smoke.OWNED_DEBUG_LOGS[0])

    def test_focus_metadata_is_numeric_only(self):
        self.assertEqual(smoke.parse_x_numeric('4194307\n'), 4194307)
        for value in ['pid=42', '42 secret', '-1', '4294967296', '']:
            with self.assertRaises(ValueError):
                smoke.parse_x_numeric(value)


class ControlledReturnDiagnosticTests(unittest.TestCase):
    def snapshot(self):
        return dict(app_pid=12, owned_window=99, active_window=99, resolved_focus_window=99,
                    active_window_owner_pid=12, resolved_focus_window_owner_pid=12)

    def test_only_absent_receipt_and_owned_active_resolved_focus_allow_diagnostic(self):
        self.assertTrue(smoke.ctrl_m_diagnostic_allowed(False, self.snapshot()))
        self.assertFalse(smoke.ctrl_m_diagnostic_allowed(True, self.snapshot()))
        self.assertFalse(smoke.ctrl_m_diagnostic_allowed(False, {}))

    def test_foreign_or_missing_focus_and_owner_fail_closed(self):
        for key in ['active_window', 'resolved_focus_window', 'active_window_owner_pid', 'resolved_focus_window_owner_pid']:
            value = self.snapshot()
            value[key] = 12345
            with self.subTest(key=key):
                self.assertFalse(smoke.ctrl_m_diagnostic_allowed(False, value))
            del value[key]
            self.assertFalse(smoke.ctrl_m_diagnostic_allowed(False, value))

    def test_diagnostic_success_never_changes_original_return_failure_to_pass(self):
        self.assertEqual(smoke.terminal_smoke_status(True), 'FAILED_RETURN_KEY_DIAGNOSTIC')
        self.assertEqual(smoke.terminal_smoke_status(False), 'CAPTURED_REQUIRES_VISUAL_REVIEW')


class TerminationDiagnosticTests(unittest.TestCase):
    def test_signal_during_best_effort_capture_aborts_before_fallback_and_runs_cleanup(self):
        actions = []
        with self.assertRaises(smoke.SmokeInterrupted) as caught:
            try:
                try:
                    # Same best-effort Exception boundary as failure screenshot capture.
                    smoke.interrupted(15, None)
                except Exception:
                    actions.append('capture-error-recorded')
                actions.append('ctrl-m-fallback')
            finally:
                actions.append('cleanup')
        self.assertEqual(caught.exception.signum, 15)
        self.assertEqual(actions, ['cleanup'])

    def test_hup_and_interrupt_also_bypass_diagnostic_exception_handlers(self):
        for signum in [1, 2]:
            with self.subTest(signum=signum), self.assertRaises(smoke.SmokeInterrupted):
                try:
                    smoke.interrupted(signum, None)
                except Exception:
                    self.fail('termination swallowed by diagnostic handler')


class SearchReviewContractTests(unittest.TestCase):
    def test_linux_search_uses_non_readline_chord(self):
        self.assertEqual(smoke.LINUX_SEARCH_CHORD, 'ctrl+shift+f')

    def test_screenshots_never_claim_automated_search_acceptance(self):
        review = smoke.search_review_requirements()
        self.assertIs(review['automated_search_verified'], False)
        self.assertEqual(review['status'], 'REQUIRES_VISUAL_REVIEW')
        self.assertEqual(len(review['required_checks']), 4)
        self.assertTrue(any('1/1' in check for check in review['required_checks']))


class SearchInputSequenceTests(unittest.TestCase):
    def test_each_search_open_is_captured_before_query_and_cancel_is_captured(self):
        source = Path(smoke.__file__).read_text()
        tree = ast.parse(source)
        events = []
        for call in sorted((node for node in ast.walk(tree) if isinstance(node, ast.Call)), key=lambda node: node.lineno):
            if not isinstance(call.func, ast.Name) or not call.args:
                continue
            if call.func.id == 'capture' and isinstance(call.args[0], ast.Constant):
                name = call.args[0].value
                if name in ['04b-search-overlay-before-query', '06-search-cancelled', '07b-search-overlay-before-repeat-query']:
                    events.append(name)
            if call.func.id == 'run' and isinstance(call.args[0], ast.List):
                arguments = call.args[0].elts
                if any(isinstance(value, ast.Name) and value.id == 'LINUX_SEARCH_CHORD' for value in arguments):
                    events.append('open-search')
                if any(isinstance(value, ast.Name) and value.id == 'marker' for value in arguments):
                    events.append('type-query')
                self.assertFalse(any(isinstance(value, ast.Constant) and value.value == 'ctrl+a' for value in arguments))
        self.assertEqual(events, ['open-search', '04b-search-overlay-before-query', 'type-query',
                                  '06-search-cancelled', 'open-search',
                                  '07b-search-overlay-before-repeat-query', 'type-query'])


if __name__ == '__main__':
    unittest.main()
