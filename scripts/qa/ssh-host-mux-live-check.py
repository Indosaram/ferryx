#!/usr/bin/env python3
"""Live check for SSH host multiplexing against a running Ferryx daemon.

Opens N remote SSH panes on one host through the daemon socket, counts the local `ssh`
processes that reach the host, kills the shared transport, and checks every pane comes back
with its earlier output replayed.

usage: ssh-host-mux-live-check.py <workspace-id> <ssh_hosts.json> <host-address> [panes]
"""
import base64
import json
import os
import re
import socket
import subprocess
import sys
import time
import uuid

SOCK = "/tmp/ferryx-%d/daemon.sock" % os.getuid()


def ask(request, timeout=120):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(timeout)
    s.connect(SOCK)
    f = s.makefile("rwb")
    f.write((json.dumps({"type": "handshake", "version": 6}) + "\n").encode())
    f.flush()
    f.readline()
    f.write((json.dumps(request) + "\n").encode())
    f.flush()
    line = f.readline()
    s.close()
    return json.loads(line)


def screen(session_id):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(6)
    s.connect(SOCK)
    f = s.makefile("rwb")
    f.write((json.dumps({"type": "handshake", "version": 6}) + "\n").encode())
    f.flush()
    f.readline()
    f.write((json.dumps({"type": "attach", "sessionId": session_id, "afterSequence": 0}) + "\n").encode())
    f.flush()
    out = b""
    try:
        for _ in range(50):
            message = json.loads(f.readline())
            if message.get("type") == "attachOk":
                out += base64.b64decode(message.get("history") or "")
                break
    except Exception:
        pass
    s.close()
    return re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\r", b"", out).decode(errors="replace")


def ssh_processes(address):
    ps = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return [line for line in ps.splitlines() if re.search(r"(^|\s|/)ssh\s", line) and address in line]


def running(session_id):
    reply = ask({"type": "describeSession", "sessionId": session_id})
    return bool((reply.get("session") or {}).get("running"))


def main():
    workspace, host_store, address = sys.argv[1:4]
    panes = int(sys.argv[4]) if len(sys.argv) > 4 else 20
    before = len(ssh_processes(address))
    sessions = []
    for index in range(panes):
        reply = ask({
            "type": "spawn", "clientRequestId": str(uuid.uuid4()), "workspaceId": workspace,
            "worktree": None, "cwd": None, "cols": 120, "rows": 36,
            "startup": {"kind": "remoteSsh", "hostStorePath": host_store},
        })
        if reply.get("type") != "spawnOk":
            print("SPAWN_FAILED", index, json.dumps(reply)[:200])
            return 1
        sessions.append(reply["sessionId"])
    for index, session_id in enumerate(sessions):
        marker = "MUXCHECK_%d_%s" % (index, uuid.uuid4().hex[:8])
        ask({"type": "write", "sessionId": session_id,
             "data": base64.b64encode(("echo %s\r" % marker).encode()).decode()})
        sessions[index] = (session_id, marker)
    time.sleep(3)
    transports = ssh_processes(address)
    added = len(transports) - before
    print("PANES", panes, "SSH_PROCESSES_ADDED", added)
    for line in transports:
        print("  ", line[:160])

    pids = [int(line.split()[0]) for line in transports if "bridge --mux" in line]
    print("MUX_TRANSPORTS", len(pids))
    for pid in pids:
        os.kill(pid, 15)
    deadline = time.time() + 90
    while time.time() < deadline:
        if all(running(session_id) for session_id, _ in sessions):
            break
        time.sleep(2)
    time.sleep(5)
    recovered = sum(1 for session_id, _ in sessions if running(session_id))
    replayed = sum(1 for session_id, marker in sessions if marker in screen(session_id))
    after = [line for line in ssh_processes(address) if "bridge --mux" in line]
    print("RECOVERED", recovered, "/", panes, "REPLAYED", replayed, "/", panes,
          "MUX_TRANSPORTS_AFTER", len(after))
    for session_id, _ in sessions:
        ask({"type": "close", "sessionId": session_id})
    ok = added <= 2 and len(pids) == 1 and recovered == panes and replayed == panes and len(after) == 1
    print("RESULT", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
