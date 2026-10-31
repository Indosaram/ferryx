#!/usr/bin/env python3
"""todo-9 live E2E driver for plan rex-learnings-20261030 (OSC 7501 program status).

python3 stdlib only. Talks to an isolated `ferryx-cli --daemon` over its unix socket.
Source references are for branch feat/rex-20261030 (src-tauri/src/...).

Wire facts this driver relies on:
- Socket: $FERRYX_RUNTIME_DIR/daemon.sock (daemon/server.rs:544 get_runtime_dir, :612 get_socket_path).
- Framing: one JSON object per line, both directions (daemon/server.rs:6033 read_line +
  serde_json::from_str; responses `resp_json.push('\\n')` :8045; stream frames
  protocol.rs:1773 encode_daemon_stream_frame). No length prefix.
- Requests: `DaemonRequest`, `#[serde(tag = "type", rename_all = "camelCase")]`, fields camelCase
  (protocol.rs:638). Responses: `DaemonResponse`, same tagging (protocol.rs:1134).
- Handshake {version: 5} -> handshakeOk {pid, epoch, ...} (protocol.rs:15, :646, :1212; server.rs:6143).
- Bytes travel base64 (protocol.rs base64_serde): write.data, attachOk.history, output.data.
- After attachOk the connection is a one-way stream of DaemonStreamMessage frames
  (protocol.rs:1526): output, gap, replayGap, agentState, exit, remoteStatus. Any byte the client
  sends on it ends the stream (server.rs:9154).
- agentState frame (protocol.rs:1562): sessionId, state, agent?, detail?, isSnapshot?, origin?
  (omitted for Agent and Protocol, :1712 is_wire_default), stateSource "protocol"? (:1682),
  outcome "done"|"error"? (:1689), kind "permission"|"question"|"auth"? (:1697).
- `ferryx-cli agents events` == {type: agentControl, request: {op: events, after}} answered by
  agentControlOk {result: {gap, events|snapshot, afterSequence}} (agent_control/mod.rs:44,:93;
  remote/agent_control_service.rs:201,:471). Its readings (agent_control/events.rs:33) carry
  state/stateSource(provider|screen|none)/tier/kind-of-screen-prompt only: no origin, no outcome.
"""
import base64
import json
import os
import re
import resource
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid

PROTOCOL_VERSION = 5  # protocol.rs:15 DAEMON_PROTOCOL_VERSION (non-QA build)
QUERY = b"\x1b]7501;?\x1b\\"
# native_terminal/program_status.rs:308 and terminal/program_status_ingest.rs:92 QUERY_REPLY
QUERY_REPLY = b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\"
REPLY_PREFIX = b"\x1b]7501;?:"
HERE = os.path.dirname(os.path.abspath(__file__))
CLI = os.path.abspath(os.environ.get("FERRYX_CLI", os.path.join(HERE, "..", "ferryx-cli")))
HELPER = os.path.abspath(os.environ.get("FERRYX_HELPER", os.path.join(HERE, "..", "ferryx-remote-helper")))
LOG_DIR = os.path.abspath(os.environ.get("E2E_LOG_DIR", os.getcwd()))
# The user's own HOME, where a logged-in Claude Code keeps its login. Panes get an isolated HOME;
# only the `claude` scenario puts this one back, for the claude process alone.
REAL_HOME = os.environ.get("E2E_REAL_HOME", os.environ.get("HOME", ""))
STATE_TIMEOUT = 30.0


class Log:
    def __init__(self, name):
        self.name = name
        self.failed = False
        self.path = os.path.join(LOG_DIR, f"task-9-{name}.log")
        self.fh = open(self.path, "w", buffering=1)

    def line(self, text):
        self.fh.write(text + "\n")
        print(text, flush=True)

    def info(self, text):
        self.line(f"INFO {text}")

    def check(self, ok, what, detail=""):
        self.line(f"{'PASS' if ok else 'FAIL'} {what}" + (f" {detail}" if detail else ""))
        if not ok:
            self.failed = True
        return ok

    def verdict(self, skip=None):
        if skip:
            self.line(f"SCENARIO {self.name} SKIP {skip}")
        else:
            self.line(f"SCENARIO {self.name} {'FAIL' if self.failed else 'PASS'}")


