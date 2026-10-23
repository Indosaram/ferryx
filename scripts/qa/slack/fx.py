#!/usr/bin/env python3
"""Minimal ferryx daemon client: fx.py '<json request>' -> prints the first reply line."""
import socket, json, sys, os
def call(req, timeout=30):
    s = socket.socket(socket.AF_UNIX); s.settimeout(timeout)
    s.connect(os.environ["FERRYX_RUNTIME_DIR"] + "/daemon.sock")
    f = s.makefile("rwb")
    f.write((json.dumps({"type": "handshake", "version": 5}) + "\n").encode()); f.flush(); f.readline()
    f.write((json.dumps(req) + "\n").encode()); f.flush()
    return f.readline().decode().strip()
if __name__ == "__main__":
    print(call(json.loads(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 30))
