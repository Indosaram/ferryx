"""Pure guard regressions; do not start displays, applications, or sockets."""
import importlib.util
import base64
import tempfile
import json
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
        with self.assertRaisesRegex(RuntimeError, 'interrupted by signal 15'):
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


if __name__ == '__main__':
    unittest.main()