class Conn:
    """One daemon connection: newline-delimited JSON."""

    def __init__(self, path, timeout=30.0):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        self.sock.connect(path)
        self.buf = b""
        self.hello = self.request({"type": "handshake", "version": PROTOCOL_VERSION})
        if self.hello.get("type") != "handshakeOk":
            raise RuntimeError(f"handshake refused: {self.hello}")

    def send(self, obj):
        self.sock.sendall(json.dumps(obj).encode() + b"\n")

    def read(self):
        while b"\n" not in self.buf:
            chunk = self.sock.recv(1 << 16)
            if not chunk:
                raise EOFError("daemon closed the connection")
            self.buf += chunk
        line, self.buf = self.buf.split(b"\n", 1)
        return json.loads(line)

    def request(self, obj):
        self.send(obj)
        return self.read()

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


class Stream:
    """An attached session stream (Attach -> attachOk -> frames), read on a thread."""

    def __init__(self, sock_path, session_id):
        self.conn = Conn(sock_path)
        ok = self.conn.request({"type": "attach", "sessionId": session_id, "afterSequence": None})
        if ok.get("type") != "attachOk":
            raise RuntimeError(f"attach failed: {ok}")
        self.history = base64.b64decode(ok.get("history") or "")
        self.conn.sock.settimeout(None)
        self.frames = []
        self.output = bytearray(self.history)
        self.cond = threading.Condition()
        self.closed = False
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self):
        try:
            while True:
                frame = self.conn.read()
                with self.cond:
                    if frame.get("type") == "output":
                        self.output += base64.b64decode(frame["data"])
                    self.frames.append(frame)
                    self.cond.notify_all()
        except (EOFError, OSError, ValueError):
            with self.cond:
                self.closed = True
                self.cond.notify_all()

    def states(self, start=0):
        with self.cond:
            return [f for f in self.frames[start:] if f.get("type") == "agentState"]

    def mark(self):
        with self.cond:
            return len(self.frames)

    def wait(self, pred, timeout=STATE_TIMEOUT, start=0, tick=None):
        """First agentState frame at index >= start matching pred, or None after timeout.
        `tick` runs about once a second (event-driven wait with a bounded slice)."""
        deadline = time.monotonic() + timeout
        with self.cond:
            while True:
                for frame in self.frames[start:]:
                    if frame.get("type") == "agentState" and pred(frame):
                        return frame
                left = deadline - time.monotonic()
                if left <= 0 or self.closed:
                    return None
                self.cond.wait(min(left, 1.0))
                if tick:
                    self.cond.release()
                    try:
                        tick()
                    finally:
                        self.cond.acquire()

    def close(self):
        self.conn.close()


def is_protocol(f):
    return f.get("stateSource") == "protocol"


def is_agent_origin(f):
    return "origin" not in f and "stateSource" not in f


def describe(f):
    keys = ("state", "origin", "stateSource", "outcome", "kind", "isSnapshot", "detail")
    return json.dumps({k: f[k] for k in keys if k in f}, sort_keys=True)


