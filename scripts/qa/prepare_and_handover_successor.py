#!/usr/bin/env python3
import hashlib
import json
import os
import socket
import subprocess
import sys
import time

PROTOCOL_VERSION = 5
EXPECTED_PID = 1256760
EXPECTED_MACHINE_ID = "2773ab38-d556-4a81-ae49-18a3b0fb83af"

def get_socket_path():
    uid = os.getuid()
    return f"/tmp/rorca-{uid}/daemon.sock"

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()

def send_request(sock, req_dict):
    payload = json.dumps(req_dict).encode("utf-8")
    sock.sendall(payload + b"\n")
    buffer = b""
    while b"\n" not in buffer:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("Socket closed prematurely while reading response")
        buffer += chunk
    line, _ = buffer.split(b"\n", 1)
    return json.loads(line.decode("utf-8"))

def count_ptmx_fds(pid):
    fd_dir = f"/proc/{pid}/fd"
    if not os.path.exists(fd_dir):
        raise FileNotFoundError(f"/proc/{pid}/fd does not exist")
    ptmx_count = 0
    for entry in os.listdir(fd_dir):
        target = os.readlink(os.path.join(fd_dir, entry))
        if "ptmx" in target or "/dev/pts/" in target:
            ptmx_count += 1
    return ptmx_count

def abort_and_exit(sock, exit_code, error_msg):
    print(f"[HANDOVER_ABORT]: {error_msg}", file=sys.stderr)
    try:
        res = send_request(sock, {"type": "abortHandover"})
        if res.get("type") != "abortHandoverOk":
            print(f"[ABORT_WARNING]: abortHandover returned unexpected response: {res}", file=sys.stderr)
        else:
            print("[ABORT_OK]: Predecessor handover successfully aborted back to active state.")
    except Exception as e:
        print(f"[ABORT_ERROR]: Failed to send abortHandover: {e}", file=sys.stderr)
    sys.exit(exit_code)

