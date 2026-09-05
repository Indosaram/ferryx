"""Isolated installed-provider probe; never reads personal provider configuration."""
import json, os, pathlib, subprocess, tempfile, selectors, time, shutil

root = pathlib.Path(tempfile.mkdtemp(prefix="ferryx-history-"))
env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin:/Users/indo/.local/bin", "HOME": str(root), "CODEX_HOME": str(root / "codex"), "CLAUDE_CONFIG_DIR": str(root / "claude"), "TERM": "xterm-256color"}
for name in ("codex", "claude", "repo"):
    (root / name).mkdir()
receipt = {"isolated": True, "versions": {}, "events": []}
try:
    for provider in ("codex", "claude"):
        result = subprocess.run(["/Users/indo/.local/bin/" + provider, "--version"], env=env, capture_output=True, text=True, timeout=10)
        receipt["versions"][provider] = result.stdout.strip()
    p = subprocess.Popen(["/Users/indo/.local/bin/codex", "app-server", "--listen", "stdio://"], cwd=root / "repo", env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    sel = selectors.DefaultSelector(); sel.register(p.stdout, selectors.EVENT_READ)
    def send(value):
        p.stdin.write((json.dumps(value) + "\n").encode()); p.stdin.flush()
    def response(request_id):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if not sel.select(max(0, deadline - time.monotonic())): raise TimeoutError("provider response deadline")
            line = p.stdout.readline()
            if not line: raise RuntimeError("provider exited")
            data = json.loads(line)
            if data.get("id") == request_id: return data
        raise TimeoutError("provider response deadline")
    try:
        send({"id":1,"method":"initialize","params":{"clientInfo":{"name":"ferryx-history-qa","version":"1"}}}); response(1)
        send({"method":"initialized"})
        send({"id":2,"method":"thread/start","params":{"cwd":str(root / "repo"),"approvalPolicy":"never","sandbox":"read-only","persistExtendedHistory":True}})
        data = response(2)
        thread = data.get("result", {}).get("thread", {}).get("id")
        receipt["threadCreated"] = bool(thread)
        if thread:
            send({"id":3,"method":"turn/start","params":{"threadId":thread,"input":[{"type":"text","text":"ferryx-history-isolated-sentinel"}]}})
            turn = response(3); receipt["turnAccepted"] = "result" in turn
    finally:
        p.terminate(); p.wait(timeout=10); sel.close()
    # Only provider-created records in this script's own empty HOME are inspected.
    records = []
    for path in list((root / "codex").rglob("*.jsonl"))[:4]:
        for line in path.read_text()[:65536].splitlines():
            value = json.loads(line)
            if value.get("type") in ("session_meta", "response_item"):
                records.append(value)
    receipt["providerRecordTypes"] = [r.get("type") for r in records]
    if thread:
        import pty
        master, slave = pty.openpty()
        native = subprocess.Popen(["/Users/indo/.local/bin/codex", "resume", thread], cwd=root / "repo", env=env, stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        reader = selectors.DefaultSelector(); reader.register(master, selectors.EVENT_READ)
        try:
            ready = reader.select(10)
            output = os.read(master, 65536) if ready else b""
            receipt["nativeCodex"] = {"exactProviderIdArg": True, "outputBytes": len(output), "restorationVerified": False}
        finally:
            native.terminate(); native.wait(timeout=10); reader.close(); os.close(master)
    result = subprocess.run(["/Users/indo/.local/bin/claude", "-p", "ferryx-history-isolated-sentinel", "--output-format", "json"], cwd=root / "repo", env=env, capture_output=True, text=True, timeout=20)
    receipt["claude"] = {"exit": result.returncode, "authBlocked": "login" in (result.stdout + result.stderr).lower() or "log in" in (result.stdout + result.stderr).lower()}
    # Sanitize generated records before storing; no personal transcript was opened.
    text = json.dumps(records, indent=2).replace(str(root / "repo"), "/fixture/project").replace(str(root), "/fixture")
    print("PROVIDER_RECORDS=" + text)
finally:
    shutil.rmtree(root)
    receipt["cleanup"] = not root.exists()
    print("RECEIPT=" + json.dumps(receipt))
