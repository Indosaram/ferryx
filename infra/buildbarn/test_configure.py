import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('configure', HERE / 'configure.py')
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


class ConfigurationTest(unittest.TestCase):
    def setUp(self):
        self.hosts = c.roster((HERE / 'hosts.example').read_text())
        self.profile = json.loads((HERE / 'profile.example.json').read_text())

    def test_all_matching_workers_share_queue_not_host_selection(self):
        f = c.render(self.hosts, self.profile)
        a = f['workers/linux-a/worker.json']['buildDirectories'][0]['runners'][0]
        b = f['workers/linux-b/worker.json']['buildDirectories'][0]['runners'][0]
        mac = f['workers/mac-a/worker.json']['buildDirectories'][0]['runners'][0]
        self.assertEqual(a['platform'], b['platform'])
        self.assertNotEqual(a['platform'], mac['platform'])
        self.assertNotEqual(a['workerId'], b['workerId'])
        self.assertEqual(2, sum(p.endswith('/.buckroot') for p in f))
        self.assertEqual(['windows-a'], f['manifest.json']['excluded'])

    def test_endpoints_and_storage_graph(self):
        self.profile['coordinator']['address'] = '100.100.1.2'
        f = c.render(self.hosts, self.profile)
        self.assertEqual('100.100.1.2:8983', f['workers/mac-a/worker.json']['scheduler']['address'])
        self.assertEqual('127.0.0.1:8982', f['coordinator/frontend.json']['schedulers']['']['endpoint']['address'])
        self.assertEqual(['100.100.1.2:8983'], f['coordinator/scheduler.json']['workerGrpcServers'][0]['listenAddresses'])
        self.assertEqual('100.100.1.2:8981', f['coordinator/frontend.json']['contentAddressableStorageServer']['contentAddressableStorage']['chunkStorage']['grpc']['client']['address'])
        for name in self.profile['workers']:
            worker = f[f'workers/{name}/worker.json']
            runner = f[f'workers/{name}/runner.json']
            self.assertEqual(runner['buildDirectoryPath'], worker['buildDirectories'][0]['native']['buildDirectoryPath'])
            self.assertEqual('unix:' + runner['grpcServers'][0]['listenPaths'][0], worker['buildDirectories'][0]['runners'][0]['endpoint']['address'])

    def test_roster_floor_comments_duplicates_malformed(self):
        self.assertEqual('host', c.roster('a ssh-unix host 20 7 # constrained host')['a']['target'])
        for text in ['a ssh-unix', 'a unknown host', '../a ssh-unix host', 'a ssh-unix h\na ssh-unix h2', 'a ssh-unix h nope']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                c.roster(text)

    def test_bad_profiles_fail_closed(self):
        for key, value in [('os', 'windows'), ('arch', 'unknown'), ('concurrency', 0), ('concurrency', True), ('concurrency', 65), ('state_dir', '../tmp'), ('toolchain', 'x\n[evil]'), ('state_dir', '/' + 'a' * 110)]:
            p = copy.deepcopy(self.profile)
            p['workers']['linux-a'][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                c.render(self.hosts, p)

    def test_unsupported_transport_is_not_silently_provisioned(self):
        for transport in ['ssh-win32', 'instabox', 'salad', 'ssh-darwin']:
            hosts = copy.deepcopy(self.hosts)
            hosts['linux-a']['transport'] = transport
            with self.subTest(transport=transport), self.assertRaises(ValueError):
                c.render(hosts, self.profile)

    def test_missing_roster_host_and_empty_workers(self):
        for workers in [{}, {'missing': self.profile['workers']['linux-a']}]:
            p = copy.deepcopy(self.profile)
            p['workers'] = workers
            with self.assertRaises(ValueError):
                c.render(self.hosts, p)

    def test_public_or_wildcard_coordinator_rejected(self):
        for address in ['0.0.0.0', '8.8.8.8', '169.254.1.1', 'example.com', '::1']:
            p = copy.deepcopy(self.profile)
            p['coordinator']['address'] = address
            with self.subTest(address=address), self.assertRaises(ValueError):
                c.render(self.hosts, p)

    def test_generated_json_and_remote_only_buck(self):
        f = c.render(self.hosts, self.profile)
        for name, content in f.items():
            if name.endswith('.json'):
                self.assertEqual(content, json.loads(json.dumps(content)))
            if name.endswith('defs.bzl'):
                self.assertIn('local_enabled = False', content)
                self.assertIn('remote_enabled = True', content)
                self.assertIn('range(8)', content)
                self.assertNotIn('local_only = True', content)
            if name.endswith('.buckconfig'):
                self.assertIn('instance_name =\n', content)
                self.assertIn('execution_platforms = root//:platforms', content)

    def test_rendered_shell_command_success_and_failure(self):
        import ast
        import os
        tree = ast.parse(c.buck_rules({'OSFamily': 'linux'}))
        command = next(node.value for node in ast.walk(tree) if isinstance(node, ast.Constant) and isinstance(node.value, str) and node.value.startswith('hostname >'))
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'out'
            result = subprocess.run(['/bin/sh', '-c', command, '--', str(out), '3', 'fresh-nonce'], capture_output=True)
            self.assertEqual(0, result.returncode, result.stderr)
            self.assertIn('buildbarn-smoke-3-fresh-nonce', out.read_text())
            # A missing hostname command must fail, not produce a success marker.
            result = subprocess.run(['/bin/sh', '-c', command, '--', str(out), '3', 'fresh-nonce'], env=dict(os.environ, PATH=tmp), capture_output=True)
            self.assertNotEqual(0, result.returncode)
            self.assertNotIn('buildbarn-smoke', out.read_text())

    def test_atomic_output_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'output'
            c.write_output(output, c.render(self.hosts, self.profile))
            before = (output / 'manifest.json').read_bytes()
            with self.assertRaises(ValueError):
                c.write_output(output, {'manifest.json': 'destroyed'})
            self.assertEqual(before, (output / 'manifest.json').read_bytes())
            self.assertEqual(0o600, (output / 'manifest.json').stat().st_mode & 0o777)

    def test_cli_environment_override_and_no_overwrite(self):
        import os
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'rendered'
            command = [sys.executable, str(HERE / 'configure.py'), '--ferryx-inventory', '--profile', str(HERE / 'profile.example.json'), '--output', str(out)]
            env = dict(os.environ, FERRYX_BUILD_HOSTS_FILE=str(HERE / 'hosts.example'))
            first = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(0, first.returncode, first.stderr)
            self.assertIn('no hosts contacted', first.stdout)
            second = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(2, second.returncode)
            self.assertIn('output already exists', second.stderr)

    def test_standalone_profile_needs_no_ferryx_inventory(self):
        f = c.render(None, self.profile)
        self.assertFalse(f['manifest.json']['inventory_supplied'])
        self.assertEqual([], f['manifest.json']['excluded'])
        self.assertEqual(3, len(f['manifest.json']['workers']))
        self.assertNotIn('target', f['manifest.json']['workers']['linux-a'])
        self.assertIn('toolchain-id', f['manifest.json']['workers']['linux-a']['properties'])

    def test_standalone_cli_ignores_ferryx_environment(self):
        import os
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, FERRYX_BUILD_HOSTS_FILE=str(Path(tmp) / 'missing'))
            env.pop('BUILDBARN_HOSTS_FILE', None)
            command = [sys.executable, str(HERE / 'configure.py'), '--profile', str(HERE / 'profile.example.json'), '--output', str(Path(tmp) / 'output')]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(0, result.returncode, result.stderr)

    def test_generic_inventory_environment(self):
        import os
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, BUILDBARN_HOSTS_FILE=str(HERE / 'hosts.example'))
            output = Path(tmp) / 'output'
            command = [sys.executable, str(HERE / 'configure.py'), '--profile', str(HERE / 'profile.example.json'), '--output', str(output)]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(0, result.returncode, result.stderr)
            self.assertTrue(json.loads((output / 'manifest.json').read_text())['inventory_supplied'])

    def test_standalone_rejects_path_traversal_worker_name(self):
        profile = copy.deepcopy(self.profile)
        profile['workers']['../escape'] = profile['workers'].pop('linux-a')
        with self.assertRaises(ValueError):
            c.render(None, profile)

    def test_validation_failure_leaves_no_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'output'
            result = subprocess.run([sys.executable, str(HERE / 'configure.py'), '--hosts', str(Path(tmp) / 'missing'), '--profile', str(HERE / 'profile.example.json'), '--output', str(output)], capture_output=True)
            self.assertEqual(2, result.returncode)
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
