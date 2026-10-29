"""omo session snapshot, piped to `python3 - snapshot <text_limit>` over ssh by the Ferryx hub.

The remote half of `omo_probe.rs`: the same endpoints and the same session view, so a remote
host reports exactly what the hub reads on its own machine. One JSON value on stdout:
{"sessions": [...], "errors": [...]}. Exit 5 on a failure before the snapshot.
"""
import concurrent.futures
import glob
import json
import os
import socket
import sys
import time

RPC_DIR = os.path.expanduser("~/.omo/agent/rpc")
DEAD = (ConnectionRefusedError, FileNotFoundError)


def rpc(path, request, timeout=4.0, secret=True):
    deadline = time.monotonic() + timeout
    conn = socket.socket(socket.AF_UNIX)
    conn.settimeout(timeout)
    conn.connect(path)
    try:
        if secret:
            with open(path + ".secret", "rb") as handle:
                conn.sendall(handle.read())
        request = dict(request)
        request["id"] = "ferryx-%d" % time.time_ns()
        conn.sendall((json.dumps(request) + "\n").encode())
        buffer = b""
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("%s timed out" % request.get("type"))
            conn.settimeout(remaining)
            chunk = conn.recv(1 << 20)
            if not chunk:
                raise RuntimeError("connection closed before the response")
            buffer += chunk
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                if not line.strip():
                    continue
                message = json.loads(line)
                if message.get("id") == request["id"]:
                    return message
    finally:
        conn.close()


def data_of(response):
    if response.get("success") is False:
        raise PermissionError(str(response.get("error") or "refused"))
    return response.get("data") or {}


def text_of(message):
    content = message.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return " ".join(
            block.get("text", "")
            for block in content
            if isinstance(block, dict) and block.get("type") == "text"
        )
    return ""


def pending_questions(messages):
    asked = {}
    answered = set()
    for message in messages:
        role = message.get("role")
        if role == "assistant" and isinstance(message.get("content"), list):
            for block in message["content"]:
                if (
                    isinstance(block, dict)
                    and block.get("type") == "toolCall"
                    and block.get("name") == "ask_user_question"
                    and isinstance(block.get("id"), str)
                ):
                    asked[block["id"]] = block.get("arguments") or {}
        elif role == "toolResult" and message.get("toolName") == "ask_user_question":
            details = message.get("details") or {}
            if details.get("status") != "pending":
                answered.add(message.get("toolCallId"))
        elif role == "user":
            text = text_of(message)
            if text.startswith("[Answer to question "):
                answered.add(text[len("[Answer to question "):].split("]", 1)[0])
    out = []
    for request_id, arguments in asked.items():
        if request_id in answered:
            continue
        items = []
        for index, question in enumerate(arguments.get("questions") or []):
            if not isinstance(question, dict):
                continue
            items.append(
                {
                    "id": question.get("id") or "q%d" % (index + 1),
                    "header": question.get("header"),
                    "question": question.get("question"),
                    "options": [
                        option.get("label")
                        for option in question.get("options") or []
                        if isinstance(option, dict) and isinstance(option.get("label"), str)
                    ],
                }
            )
        out.append({"request_id": request_id, "questions": items})
    return out


def updated_at(session_file):
    try:
        mtime = os.path.getmtime(session_file)
    except (OSError, TypeError):
        return None
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(mtime))


def listed_ids(host):
    try:
        listed = data_of(rpc(host, {"type": "list_sessions"}, secret=False))
    except Exception:
        return []
    return [s.get("sessionId") for s in listed.get("sessions") or [] if s.get("sessionId")]


def endpoints():
    found = []
    for path in sorted(glob.glob(os.path.join(RPC_DIR, "tui", "*.sock"))):
        found.append((path, "tui", True, [None]))
    hosts = [os.path.join(RPC_DIR, "rpc.sock")]
    hosts += sorted(glob.glob(os.path.join(RPC_DIR, "p-*.sock")))
    hosts = [host for host in hosts if os.path.exists(host)]
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        for host, ids in zip(hosts, pool.map(listed_ids, hosts)):
            if ids:
                found.append((host, "rpc_host", False, ids))
    return found


def session_view(path, kind, secret, routing, limit):
    extra = {} if routing is None else {"sessionId": routing}
    state = data_of(rpc(path, dict({"type": "get_state"}, **extra), secret=secret))
    session_id = state.get("sessionId") or routing
    address = routing or session_id
    try:
        messages = data_of(
            rpc(path, {"type": "get_messages", "sessionId": address}, 8.0, secret=secret)
        ).get("messages") or []
        messages_error = None
    except DEAD:
        raise
    except Exception as error:
        messages, messages_error = [], str(error)[:120]
    last_user = next((text_of(m) for m in reversed(messages) if m.get("role") == "user" and text_of(m).strip()), "")
    last_assistant = next((text_of(m) for m in reversed(messages) if m.get("role") == "assistant" and text_of(m).strip()), "")
    durable = session_id
    if kind == "rpc_host":
        session_file = state.get("sessionFile") or ""
        stem = os.path.basename(session_file).rsplit(".", 1)[0]
        durable = stem.split("_", 1)[1] if "_" in stem else session_id
    return {
        "session_id": durable,
        "routing_id": address,
        "endpoint_kind": kind,
        "name": state.get("sessionName"),
        "cwd": state.get("cwd"),
        "streaming": bool(state.get("isStreaming")),
        "compacting": bool(state.get("isCompacting") or state.get("compacting")),
        "blocking_question": bool(state.get("blocking_question")),
        "pending_messages": int(state.get("pendingMessageCount") or 0),
        "message_count": int(state.get("messageCount") or len(messages)),
        "last_user": last_user[-limit:],
        "last_assistant": last_assistant[-limit:],
        "questions": pending_questions(messages),
        "messages_error": messages_error,
        "updated_at": updated_at(state.get("sessionFile")),
    }


def snapshot(limit):
    jobs = [(path, kind, secret, routing) for path, kind, secret, ids in endpoints() for routing in ids]
    sessions, errors, seen = [], [], set()
    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        futures = [(job[0], pool.submit(session_view, *job, limit)) for job in jobs]
        for path, future in futures:
            try:
                view = future.result()
            except DEAD:
                continue
            except Exception as error:
                errors.append({"endpoint": path, "error": str(error)[:200]})
                continue
            if view["session_id"] not in seen:
                seen.add(view["session_id"])
                sessions.append(view)
    return {"sessions": sessions, "errors": errors}


def main(argv):
    if argv[:1] != ["snapshot"]:
        sys.stdout.write(json.dumps({"ok": False, "error": "usage: snapshot <text_limit>"}) + "\n")
        sys.exit(5)
    limit = int(argv[1]) if len(argv) > 1 else 1000
    sys.stdout.write(json.dumps(snapshot(limit)) + "\n")


try:
    main(sys.argv[1:])
except SystemExit:
    raise
except Exception as error:
    sys.stdout.write(json.dumps({"ok": False, "error": str(error)[:300]}) + "\n")
    sys.exit(5)