class Daemon:
    """An isolated `ferryx-cli --daemon` (cli.rs:4093 run_daemon_headless)."""

    def __init__(self, log, tag, integration=True, claude_dir=False):
        self.log = log
        self.root = tempfile.mkdtemp(prefix=f"ferryx-e2e-{tag}-")
        self.runtime = os.path.join(self.root, "runtime")
        self.data = os.path.join(self.root, "data")
        self.home = os.path.join(self.root, "home")
        self.work = os.path.join(self.root, "work")
        for d in (self.runtime, self.data, self.home, self.work, os.path.join(self.work, "bin")):
            os.makedirs(d, mode=0o700, exist_ok=True)
        with open(os.path.join(self.home, ".bashrc"), "w") as fh:
            # No OSC 133 shell integration: prompt-start pruning must not mask R12 release pruning.
            fh.write("PS1='$ '\nunset PROMPT_COMMAND\n")
        if claude_dir:
            # agent_hook_install.rs:680 installs only where the agent's config dir exists.
            os.makedirs(os.path.join(self.home, ".claude"), exist_ok=True)
        if not integration:
            # agent_extension.rs:28,:115 <FERRYX_DATA_DIR>/remote/agent-integration.json; an explicit
            # opt-out gives panes no agent-state endpoint (terminal/pty.rs:537-552) and skips the
            # hook install (agent_extension.rs:325).
            os.makedirs(os.path.join(self.data, "remote"), exist_ok=True)
            with open(os.path.join(self.data, "remote", "agent-integration.json"), "w") as fh:
                json.dump({"enabled": False}, fh)
        shutil.copy(os.path.join(HERE, "emitter.sh"), os.path.join(self.work, "bin", "claude"))
        env = {k: v for k, v in os.environ.items() if not k.startswith("FERRYX_") and k != "NOTIFY_SOCKET"}
        env.update({
            "FERRYX_RUNTIME_DIR": self.runtime,   # daemon/server.rs:544
            "FERRYX_DATA_DIR": self.data,         # daemon/server.rs:848, remote/auth.rs:48
            "FERRYX_SESSION_DIR": os.path.join(self.data, "session"),  # daemon/server.rs:419
            "HOME": self.home,                    # hook/skill targets: agent_hook_install.rs:115
            "XDG_CONFIG_HOME": os.path.join(self.home, ".config"),  # agent_hook_install.rs:122
            "SHELL": "/bin/bash",
            "TERM": "xterm-256color",
        })
        self.env = env
        self.socket = os.path.join(self.runtime, "daemon.sock")
        self.err = open(os.path.join(self.root, "daemon.stderr"), "w")
        self.proc = subprocess.Popen([CLI, "--daemon"], env=env, stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=self.err, start_new_session=True)
        ready = threading.Event()

        def drain():
            for raw in self.proc.stdout:
                if b"FERRYX_DAEMON_READY" in raw:  # cli.rs:4204
                    ready.set()
            ready.set()

        threading.Thread(target=drain, daemon=True).start()
        if not ready.wait(60) or self.proc.poll() is not None:
            raise RuntimeError(f"daemon did not become ready (exit {self.proc.poll()}), see {self.root}")
        self.ctl = Conn(self.socket)
        self.pid = self.ctl.hello["pid"]
        log.info(f"daemon pid={self.pid} root={self.root} version={self.ctl.hello.get('daemonVersion')}")
        reg = self.ctl.request({"type": "registerWorkspace", "workspaceId": "e2e",
                                "repoRoot": os.path.realpath(self.work)})  # protocol.rs:679
        if reg.get("type") != "registerWorkspaceOk":
            raise RuntimeError(f"registerWorkspace failed: {reg}")

    def reconnect(self):
        self.ctl.close()
        self.ctl = Conn(self.socket)
        self.pid = self.ctl.hello["pid"]

    def spawn(self, cols=120, rows=40):
        resp = self.ctl.request({"type": "spawn", "clientRequestId": str(uuid.uuid4()),
                                 "workspaceId": "e2e", "cols": cols, "rows": rows,
                                 "shell": "/bin/bash"})  # protocol.rs:702, shell.rs:370 custom shell
        if resp.get("type") != "spawnOk":
            raise RuntimeError(f"spawn failed: {resp}")
        return resp["sessionId"]

    def write(self, sid, data):
        resp = self.ctl.request({"type": "write", "sessionId": sid,
                                 "data": base64.b64encode(data).decode()})  # protocol.rs:734
        if resp.get("type") != "writeOk":
            raise RuntimeError(f"write failed: {resp}")

    def sessions(self):
        resp = self.ctl.request({"type": "listSessions"})  # protocol.rs:844 -> :1364
        return sorted(resp.get("sessions", []))

    def agent_control(self, op):
        resp = self.ctl.request({"type": "agentControl", "request": op})  # protocol.rs:1106
        if resp.get("type") != "agentControlOk":
            raise RuntimeError(f"agentControl {op.get('op')} failed: {resp}")
        return resp["result"]

    def events(self, after=0):
        return self.agent_control({"op": "events", "after": after})

    def observer_wait(self, sid, until, after, timeout_ms=15000):
        """`ferryx-cli agents wait` (agent_control/mod.rs:83; agent_control_service.rs:445)."""
        old = self.ctl.sock.gettimeout()
        self.ctl.sock.settimeout(timeout_ms / 1000 + 20)
        try:
            return self.agent_control({"op": "wait", "sessionId": sid, "until": until,
                                       "after": after, "timeoutMs": timeout_ms})
        except RuntimeError as error:
            return {"error": str(error)}
        finally:
            self.ctl.sock.settimeout(old)

    def acknowledge(self, sid):
        return self.ctl.request({"type": "acknowledgeAgentOutcome", "sessionId": sid})  # protocol.rs:864

    def upgrade(self, path):
        return self.ctl.request({"type": "upgradeBinary", "newBinaryPath": path})  # protocol.rs:1065

    def shutdown(self):
        try:
            Conn(self.socket, timeout=10).send({"type": "shutdown"})  # protocol.rs:1100, server.rs:8016
        except (OSError, RuntimeError, EOFError, ValueError):
            pass
        try:
            self.proc.wait(15)
        except subprocess.TimeoutExpired:
            os.killpg(self.proc.pid, signal.SIGKILL)
        if self.pid and self.pid != self.proc.pid:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except OSError:
                pass


