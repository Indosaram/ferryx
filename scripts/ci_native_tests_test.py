"""Verify diagnostic staging preserves the complete native suite contract."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ci-native-tests.sh').resolve()
TARGETS = '''backend_hardening daemon_handover_contract daemon_handover_transfer_contract
 daemon_persistence_contract e2e_agent_workflow ghostty_build_contract ghostty_build_info_ffi
 ipc_hardening_contract native_terminal_capability_contract native_terminal_child_surface_contract
 native_terminal_composition_contract native_terminal_engine_contract native_terminal_input_boundary_contract
 native_terminal_surface_host_contract native_terminal_wayland_subsurface_contract platform_build_contract rorca_native_contract
 session_persistence_integration updater_config_contract updater_endpoint_contract worktree_safety'''.split()


class NativeSuiteTests(unittest.TestCase):
    def invoke(self, mode, suite):
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / 'cargo'
            cargo.write_text('#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps(sys.argv[1:]))\n')
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ['PATH'], TEST_SUITE=suite)
            return subprocess.run(['bash', str(SCRIPT), mode], env=env, capture_output=True, text=True)

    def test_compile_and_run_use_the_original_suite_and_skips(self):
        for suite in ('full', 'integration'):
            with self.subTest(suite=suite):
                expected = ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--target',
                            'aarch64-apple-darwin', '--no-fail-fast']
                if suite == 'full':
                    expected += ['--lib']
                for target in TARGETS:
                    expected += ['--test', target]
                expected += ['--', '--test-threads=1', '--skip', 'native_terminal::renderer::',
                             '--skip', 'terminal::tests::test_spawn_startup_command_omo']
                for mode in ('compile', 'run'):
                    result = self.invoke(mode, suite)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    args = json.loads(result.stdout.splitlines()[-1])
                    if mode == 'compile':
                        self.assertEqual(args.pop(1), '--no-run')
                    self.assertEqual(args, expected)

    def test_invalid_mode_or_scope_fails_instead_of_selecting_less_coverage(self):
        self.assertNotEqual(self.invoke('invalid', 'full').returncode, 0)
        self.assertNotEqual(self.invoke('run', 'invalid').returncode, 0)


if __name__ == '__main__':
    unittest.main()
