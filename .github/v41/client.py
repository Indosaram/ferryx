import base64, json, socket, sys, time, threading
path, work, out = sys.argv[1], sys.argv[2], sys.argv[3]
T0 = time.time()
def ts(): return "%.2fs" % (time.time() - T0)
def conn():
    s = socket.socket(socket.AF_UNIX); s.settimeout(120); s.connect(path); return s, s.makefile("rwb")
def req(f, obj, want):
    f.write((json.dumps(obj) + "\n").encode()); f.flush()
    for _ in range(50):
        v = json.loads(f.readline())
        if v.get("type") in want: return v
s, f = conn()
print(ts(), "handshake", json.dumps(req(f, {"type": "handshake", "version": 5}, {"handshakeOk", "error"}))[:120])
print(ts(), "register", str(req(f, {"type": "registerWorkspace", "workspaceId": "w41", "repoRoot": work}, {"registerWorkspaceOk", "error"}))[:120])
# probe async-runtime liveness while the spawn is in flight
probes = []
stop = False
def prober():
    while not stop:
        t = time.time()
        try:
            ps, pf = conn(); req(pf, {"type": "handshake", "version": 5}, {"handshakeOk", "error"}); ps.close()
            probes.append(time.time() - t)
        except Exception as e:
            probes.append(("err", str(e)))
        time.sleep(1)
th = threading.Thread(target=prober, daemon=True); th.start()
t = time.time()
r = req(f, {"type": "spawn", "clientRequestId": "r41", "workspaceId": "w41", "worktree": None, "cwd": work, "cols": 120, "rows": 40}, {"spawnOk", "error"})
spawn_s = time.time() - t
stop = True
print(ts(), "SPAWN_LATENCY=%.2fs" % spawn_s, json.dumps(r)[:200])
nums = [p for p in probes if isinstance(p, float)]
print("HANDSHAKE_PROBES_DURING_SPAWN n=%d max=%.3fs errors=%d" % (len(nums), max(nums) if nums else -1, len(probes) - len(nums)))
sid = r["sessionId"]
a, af = conn(); req(af, {"type": "handshake", "version": 5}, {"handshakeOk", "error"})
af.write((json.dumps({"type": "attach", "sessionId": sid, "afterSequence": None}) + "\n").encode()); af.flush()
cmd = "locale > %s 2>&1; echo V41""DONE\n" % out
req(f, {"type": "write", "sessionId": sid, "data": base64.b64encode(cmd.encode()).decode()}, {"writeOk", "error"})
buf = b""; a.settimeout(2); deadline = time.time() + 30
while time.time() < deadline and b"V41DONE" not in buf:
    try: line = af.readline()
    except socket.timeout: continue
    if not line: break
    v = json.loads(line)
    for k in ("data", "history"):
        if isinstance(v.get(k), str):
            try: buf += base64.b64decode(v[k])
            except Exception: pass
txt = buf.decode("utf-8", "replace")
print("=== PTY transcript (tail) ===\n" + txt[-600:] + "\n=== end ===")
print("SETLOCALE_WARNING", "yes" if ("setlocale" in txt or "cannot change locale" in txt) else "no")