class Emitter:
    """Drives emitter.sh in a session through FIFOs (no sleeps)."""

    def __init__(self, daemon, sid):
        self.d, self.sid = daemon, sid
        self.dir = tempfile.mkdtemp(prefix="emit-", dir=daemon.work)
        os.mkfifo(os.path.join(self.dir, "go"))
        os.mkfifo(os.path.join(self.dir, "ack"))
        # argv `bash <work>/bin/claude` -> foreground.rs agent_identity == "claude"
        daemon.write(sid, f"E2E_DIR={self.dir} bash {daemon.work}/bin/claude\r".encode())

    def step(self, text, timeout=60):
        result = {}

        def run():
            with open(os.path.join(self.dir, "go"), "w") as go:
                go.write(text + "\n")
            with open(os.path.join(self.dir, "ack")) as ack:
                result["ack"] = ack.readline().strip()

        worker = threading.Thread(target=run, daemon=True)
        worker.start()
        worker.join(timeout)
        if result.get("ack") != text:
            raise RuntimeError(f"emitter did not acknowledge {text!r}")

    def report(self, body):
        self.step("report:" + body)

    def capture(self, tag, query=True):
        self.step(("query:" if query else "drain:") + tag)
        with open(os.path.join(self.dir, f"cap-{tag}.bin"), "rb") as fh:
            return fh.read()


def check_replies(log, what, captured, expected):
    count = captured.count(REPLY_PREFIX)
    ok = count == expected and (expected == 0 or captured.count(QUERY_REPLY) == expected)
    log.check(ok, what, f"replies={count} expected={expected} bytes={captured[:200]!r}")


def observer_head(d):
    return d.events(0).get("afterSequence", 0)


def drive_reports(log, d, em, stream, seq):
    """working -> blocked(permission) -> done, each confirmed on the attach stream (protocol
    frame) and by the observer (`agents wait`, stateSource provider)."""
    plan = [
        ("state=working:app=e2e", "working", lambda f: f["state"] == "working" and is_protocol(f)),
        ("state=blocked:kind=permission:app=e2e", "blocked",
         lambda f: f["state"] == "blocked" and f.get("kind") == "permission" and is_protocol(f)),
        ("state=done:app=e2e", "idle",
         lambda f: f["state"] == "idle" and f.get("outcome") == "done" and is_protocol(f)),
    ]
    for body, until, pred in plan:
        start = stream.mark()
        em.report(body)
        frame = stream.wait(pred, start=start)
        log.check(frame is not None, f"stream {body}", describe(frame or {}))
        hit = d.observer_wait(em.sid, until, seq)
        obs = hit.get("observation") or {}
        log.check(obs.get("state") == until and obs.get("stateSource") == "provider",
                  f"agents-wait {until}", json.dumps(hit, sort_keys=True)[:400])
        seq = max(seq, obs.get("sequence", seq))
    return seq


