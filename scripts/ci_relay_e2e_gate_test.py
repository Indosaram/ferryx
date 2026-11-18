"""The relay E2E gate fails on ignored, filtered, empty or missing suites; no cargo needed."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

GATE = Path(__file__).with_name("ci-relay-e2e-gate.py").resolve()


def summary(passed=1, failed=0, ignored=0, filtered=0):
    status = "FAILED" if failed else "ok"
    return ("test result: {}. {} passed; {} failed; {} ignored; 0 measured; {} filtered out; "
            "finished in 0.01s").format(status, passed, failed, ignored, filtered)


# The fake cargo is an executable shebang script found on PATH, which only POSIX runs.
@unittest.skipUnless(os.name == "posix", "fake cargo on PATH needs a POSIX shebang script")
class RelayE2EGateTests(unittest.TestCase):
    def run_gate(self, suites, exit_code=0, *, color=False):
        """Runs the gate against a fake cargo that prints one block per (target, summary line)."""
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / "cargo"
            lines = []
            for name, line in suites:
                running = "\x1b[1m\x1b[92m     Running\x1b[0m" if color else "     Running"
                lines.append("{} tests/{}.rs (target/debug/deps/{}-0)".format(running, name, name))
                lines.append(line)
            cargo.write_text("#!{}\nimport json, sys\nprint(json.dumps(sys.argv[1:]))\nprint({!r})\nsys.exit({})\n".format(
                sys.executable, "\n".join(lines), exit_code))
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ["PATH"])
            args = [sys.executable, str(GATE), "--target", "a", "--target", "b"]
            return subprocess.run(args, env=env, capture_output=True, text=True)

    def test_all_suites_ran_and_nothing_ignored_passes(self):
        result = self.run_gate([("a", summary(3)), ("b", summary(2))])
        self.assertEqual(result.returncode, 0, result.stderr)
        argv = json.loads(result.stdout.splitlines()[0])
        self.assertEqual(argv[:5], ["test", "--manifest-path", "src-tauri/Cargo.toml", "--no-fail-fast", "--test"])
        self.assertIn("a", argv)
        self.assertIn("b", argv)
        self.assertNotIn("--skip", argv)
        self.assertNotIn("--ignored", argv)

    def test_an_ignored_test_fails_the_gate(self):
        result = self.run_gate([("a", summary(3)), ("b", summary(2, ignored=1))])
        self.assertEqual(result.returncode, 1)
        self.assertIn("b: 1 ignored", result.stderr)

    def test_colored_target_headers_preserve_each_suite_verdict(self):
        passed = self.run_gate([("a", summary(3)), ("b", summary(2))], color=True)
        self.assertEqual(passed.returncode, 0, passed.stderr)
        failed = self.run_gate([("a", summary(1)), ("b", summary(7, failed=1))], exit_code=101, color=True)
        self.assertEqual(failed.returncode, 1)
        self.assertIn("b: 1 failed", failed.stderr)
        self.assertNotIn("no test summary", failed.stderr)

    def test_a_filtered_test_fails_the_gate(self):
        result = self.run_gate([("a", summary(3, filtered=2)), ("b", summary(2))])
        self.assertEqual(result.returncode, 1)
        self.assertIn("a: 2 filtered out", result.stderr)

    def test_a_suite_with_zero_tests_fails_the_gate(self):
        result = self.run_gate([("a", summary(0)), ("b", summary(2))])
        self.assertEqual(result.returncode, 1)
        self.assertIn("a: zero tests ran", result.stderr)

    def test_a_missing_suite_fails_the_gate(self):
        result = self.run_gate([("a", summary(3))])
        self.assertEqual(result.returncode, 1)
        self.assertIn("b: no test summary", result.stderr)

    def test_a_failing_cargo_run_fails_the_gate(self):
        result = self.run_gate([("a", summary(3, failed=1)), ("b", summary(2))], exit_code=101)
        self.assertEqual(result.returncode, 1)
        self.assertIn("cargo test exited 101", result.stderr)
        self.assertIn("a: 1 failed", result.stderr)


if __name__ == "__main__":
    unittest.main()