def main():
    if len(sys.argv) < 3:
        print("Usage: prepare_and_handover_successor.py <path_to_new_binary> <expected_sha256_prefix> [--dry-run]", file=sys.stderr)
        sys.exit(1)

    new_binary = os.path.abspath(sys.argv[1])
    expected_sha_prefix = sys.argv[2].lower()
    dry_run = "--dry-run" in sys.argv

    if not os.path.isfile(new_binary) or not os.access(new_binary, os.X_OK):
        print(f"[GATE_ERROR]: Successor binary not found or not executable: {new_binary}", file=sys.stderr)
        sys.exit(1)

    actual_sha = compute_sha256(new_binary)
    print(f"[ARTIFACT]: Path={new_binary}, SHA256={actual_sha}")
    if not actual_sha.lower().startswith(expected_sha_prefix):
        print(f"[GATE_ERROR]: Artifact SHA mismatch! Expected prefix {expected_sha_prefix}, got {actual_sha}", file=sys.stderr)
        sys.exit(1)

    identity_path = os.path.expanduser("~/.ferryx/remote/identity.json")
    if not os.path.exists(identity_path):
        print(f"[GATE_ERROR]: Identity file not found at {identity_path}", file=sys.stderr)
        sys.exit(1)

    with open(identity_path, "r") as f:
        identity_data = json.load(f)
        loaded_machine_id = identity_data.get("machineId")
        if loaded_machine_id != EXPECTED_MACHINE_ID:
            print(f"[GATE_ERROR]: Machine ID mismatch! Expected {EXPECTED_MACHINE_ID}, got {loaded_machine_id}", file=sys.stderr)
            sys.exit(1)
        print(f"[GATE_OK]: Machine ID validated: {loaded_machine_id}")

    sock_path = get_socket_path()
    if not os.path.exists(sock_path):
        print(f"[GATE_ERROR]: Daemon socket not found at {sock_path}", file=sys.stderr)
        sys.exit(1)

    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(5.0)
    try:
        s.connect(sock_path)
    except Exception as e:
        print(f"[GATE_ERROR]: Failed to connect to {sock_path}: {e}", file=sys.stderr)
        sys.exit(1)

    handshake_res = send_request(s, {"type": "handshake", "version": PROTOCOL_VERSION})
    if handshake_res.get("type") != "handshakeOk":
        print(f"[GATE_ERROR]: Handshake failed: {handshake_res}", file=sys.stderr)
        sys.exit(1)

    predecessor_pid = handshake_res.get("pid")
    predecessor_epoch = handshake_res.get("epoch")
    print(f"[BASELINE]: Predecessor PID={predecessor_pid}, Epoch={predecessor_epoch}")

    if predecessor_pid != EXPECTED_PID:
        print(f"[GATE_ERROR]: Predecessor PID mismatch! Expected {EXPECTED_PID}, connected to {predecessor_pid}", file=sys.stderr)
        sys.exit(1)

    list_res = send_request(s, {"type": "listSessions"})
    if list_res.get("type") != "listSessionsOk":
        print(f"[GATE_ERROR]: listSessions failed: {list_res}", file=sys.stderr)
        sys.exit(1)

    sessions = list_res.get("sessions", [])
    print(f"[BASELINE]: Active sessions count={len(sessions)}")
    if len(sessions) != 0:
        print(f"[REFUSAL]: Live sessions present ({len(sessions)}). Non-zero session safety policy prevents unverified handover.", file=sys.stderr)
        sys.exit(2)

    try:
        ptmx_count = count_ptmx_fds(predecessor_pid)
    except Exception as e:
        print(f"[GATE_ERROR]: Failed to audit /proc/{predecessor_pid}/fd: {e}", file=sys.stderr)
        sys.exit(3)

    print(f"[BASELINE]: Open ptmx/pts file descriptors={ptmx_count}")
    if ptmx_count != 0:
        print(f"[REFUSAL]: Predecessor holds open ptmx/pts descriptors ({ptmx_count} != 0). Refusing handover.", file=sys.stderr)
        sys.exit(3)

    print("[GATE_OK]: Baseline check passed (0 live sessions, 0 open ptmx descriptors).")

    if dry_run:
        print("[DRY_RUN]: Safety checks verified. Handover request not dispatched.")
        sys.exit(0)

    print("[HANDOVER]: Requesting PrepareHandover on live daemon...")
    prep_res = send_request(s, {"type": "prepareHandover"})
    if prep_res.get("type") != "prepareHandoverOk":
        print(f"[HANDOVER_ERROR]: PrepareHandover rejected: {prep_res}", file=sys.stderr)
        sys.exit(4)

    active_sessions_at_prep = prep_res.get("activeSessions", [])
    if len(active_sessions_at_prep) != 0:
        abort_and_exit(s, 4, f"Race detected! PrepareHandover returned non-empty activeSessions: {active_sessions_at_prep}")

    legacy_socket_path = prep_res.get("legacySocketPath")
    if not legacy_socket_path:
        abort_and_exit(s, 5, f"Missing legacySocketPath in response: {prep_res}")

    print(f"[HANDOVER_OK]: Predecessor prepared. Legacy socket: {legacy_socket_path}")

    successor_unit = f"ferryx-daemon-{int(time.time())}"
    cmd = [
        "systemd-run",
        "--user",
        f"--unit={successor_unit}",
        "--description=Ferryx Successor PTY Daemon",
        new_binary,
        "--daemon",
        "--handover-from",
        legacy_socket_path,
    ]

    print(f"[LAUNCH]: Spawning successor in distinct systemd unit: {successor_unit}")
    print(f"[COMMAND]: {' '.join(cmd)}")
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        abort_and_exit(s, 6, f"systemd-run failed (exit {proc.returncode}):\n{proc.stderr}")

    print(f"[LAUNCH_OK]: Successor spawned in unit {successor_unit}. Predecessor socket will remain draining.")

if __name__ == "__main__":
    main()
