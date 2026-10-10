#!/usr/bin/env python3
import json
import os
import socket
import sys

PROTOCOL_VERSION = 5

def get_socket_path():
    uid = os.getuid()
    return f"/tmp/rorca-{uid}/daemon.sock"

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

def probe_daemon():
    sock_path = get_socket_path()
    if not os.path.exists(sock_path):
        print(f"[PROBE_ERROR]: Daemon socket not found at {sock_path}", file=sys.stderr)
        sys.exit(1)

    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(5.0)
    try:
        s.connect(sock_path)
    except Exception as e:
        print(f"[PROBE_ERROR]: Failed to connect to {sock_path}: {e}", file=sys.stderr)
        sys.exit(1)

    handshake_req = {
        "type": "handshake",
        "version": PROTOCOL_VERSION,
    }
    handshake_res = send_request(s, handshake_req)
    if handshake_res.get("type") != "handshakeOk":
        print(f"[PROBE_ERROR]: Handshake failed: {handshake_res}", file=sys.stderr)
        sys.exit(1)

    daemon_version = handshake_res.get("version")
    daemon_epoch = handshake_res.get("epoch")
    daemon_pid = handshake_res.get("pid")

    print(f"DAEMON_PID: {daemon_pid}")
    print(f"DAEMON_PROTOCOL_VERSION: {daemon_version}")
    print(f"DAEMON_EPOCH: {daemon_epoch}")

    list_res = send_request(s, {"type": "listSessions"})
    if list_res.get("type") != "listSessionsOk":
        print(f"[PROBE_ERROR]: Unexpected response to listSessions: {list_res}", file=sys.stderr)
        sys.exit(1)

    sessions = list_res.get("sessions", [])
    session_ids = [str(s) for s in sessions if s]
    print(f"TOTAL_SESSIONS_COUNT: {len(session_ids)}")
    print(f"ACTIVE_SESSION_IDS: {','.join(session_ids)}")

if __name__ == "__main__":
    probe_daemon()