def scenario_emitter(log):
    d = Daemon(log, "emitter")
    try:
        sid = d.spawn()
        em = Emitter(d, sid)
        check_replies(log, "query-0-attached-clients", em.capture("q0"), 1)
        stream = Stream(d.socket, sid)
        check_replies(log, "query-1-attached-client", em.capture("q1"), 1)
        gui = Stream(d.socket, sid)  # GUI-equivalent second attach
        check_replies(log, "query-2-attached-clients", em.capture("q2"), 1)
        both = em.capture("q3", query=False)
        check_replies(log, "no-stray-reply-after-queries", both, 0)
        seq = drive_reports(log, d, em, stream, observer_head(d))
        sequence = [f["state"] for f in stream.states() if not f.get("isSnapshot") and is_protocol(f)]
        log.check(sequence == ["working", "blocked", "idle"], "protocol-edge-sequence", str(sequence))
        log.check(sequence == [f["state"] for f in gui.states() if not f.get("isSnapshot") and is_protocol(f)],
                  "second-client-sees-same-edges")
        ev = d.events(0)
        mine = [e for e in ev.get("events", ev.get("snapshot", [])) if e.get("sessionId") == sid]
        log.info("agents-events " + json.dumps([(e["state"], e["stateSource"]) for e in mine]))
        log.info("agents-events carries no origin/outcome fields (agent_control/events.rs:33)")
        em.step("exit")
        stream.close()
        gui.close()
    finally:
        d.shutdown()


def scenario_replay(log):
    d = Daemon(log, "replay")
    try:
        sid = d.spawn()
        extra = d.spawn()
        em = Emitter(d, sid)
        stream = Stream(d.socket, sid)
        check_replies(log, "query-before-handover", em.capture("q0"), 1)
        drive_reports(log, d, em, stream, observer_head(d))
        fresh = Stream(d.socket, sid)  # history replays the query and the reports
        check_replies(log, "fresh-attach-writes-no-reply", em.capture("fresh", query=False), 0)
        log.check(not [f for f in fresh.states() if not f.get("isSnapshot")],
                  "fresh-attach-no-new-edges", str([describe(f) for f in fresh.states()]))
        fresh.close()
        before = d.sessions()
        old_pid = d.pid
        resp = d.upgrade(CLI)
        log.check(resp.get("type") == "upgradeScheduled", "upgrade-scheduled", json.dumps(resp))
        stream.close()
        # Successor takes the socket over; bounded retry on the real signal (a new pid answering).
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            try:
                d.reconnect()
                if d.pid != old_pid:
                    break
            except (OSError, RuntimeError, EOFError, ValueError):
                pass
            try:
                d.proc.wait(0.5)
            except subprocess.TimeoutExpired:
                pass
        log.check(d.pid != old_pid, "successor-serving", f"old={old_pid} new={d.pid}")
        after = d.sessions()
        log.check(before == after and sid in after and extra in after, "sessions-before-eq-after",
                  f"before={before} after={after}")
        post = Stream(d.socket, sid)
        check_replies(log, "replay-writes-no-reply", em.capture("replay", query=False), 0)
        edges = [f for f in post.states() if not f.get("isSnapshot") and is_protocol(f)]
        log.check(not edges, "replay-zero-protocol-edges", str([describe(f) for f in edges]))
        log.info("post-handover snapshot " + str([describe(f) for f in post.states() if f.get("isSnapshot")]))
        check_replies(log, "successor-answers-live-query", em.capture("q1"), 1)
        start = post.mark()
        em.report("state=working:app=e2e")
        log.check(post.wait(lambda f: f["state"] == "working" and is_protocol(f), start=start) is not None,
                  "successor-ingests-live-report")
        em.step("exit")
        post.close()
    finally:
        d.shutdown()


def scenario_release(log):
    d = Daemon(log, "release")
    try:
        sid = d.spawn()
        em = Emitter(d, sid)
        stream = Stream(d.socket, sid)
        start = stream.mark()
        em.report("state=working:app=e2e")
        log.check(stream.wait(lambda f: f["state"] == "working" and is_protocol(f), start=start) is not None,
                  "working-reported")
        start = stream.mark()
        em.step("exit")  # program exits; the shell owns the foreground again
        left = stream.wait(lambda f: f["state"] != "working", start=start)
        log.check(left is not None, "state-leaves-working-after-exit", describe(left or {}))
        last = stream.states()[-1] if stream.states() else {}
        log.check(last.get("state") != "working", "final-state-not-working", describe(last))
        if left is None or last.get("state") == "working":
            # Every agentState frame since the exit step, to tell a missing release from a late one.
            for frame in stream.states(start):
                log.info("after-exit frame " + describe(frame))
        stream.close()
    finally:
        d.shutdown()


