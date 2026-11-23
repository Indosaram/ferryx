#!/usr/bin/env python3
"""Box side of the model-egress bridge.

Some providers give a sandbox free outbound internet to the package registries and not to the
model endpoint (task 1 measured `registry.npmjs.org` -> 200, `https://100milionseek.top/` -> 000
from a fresh Daytona box). This script is the box half of the fix: it serves the endpoint's API
on plain HTTP at 127.0.0.1:<port>, and frames every request over stdin/stdout to the Mac, which
makes the real TLS call and streams the reply back.

Nothing here holds state or a credential: headers and body go up as they arrived, and the
response is copied back chunk by chunk so server-sent events reach the engine as they are
produced. The bridge is only ever started by `ferryx-cli cloud socket <name> --hold` on the Mac
and terminates with that ssh channel.

Frames (both directions), 4-byte big-endian length then a UTF-8 JSON object:

  -> {"t":"req","id":N,"method":"POST","path":"/v1/chat/completions","headers":{...},"body":"<b64>"}
  <- {"t":"res","id":N,"status":200,"headers":{...}}
  <- {"t":"chunk","id":N,"body":"<b64>"}      zero or more
  <- {"t":"end","id":N}                        or {"t":"err","id":N,"error":"..."}
"""

import base64
import itertools
import json
import queue
import socketserver
import struct
import sys
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler

ARGV = sys.argv[1:]
# `--probe <port>` is the readiness check `cloud socket` runs from inside the box before it
# publishes a socket: it asks the bridge for one real request and reports what came back.
PROBE = "--probe" in ARGV
PORT = int(ARGV[-1]) if ARGV and ARGV[-1].isdigit() else 18802
LEN = struct.Struct("!I")
# Frames the Mac produced for one request, keyed by frame id.
PENDING = {}
PENDING_LOCK = threading.Lock()
NEXT_ID = itertools.count(1)
CHANNEL_CLOSED = False
OUT = sys.stdout.buffer
OUT_LOCK = threading.Lock()
# Hop-by-hop headers that must not be copied: the reply is re-framed with chunked encoding.
HOP = ("transfer-encoding", "connection", "content-encoding", "content-length")


def send(obj):
    payload = json.dumps(obj).encode()
    with OUT_LOCK:
        OUT.write(LEN.pack(len(payload)) + payload)
        OUT.flush()


def read_exact(stream, size):
    buf = b""
    while len(buf) < size:
        chunk = stream.read(size - len(buf))
        if not chunk:
            return buf
        buf += chunk
    return buf


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):
        # stderr is the ssh channel's diagnostic side; the Mac holder forwards it to its own log.
        sys.stderr.write("model-bridge: " + format % args + "\n")
        sys.stderr.flush()
    def _handle(self):
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length) if length else b""
        with PENDING_LOCK:
            if CHANNEL_CLOSED:
                self.send_error(502, "upstream bridge channel is closed")
                return
            rid = next(NEXT_ID)
            replies = queue.Queue()
            PENDING[rid] = replies
        try:
            send(
                {
                    "t": "req",
                    "id": rid,
                    "method": self.command,
                    "path": self.path,
                    "headers": dict(self.headers.items()),
                    "body": base64.b64encode(body).decode(),
                }
            )
            try:
                first = replies.get(timeout=600)
            except queue.Empty:
                self.send_error(504, "the Mac did not answer the bridge request")
                return
            if first.get("t") == "err":
                message = json.dumps({"error": first.get("error")}).encode()
                self.send_response(502)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(message)))
                self.end_headers()
                self.wfile.write(message)
                return
            self.send_response(int(first.get("status") or 502))
            for key, value in (first.get("headers") or {}).items():
                if key.lower() in HOP:
                    continue
                self.send_header(key, value)
            self.send_header("transfer-encoding", "chunked")
            self.send_header("connection", "close")
            self.close_connection = True
            self.end_headers()
            self.wfile.flush()
            while True:
                try:
                    item = replies.get(timeout=900)
                except queue.Empty:
                    break
                kind = item.get("t")
                if kind in ("end", "err"):
                    break
                data = base64.b64decode(item.get("body") or "")
                if data:
                    self.wfile.write(b"%x\r\n" % len(data) + data + b"\r\n")
                    self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        except Exception as exc:  # noqa: BLE001 - a broken request must not kill the server
            sys.stderr.write("model-bridge error: %s\n" % exc)
            sys.stderr.flush()
        finally:
            with PENDING_LOCK:
                PENDING.pop(rid, None)

    do_GET = _handle
    do_POST = _handle
    do_PUT = _handle
    do_DELETE = _handle


def reader(server):
    """Reads the Mac's frames and hands each to the request waiting for it."""
    global CHANNEL_CLOSED
    stream = sys.stdin.buffer
    try:
        while True:
            head = read_exact(stream, LEN.size)
            if len(head) < LEN.size:
                break
            (size,) = LEN.unpack(head)
            raw = read_exact(stream, size)
            if len(raw) < size:
                break
            frame = json.loads(raw)
            with PENDING_LOCK:
                replies = PENDING.get(frame.get("id"))
            if replies is not None:
                replies.put(frame)
    finally:
        # The Mac's end of the channel closed: fail every waiting request immediately and stop
        # the local listener so the port is free for the next `cloud socket` session.
        with PENDING_LOCK:
            CHANNEL_CLOSED = True
            waiting = list(PENDING.values())
        for replies in waiting:
            replies.put({"t": "err", "error": "upstream bridge channel closed"})
        sys.stderr.write("model-bridge: upstream channel closed\n")
        sys.stderr.flush()
        server.shutdown()


def probe():
    """Ask the bridge one request and report whether a real upstream status came back.

    Any HTTP status proves the route: a 401 means the Mac reached the endpoint and the endpoint
    answered, which is exactly the property a dispatch needs. Only a transport failure (no
    listener, or the Mac's channel closed) is a failure.
    """
    url = "http://127.0.0.1:%d/v1/models" % PORT
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            sys.stderr.write("model-bridge: probe answered %d\n" % response.status)
            return 0
    except urllib.error.HTTPError as exc:
        sys.stderr.write("model-bridge: probe answered %d\n" % exc.code)
        return 0
    except Exception as exc:  # noqa: BLE001 - any transport failure is the answer we report
        sys.stderr.write("model-bridge: probe failed: %s\n" % exc)
        return 1


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def main():
    if PROBE:
        raise SystemExit(probe())
    server = Server(("127.0.0.1", PORT), Handler)
    threading.Thread(target=reader, args=(server,), daemon=True).start()
    sys.stderr.write("model-bridge: listening on 127.0.0.1:%d\n" % PORT)
    sys.stderr.flush()
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
