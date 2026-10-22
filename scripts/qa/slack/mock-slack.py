#!/usr/bin/env python3
"""Loopback stand-in for the two Slack endpoints the hub uses, for a local round trip only.
POST /api/apps.connections.open -> ws url; WS /ws speaks Socket Mode (hello, envelopes, acks);
POST /api/chat.postMessage is recorded; POST /inject {payload, type} pushes one envelope."""
import asyncio, json, sys, uuid
from aiohttp import web
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 9801
LOG = open(sys.argv[2] if len(sys.argv) > 2 else "/tmp/mock-slack.jsonl", "a", buffering=1)
sockets, acks = set(), []
def log(kind, **kw): LOG.write(json.dumps({"kind": kind, **kw}) + "\n")
async def open_conn(req):
    log("connections.open", auth=req.headers.get("Authorization", "")[:12])
    return web.json_response({"ok": True, "url": f"ws://127.0.0.1:{PORT}/ws"})
async def post_message(req):
    body = await req.json(); log("chat.postMessage", auth=req.headers.get("Authorization", "")[:12], body=body)
    return web.json_response({"ok": True, "channel": body.get("channel"), "ts": "1.0"})
async def ws(req):
    w = web.WebSocketResponse(); await w.prepare(req); sockets.add(w)
    await w.send_str(json.dumps({"type": "hello", "num_connections": 1}))
    log("ws.connected")
    async for msg in w:
        if msg.type == web.WSMsgType.TEXT: acks.append(msg.data); log("ws.ack", data=json.loads(msg.data))
    sockets.discard(w); log("ws.closed"); return w
async def inject(req):
    body = await req.json(); env = {"envelope_id": str(uuid.uuid4()), "type": body["type"], "payload": body["payload"], "accepts_response_payload": False}
    if body["type"] == "disconnect": env = {"type": "disconnect", "reason": "refresh_requested"}
    for w in list(sockets): await w.send_str(json.dumps(env))
    log("injected", envelope=env.get("envelope_id"), type=body["type"], sockets=len(sockets))
    return web.json_response({"ok": True, "sockets": len(sockets), "envelope_id": env.get("envelope_id")})
app = web.Application()
app.add_routes([web.post("/api/apps.connections.open", open_conn), web.post("/api/chat.postMessage", post_message), web.get("/ws", ws), web.post("/inject", inject)])
web.run_app(app, host="127.0.0.1", port=PORT, print=None)