def scenario_ack(log):
    d = Daemon(log, "ack")
    try:
        sid = d.spawn()
        em = Emitter(d, sid)
        stream = Stream(d.socket, sid)
        start = stream.mark()
        em.report("state=done:app=e2e")
        done = stream.wait(lambda f: f.get("outcome") == "done" and is_protocol(f), start=start)
        log.check(done is not None and done["state"] == "idle", "done-is-idle-with-outcome", describe(done or {}))
        start = stream.mark()
        resp = d.acknowledge(sid)
        log.check(resp.get("type") == "acknowledgeAgentOutcomeOk", "ack-ok", json.dumps(resp))
        cleared = stream.wait(lambda f: "outcome" not in f, start=start)
        log.check(cleared is not None and cleared["state"] == "idle", "outcome-cleared", describe(cleared or {}))
        em.step("exit")
        stream.close()
    finally:
        d.shutdown()


def scenario_observer80(log):
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    resource.setrlimit(resource.RLIMIT_NOFILE, (min(hard, 8192), hard))
    d = Daemon(log, "observer80")
    try:
        sids = [d.spawn(cols=100, rows=30) for _ in range(80)]
        log.check(len(set(sids)) == 80, "spawned-80")
        for i, sid in enumerate(sids):
            d.write(sid, f"echo e2e-pane-{i}\r".encode())
        seen = set()
        for sid in sids:
            hit = d.observer_wait(sid, "any", 0)
            if (hit.get("observation") or {}).get("sessionId") == sid or hit.get("gap"):
                seen.add(sid)
        ev = d.events(0)
        listed = {e["sessionId"] for e in ev.get("events", []) + ev.get("snapshot", [])}
        seen |= listed & set(sids)
        log.check(seen == set(sids), "all-80-observed", f"observed={len(seen)}")
        log.check(set(sids) <= set(d.sessions()), "all-80-listed")
        log.info("mirror lock-hold time is recorded by todo 5's unit test, not asserted here")
    finally:
        d.shutdown()


def claude_ready(log):
    exe = shutil.which("claude")
    if not exe:
        return "no-claude-binary"
    out = subprocess.run([exe, "--version"], capture_output=True, text=True, timeout=60).stdout
    m = re.search(r"(\d+)\.(\d+)\.(\d+)", out)
    log.info(f"claude version {m.group(0) if m else out.strip()!r}")
    if not m or tuple(map(int, m.groups())) < (2, 1, 295):
        return "claude-too-old"
    # Only the exit status and a login flag are inspected; the output is never logged or stored.
    st = subprocess.run([exe, "auth", "status"], capture_output=True, text=True, timeout=60)
    text = (st.stdout + st.stderr).lower()
    if st.returncode != 0 or "not logged in" in text or re.search(r'"loggedin"\s*:\s*false', text):
        return "no-claude-login"
    return None


