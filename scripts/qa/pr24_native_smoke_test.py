"""Pure guard regressions; do not start displays, applications, or sockets."""
import importlib.util
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


if __name__ == '__main__':
    unittest.main()
