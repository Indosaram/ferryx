#!/usr/bin/env python3
"""Bound a POSIX CI command, tee its output, and clean up its owned process group.

Diagnostics never print command arguments or environment values. The command's own
stdout/stderr is preserved in the logfile. Live console output is best-effort
under backpressure, with dropped-byte accounting in that logfile. Descendants must remain in the
new process group (a deliberately daemonized/setsid process is outside this contract).
"""
import argparse
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import time


def positive(value):
    number = float(value)
    if not 0 < number < float("inf"):
        raise argparse.ArgumentTypeError("must be finite and greater than zero")
    return number


def group_signal(pgid, signum):
    try:
        os.killpg(pgid, signum)
    except ProcessLookupError:
        pass


def group_exists(pgid):
    try:
        os.killpg(pgid, 0)
        return True
    except ProcessLookupError:
        return False


def resources(pgid):
    """Read only numeric process metadata; never request argv or environment."""
    try:
        result = subprocess.run(
            ["ps", "-axo", "pgid=,rss=,pcpu="], capture_output=True,
            text=True, timeout=2, check=True,
        )
        rows = [line.split() for line in result.stdout.splitlines()]
        owned = [row for row in rows if len(row) == 3 and int(row[0]) == pgid]
        return "processes={} rss_kib={} cpu_percent={:.1f}".format(
            len(owned), sum(int(row[1]) for row in owned),
            sum(float(row[2]) for row in owned),
        )
    except (OSError, subprocess.SubprocessError, ValueError):
        return "resources=unavailable"


def run(args):
    requested_signal = None
    previous_handlers = {}

    def interrupted(signum, _frame):
        nonlocal requested_signal
        requested_signal = signum

    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        previous_handlers[signum] = signal.signal(signum, interrupted)

    child = None
    selector = selectors.DefaultSelector()
    stdout_available = True
    stdout_fd = sys.stdout.fileno()
    stdout_was_blocking = os.get_blocking(stdout_fd)
    console_dropped = 0
    os.set_blocking(stdout_fd, False)
    started = time.monotonic()
    try:
        with Path(args.log).open("wb") as log:
            def emit(data):
                nonlocal stdout_available, console_dropped
                log.write(data)
                log.flush()
                # Never let an undrained CI log pipe suspend deadline/signal handling.
                # Child reads are bounded at 64 KiB; no unbounded console queue exists.
                remaining = memoryview(data)
                while stdout_available and remaining:
                    try:
                        written = os.write(stdout_fd, remaining)
                        if written == 0:
                            break
                        remaining = remaining[written:]
                    except BlockingIOError:
                        break
                    except BrokenPipeError:
                        stdout_available = False
                if remaining:
                    if console_dropped == 0:
                        log.write(b"\n[ci-bounded] console_backpressure live_output_dropped; full command output remains in logfile\n")
                    console_dropped += len(remaining)
                    log.flush()

            def diagnostic(message):
                emit(("[ci-bounded] " + message + "\n").encode())

            try:
                child = subprocess.Popen(
                    args.command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
            except OSError as error:
                diagnostic("spawn_failed error_type=" + type(error).__name__)
                return 127
            pgid = child.pid
            os.set_blocking(child.stdout.fileno(), False)
            selector.register(child.stdout, selectors.EVENT_READ)
            diagnostic("started leader_pid={} timeout_seconds={:g}".format(pgid, args.timeout))
            next_heartbeat = started + args.heartbeat
            shutdown_at = None
            final_code = None
            killed_at = None
            while True:
                now = time.monotonic()
                code = child.poll()
                if shutdown_at is None:
                    if requested_signal is not None:
                        final_code = 128 + requested_signal
                        reason = "signal signal_number=" + str(requested_signal)
                    elif now - started >= args.timeout:
                        final_code = 124
                        reason = "timeout"
                    elif code is not None:
                        final_code = code if code >= 0 else 128 - code
                        reason = "leader_exit exit_code=" + str(final_code)
                    else:
                        reason = None
                    if reason:
                        diagnostic("shutdown reason=" + reason)
                        group_signal(pgid, signal.SIGTERM)
                        shutdown_at = now
                if shutdown_at is not None and killed_at is None and now - shutdown_at >= args.grace:
                    group_signal(pgid, signal.SIGKILL)
                    diagnostic("kill_group leader_pid=" + str(pgid))
                    killed_at = now
                if now >= next_heartbeat:
                    diagnostic("heartbeat elapsed_seconds={:.1f} leader_pid={} {}".format(
                        now - started, pgid, resources(pgid)))
                    next_heartbeat = time.monotonic() + args.heartbeat
                for key, _ in selector.select(timeout=0.05):
                    try:
                        data = os.read(key.fd, 65536)
                    except BlockingIOError:
                        continue
                    if data:
                        emit(data)
                    else:
                        selector.unregister(key.fileobj)
                if shutdown_at is not None:
                    if child.poll() is not None and not selector.get_map() and not group_exists(pgid):
                        break
                    # A descendant may have escaped the group while retaining the pipe.
                    # Never let it extend this command's bounded cleanup indefinitely.
                    if killed_at is not None and time.monotonic() - killed_at >= 1:
                        break
            child.wait(timeout=1)
            diagnostic("finished exit_code={} elapsed_seconds={:.1f}".format(
                final_code, time.monotonic() - started))
            if console_dropped:
                log.write(("[ci-bounded] console_dropped_bytes={} logfile_complete=true\n".format(
                    console_dropped)).encode())
                log.flush()
            return final_code
    finally:
        try:
            if child is not None:
                group_signal(child.pid, signal.SIGKILL)
                child.wait(timeout=2)
                child.stdout.close()
        finally:
            selector.close()
            os.set_blocking(stdout_fd, stdout_was_blocking)
            for signum, handler in previous_handlers.items():
                signal.signal(signum, handler)



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", "--timeout-seconds", dest="timeout", type=positive, required=True)
    parser.add_argument("--heartbeat", "--heartbeat-seconds", dest="heartbeat", type=positive, default=30)
    parser.add_argument("--grace", "--term-grace-seconds", dest="grace", type=positive, default=10)
    parser.add_argument("--log", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command and args.command[0] == "--":
        args.command = args.command[1:]
    if os.name != "posix":
        parser.error("this process-group wrapper supports macOS/Linux only")
    if not args.command:
        parser.error("a command after -- is required")
    Path(args.log).parent.mkdir(parents=True, exist_ok=True)
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
