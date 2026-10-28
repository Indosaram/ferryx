"""Local subprocess contracts; no external commands or network access required."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

WRAPPER = Path(__file__).with_name("ci-run-bounded.py")


@unittest.skipUnless(os.name == "posix", "POSIX process groups required")
class BoundedCommandTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.log = self.root / "command.log"

    def command(self, source, timeout=5):
        return [sys.executable, str(WRAPPER), "--timeout", str(timeout),
                "--heartbeat", "0.1", "--grace", "0.1", "--log", str(self.log),
                "--", sys.executable, "-u", "-c", source]

    def test_success_and_failure_preserve_exit_and_tee_output(self):
        for code in (0, 7):
            result = subprocess.run(self.command(
                "import sys; print('stdout-marker'); print('stderr-marker', file=sys.stderr); sys.exit(%d)" % code
            ), capture_output=True, timeout=8)
            self.assertEqual(result.returncode, code)
            self.assertEqual(result.stdout, self.log.read_bytes())
            self.assertIn(b"stdout-marker\n", result.stdout)
            self.assertIn(b"stderr-marker\n", result.stdout)

    def check_stopped_console_consumer(self, terminate):
        source = ("import os,pathlib,time; "
                  "pathlib.Path(%r).write_text(str(os.getpid())); "
                  "os.write(1,b'~'*1048576); print('FULL_OUTPUT_END',flush=True); time.sleep(60)"
                  % str(self.root / "descendant.pid"))
        process = subprocess.Popen(self.command(source, timeout=5 if terminate else .5),
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            # Deliberately never drain stdout until the wrapper has already exited.
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                if self.log.exists() and b'FULL_OUTPUT_END' in self.log.read_bytes():
                    break
                time.sleep(.01)
            self.assertIn(b'FULL_OUTPUT_END', self.log.read_bytes())
            if terminate:
                process.send_signal(signal.SIGTERM)
            self.assertEqual(process.wait(timeout=4), 143 if terminate else 124)
            output = self.log.read_bytes()
            self.assertEqual(output.count(b'~'), 1048576)
            self.assertIn(b'console_backpressure', output)
            self.assertIn(b'console_dropped_bytes=', output)
            self.assert_descendant_stopped()
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=3)
            # Safety net if a regression strands the intentionally long-lived child.
            pidfile = self.root / "descendant.pid"
            if pidfile.exists():
                try:
                    os.killpg(int(pidfile.read_text()), signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_timeout_with_stopped_console_consumer(self):
        self.check_stopped_console_consumer(False)

    def test_sigterm_with_stopped_console_consumer(self):
        self.check_stopped_console_consumer(True)

    def test_stdout_shared_descriptor_blocking_flag_is_restored(self):
        read_fd, write_fd = os.pipe()
        try:
            self.assertTrue(os.get_blocking(write_fd))
            result = subprocess.run(self.command("print('small-output')"),
                                    stdout=write_fd, stderr=subprocess.PIPE, timeout=4)
            self.assertEqual(result.returncode, 0)
            self.assertTrue(os.get_blocking(write_fd))
        finally:
            os.close(write_fd)
            os.close(read_fd)

    def test_child_signal_exit_is_reported_without_success_masking(self):
        result = subprocess.run(self.command(
            "import os,signal; os.kill(os.getpid(), signal.SIGTERM)"
        ), capture_output=True, timeout=8)
        self.assertEqual(result.returncode, 128 + signal.SIGTERM)

    def test_spawn_failure_does_not_echo_command_arguments(self):
        command = self.command("unused")
        command = command[:command.index("--") + 1] + ["/missing/secret-command-argument"]
        result = subprocess.run(command, capture_output=True, timeout=8)
        self.assertEqual(result.returncode, 127)
        self.assertIn(b"spawn_failed error_type=FileNotFoundError", result.stdout)
        self.assertNotIn(b"secret-command-argument", result.stdout + result.stderr)

    def test_heartbeat_has_no_command_arguments_or_environment(self):
        secret = "never-print-this-argument-or-env"
        source = "import time; time.sleep(0.35) # " + secret
        result = subprocess.run(self.command(source), capture_output=True, timeout=8,
                                env={**os.environ, "BOUNDED_TEST_SECRET": secret})
        self.assertEqual(result.returncode, 0)
        self.assertIn(b"heartbeat elapsed_seconds=", result.stdout)
        self.assertNotIn(secret.encode(), result.stdout + result.stderr + self.log.read_bytes())

    def descendant_source(self, leader_exit=False):
        pidfile = str(self.root / "descendant.pid")
        child = ("import os,signal,time,pathlib; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                 "pathlib.Path(%r).write_text(str(os.getpid())); time.sleep(60)" % pidfile)
        return ("import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',%r]); " % child
                + ("time.sleep(0.2)" if leader_exit else "time.sleep(60)"))

    def assert_descendant_stopped(self):
        pid = int((self.root / "descendant.pid").read_text())
        # On Linux orphan zombies may persist until the container's PID 1 reaps them;
        # a zombie is terminated and cannot run or hold the inherited output pipe.
        for _ in range(100):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            stat = Path("/proc") / str(pid) / "stat"
            if stat.exists() and stat.read_text().rsplit(")", 1)[1].split()[0] == "Z":
                return
            time.sleep(0.02)
        self.fail("owned descendant survived process-group cleanup")

    def test_timeout_kills_term_ignoring_descendant(self):
        result = subprocess.run(self.command(self.descendant_source(), timeout=0.5),
                                capture_output=True, timeout=8)
        self.assertEqual(result.returncode, 124)
        self.assertIn(b"shutdown reason=timeout", result.stdout)
        self.assert_descendant_stopped()

    def test_success_also_cleans_up_surviving_descendants(self):
        result = subprocess.run(self.command(self.descendant_source(leader_exit=True)),
                                capture_output=True, timeout=8)
        self.assertEqual(result.returncode, 0)
        self.assert_descendant_stopped()

    def test_sigterm_cleans_up_group_and_preserves_interrupted_status(self):
        process = subprocess.Popen(self.command(self.descendant_source()), stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.kill() if process.poll() is None else None)
        deadline = time.monotonic() + 4
        while not (self.root / "descendant.pid").exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertTrue((self.root / "descendant.pid").exists())
        process.send_signal(signal.SIGTERM)
        stdout, _ = process.communicate(timeout=8)
        self.assertEqual(process.returncode, 128 + signal.SIGTERM)
        self.assertIn(b"shutdown reason=signal", stdout)
        self.assert_descendant_stopped()

    def test_log_is_flushed_before_process_finishes(self):
        process = subprocess.Popen(self.command("import time; print('live-marker'); time.sleep(0.7)"),
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.kill() if process.poll() is None else None)
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if self.log.exists() and b"live-marker" in self.log.read_bytes():
                break
            time.sleep(0.01)
        self.assertIn(b"live-marker", self.log.read_bytes())
        self.assertIsNone(process.poll())
        process.communicate(timeout=8)
        self.assertEqual(process.returncode, 0)


if __name__ == "__main__":
    unittest.main()