def claude_turn(log, d, sid, settings, run, want):
    """One Claude turn under the user's real HOME (credentials stay where they are)."""
    stream = Stream(d.socket, sid)
    extra = f" --settings {settings}" if settings else ""
    prompt = "Create a file named x.txt in the current directory containing exactly: hi"
    trusted = {"done": False}

    def tick():
        # Workspace trust dialog: accept the default ("Yes, proceed") once.
        if not trusted["done"] and b"trust" in bytes(stream.output[-8192:]).lower():
            trusted["done"] = True
            d.write(sid, b"\r")

    start = stream.mark()
    d.write(sid, f"cd {d.work} && HOME={REAL_HOME} claude{extra} '{prompt}'\r".encode())
    working = stream.wait(lambda f: f["state"] == "working" and want(f), timeout=120, start=start, tick=tick)
    log.check(working is not None, f"{run} working", describe(working or {}))
    blocked = stream.wait(lambda f: f["state"] == "blocked" and want(f), timeout=180, start=start, tick=tick)
    log.check(blocked is not None, f"{run} blocked", describe(blocked or {}))
    if blocked is not None and run == "run1-protocol":
        log.check(blocked.get("kind") == "permission", f"{run} blocked-kind-permission", describe(blocked))
    mark = stream.mark()
    d.write(sid, b"\r")  # answer the permission prompt with its default (allow)
    idle = stream.wait(lambda f: f["state"] == "idle" and want(f), timeout=180, start=mark, tick=tick)
    log.check(idle is not None, f"{run} idle-after-answer", describe(idle or {}))
    if idle is not None and run == "run1-protocol":
        log.check(idle.get("outcome") == "done", f"{run} outcome-done", describe(idle))
    log.check(os.path.exists(os.path.join(d.work, "x.txt")), f"{run} file-created")
    frames = [f for f in stream.states(start) if not f.get("isSnapshot")]
    log.info(f"{run} frames " + json.dumps([describe(f) for f in frames]))
    d.write(sid, b"/exit\r")
    stream.close()
    return frames


def scenario_claude(log):
    skip = claude_ready(log)
    if skip:
        log.verdict(skip=skip)
        return True
    log.info("credentials are never read or copied: claude runs with the real HOME; "
             "CLAUDE_CONFIG_DIR isolation would need the credentials copied, so it is not used")
    # Run 1 (R13): no Ferryx hooks can report - the daemon gives the pane no agent-state endpoint,
    # so any hook in the user's real ~/.claude is a no-op and the session never holds Agent origin.
    d = Daemon(log, "claude-run1", integration=False)
    try:
        sid = d.spawn()
        frames = claude_turn(log, d, sid, None, "run1-protocol", is_protocol)
        log.check(not [f for f in frames if is_agent_origin(f)], "run1 no-agent-origin-frames")
    finally:
        d.shutdown()
    # Run 2: hooks from the daemon's own installer (server.rs:5875 ->
    # agent_hook_install.rs:721) into the isolated HOME, loaded with `claude --settings`.
    d = Daemon(log, "claude-run2", integration=True, claude_dir=True)
    try:
        installed = os.path.join(d.home, ".claude", "settings.json")
        if not log.check(os.path.exists(installed), "run2 installer-wrote-hooks", installed):
            return
        text = open(installed).read()
        log.check(" agent-hook claude " in text, "run2 hook-command-shape")
        # ferryx-cli has no `agent-hook` dispatch (bin/cli.rs); the installed command names the
        # daemon's own exe (agent_extension.rs:303). Point it at the remote helper, which runs the
        # same reporter (ferryx_scope/ssh/process.rs:954).
        text = text.replace(f"'{CLI}'", f"'{HELPER}'")
        settings = os.path.join(d.work, "claude-hooks.json")
        with open(settings, "w") as fh:
            fh.write(text)
        sid = d.spawn()
        frames = claude_turn(log, d, sid, settings, "run2-agent", is_agent_origin)
        agent = [i for i, f in enumerate(frames) if is_agent_origin(f)]
        later_protocol = [f for f in frames[agent[0]:] if is_protocol(f)] if agent else []
        log.check(bool(agent), "run2 agent-origin-present")
        log.check(not later_protocol, "run2 agent-origin-wins",
                  str([describe(f) for f in later_protocol]))
    finally:
        d.shutdown()


SCENARIOS = {
    "emitter": scenario_emitter,
    "replay": scenario_replay,
    "release": scenario_release,
    "ack": scenario_ack,
    "observer80": scenario_observer80,
    "claude": scenario_claude,
}


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else ""
    if name not in SCENARIOS:
        print(f"usage: {sys.argv[0]} {'|'.join(SCENARIOS)}", file=sys.stderr)
        return 2
    os.makedirs(LOG_DIR, exist_ok=True)
    log = Log(name)
    log.info(f"cli={CLI}")
    try:
        if SCENARIOS[name](log):
            return 0
    except Exception as error:  # a crash is a FAIL line, never a silent pass
        log.check(False, "scenario-crashed", repr(error))
    log.verdict()
    return 1 if log.failed else 0


if __name__ == "__main__":
    sys.exit(main())
