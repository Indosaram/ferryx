#!/usr/bin/env python3
"""Create, exercise, and explicitly clean up a private SYSTEM-token sshd."""

from __future__ import annotations

import argparse
import ctypes
import datetime as dt
import json
import os
from pathlib import Path
import queue
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
import traceback
import uuid
from typing import Any
from ctypes import wintypes


def timestamp() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def record(root: Path, **fields: object) -> None:
    lock_path = root / "events.lock"
    with lock_path.open("a+b") as lock_file:
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(lock_file.fileno(), msvcrt.LK_LOCK, 1)
        try:
            with (root / "events.jsonl").open("a", encoding="utf-8") as stream:
                stream.write(json.dumps({"timestamp": timestamp(), **fields}, sort_keys=True) + "\n")
                stream.flush()
        finally:
            if os.name == "nt":
                msvcrt.locking(lock_file.fileno(), msvcrt.LK_UNLCK, 1)


def ps_json(command: str) -> list[dict]:
    result = subprocess.run(["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command],
                            check=True, capture_output=True, text=True, timeout=10)
    if not result.stdout.strip():
        return []
    value = json.loads(result.stdout)
    return value if isinstance(value, list) else [value]


def snapshot() -> list[dict]:
    return ps_json("Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CreationDate | ConvertTo-Json -Compress")


def descendants(root_pid: int, processes: list[dict]) -> list[int]:
    children: dict[int, list[int]] = {}
    for process in processes:
        children.setdefault(int(process["ParentProcessId"]), []).append(int(process["ProcessId"]))
    found: list[int] = []
    pending = [root_pid]
    while pending:
        for child in children.get(pending.pop(), []):
            if child not in found:
                found.append(child)
                pending.append(child)
    return found


def wait_for_file_event(directory: Path, filename: str, timeout: float) -> None:
    log_path = directory / filename
    if log_path.is_file() and "server listening" in log_path.read_text(encoding="utf-8", errors="replace").lower():
        return
    completed = threading.Event()
    failure: list[BaseException] = []
    handles: list[int] = []
    kernel_box: list[Any] = []

    def watch() -> None:
        kernel = getattr(ctypes, "WinDLL")("kernel32", use_last_error=True)
        kernel.CreateFileW.restype = wintypes.HANDLE
        kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                      wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
        kernel.CreateEventW.restype = wintypes.HANDLE
        kernel.CreateEventW.argtypes = [wintypes.LPVOID, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR]
        directory_handle = None
        event_handle = None
        try:
            directory_handle = kernel.CreateFileW(str(directory), 0x0001, 0x00000007, None, 3,
                                                   0x02000000 | 0x40000000, None)
            if directory_handle == wintypes.HANDLE(-1).value:
                raise OSError("CreateFileW failed for readiness directory")
            event_handle = kernel.CreateEventW(None, True, False, None)
            if not event_handle:
                raise OSError("CreateEventW failed for readiness directory event")
            handles.extend([directory_handle, event_handle])
            kernel_box.append(kernel)
            class Overlapped(ctypes.Structure):
                _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t),
                            ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD),
                            ("hEvent", wintypes.HANDLE)]
            overlapped = Overlapped()
            overlapped.hEvent = event_handle
            buffer = ctypes.create_string_buffer(4096)
            returned = wintypes.DWORD()
            kernel.ReadDirectoryChangesW.restype = wintypes.BOOL
            kernel.ReadDirectoryChangesW.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                                                       wintypes.BOOL, wintypes.DWORD,
                                                       ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID,
                                                       wintypes.LPVOID]
            kernel.WaitForSingleObject.restype = wintypes.DWORD
            kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            kernel.GetOverlappedResult.restype = wintypes.BOOL
            kernel.GetOverlappedResult.argtypes = [wintypes.HANDLE, wintypes.LPVOID,
                                                    ctypes.POINTER(wintypes.DWORD), wintypes.BOOL]
            kernel.CancelIoEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
            kernel.ResetEvent.argtypes = [wintypes.HANDLE]
            kernel.SetEvent.argtypes = [wintypes.HANDLE]
            kernel.CloseHandle.argtypes = [wintypes.HANDLE]
            started = kernel.ReadDirectoryChangesW(directory_handle, buffer, len(buffer), False,
                                                    0x00000001 | 0x00000008, ctypes.byref(returned),
                                                    ctypes.byref(overlapped), None)
            if not started:
                raise OSError("ReadDirectoryChangesW failed to arm readiness event")
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if log_path.is_file() and "server listening" in log_path.read_text(encoding="utf-8", errors="replace").lower():
                    completed.set()
                    return
                kernel.ResetEvent(event_handle)
                started = kernel.ReadDirectoryChangesW(directory_handle, buffer, len(buffer), False,
                                                        0x00000001 | 0x00000008, ctypes.byref(returned),
                                                        ctypes.byref(overlapped), None)
                if not started:
                    raise OSError("ReadDirectoryChangesW failed to arm readiness event")
                wait_ms = max(1, int((deadline - time.monotonic()) * 1000))
                wait = kernel.WaitForSingleObject(event_handle, wait_ms)
                if wait != 0:
                    kernel.CancelIoEx(directory_handle, ctypes.byref(overlapped))
                    kernel.GetOverlappedResult(directory_handle, ctypes.byref(overlapped), ctypes.byref(returned), True)
                    raise TimeoutError("WaitForSingleObject readiness deadline expired")
                ok = kernel.GetOverlappedResult(directory_handle, ctypes.byref(overlapped), ctypes.byref(returned), False)
                if not ok:
                    raise OSError("GetOverlappedResult failed for readiness event")
            raise TimeoutError("sshd did not log listener-ready before deadline")
        except BaseException as error:
            failure.append(error)
            completed.set()
        finally:
            if event_handle:
                kernel.CloseHandle(event_handle)
            if directory_handle and directory_handle != wintypes.HANDLE(-1).value:
                kernel.CloseHandle(directory_handle)

    watcher = threading.Thread(target=watch, name="readiness-directory-event", daemon=True)
    watcher.start()
    if not completed.wait(timeout):
        if len(handles) == 2:
            kernel_box[0].CancelIoEx(handles[0], None)
            kernel_box[0].SetEvent(handles[1])
        watcher.join(timeout=2)
        if watcher.is_alive():
            raise RuntimeError("readiness watcher did not exit after cancellation")
        raise TimeoutError(f"timed out waiting for {filename} directory event")
    if failure:
        raise RuntimeError(f"Windows directory event watcher failed: {failure[0]}")


def open_owned_processes(owned: list[dict]) -> list[tuple[int, int]]:
    kernel = getattr(ctypes, "WinDLL")("kernel32", use_last_error=True)
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.GetProcessId.restype = wintypes.DWORD
    kernel.GetProcessId.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handles: list[tuple[int, int]] = []
    for item in owned:
        handle = kernel.OpenProcess(0x0001 | 0x00100000, False, int(item["pid"]))
        if not handle:
            for _, acquired in handles:
                kernel.CloseHandle(acquired)
            raise OSError(f"OpenProcess failed for owned PID {item['pid']}")
        if int(kernel.GetProcessId(handle)) != int(item["pid"]):
            kernel.CloseHandle(handle)
            raise RuntimeError(f"opened handle PID mismatch for {item['pid']}")
        handles.append((int(item["pid"]), handle))
    return handles


def terminate_owned_processes(owned: list[dict], handles: list[tuple[int, int]], timeout: float) -> list[int]:
    kernel = getattr(ctypes, "WinDLL")("kernel32", use_last_error=True)
    kernel.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    try:
        parent = {int(item["pid"]): int(item["parent_pid"]) for item in owned}
        def depth(pid: int) -> int:
            value = 0
            current = pid
            visited = set()
            while current in parent and parent[current] in parent and current not in visited:
                visited.add(current)
                value += 1
                current = parent[current]
            return value
        for pid, handle in sorted(handles, key=lambda pair: depth(pair[0]), reverse=True):
            if not kernel.TerminateProcess(handle, 1):
                result = kernel.WaitForSingleObject(handle, 0)
                if result != 0:
                    raise OSError(f"TerminateProcess failed for owned PID {pid}")
        remaining: list[int] = []
        for pid, handle in handles:
            result = kernel.WaitForSingleObject(handle, int(timeout * 1000))
            if result != 0:
                remaining.append(pid)
        if remaining:
            raise TimeoutError(f"owned process handles did not signal exit: {remaining}")
        return [pid for pid, _ in handles]
    finally:
        for _, handle in handles:
            kernel.CloseHandle(handle)


def find_binary(path: str | None, name: str) -> Path:
    result = Path(path) if path else Path(os.environ.get("WINDIR", r"C:\Windows")) / "System32" / "OpenSSH" / f"{name}.exe"
    result = result.resolve()
    if not result.is_file():
        raise FileNotFoundError(f"existing inbox binary not found: {result}")
    return result


def prepare(root: Path, sshd: Path, ssh: Path, keygen: Path, port: int) -> dict:
    root.mkdir(parents=True, exist_ok=False)
    acl = subprocess.run(["icacls.exe", str(root), "/inheritance:r", "/grant:r",
                          "SYSTEM:(OI)(CI)F", f"{os.environ.get('USERNAME', 'UNKNOWN')}:(OI)(CI)F"],
                         capture_output=True, text=True, timeout=15)
    if acl.returncode != 0:
        raise RuntimeError(f"could not grant private evidence directory access to SYSTEM/current user: {acl.stderr.strip()}")
    account = ps_json("Get-LocalUser -Name 'sook' -ErrorAction SilentlyContinue | Select-Object Name,Enabled,SID | ConvertTo-Json -Compress")
    if not account or account[0].get("Enabled") is not True:
        raise RuntimeError("local account sook must already exist and be enabled; harness will not create or modify accounts")
    private_keygen = root / "ssh-keygen.exe"
    shutil.copy2(keygen, private_keygen)
    host_key = root / "ssh_host_ed25519_key"
    client_key = root / "client_ed25519"
    subprocess.run([str(private_keygen), "-q", "-t", "ed25519", "-N", "", "-f", str(host_key)],
                   check=True, capture_output=True, text=True, timeout=20)
    subprocess.run([str(private_keygen), "-q", "-t", "ed25519", "-N", "", "-f", str(client_key)],
                   check=True, capture_output=True, text=True, timeout=20)
    for key_path in (host_key, Path(str(host_key) + ".pub"), client_key, Path(str(client_key) + ".pub")):
        secured = subprocess.run(["icacls.exe", str(key_path), "/inheritance:r", "/grant:r",
                                  "SYSTEM:F", f"{os.environ.get('USERNAME', 'UNKNOWN')}:F"],
                                 capture_output=True, text=True, timeout=15)
        if secured.returncode != 0:
            raise RuntimeError(f"could not secure private host-key ACL: {secured.stderr.strip()}")
    config = root / "sshd_config"
    config.write_text("\n".join([
        f"Port {port}", "ListenAddress 127.0.0.1", "AddressFamily inet",
        f"HostKey {host_key}", f"PidFile {root / 'sshd.pid'}", f"LogLevel VERBOSE",
        f"AuthorizedKeysFile {root / 'authorized_keys'}", "PubkeyAuthentication yes",
        "PasswordAuthentication no", "KbdInteractiveAuthentication no", "GSSAPIAuthentication no",
        "HostbasedAuthentication no", "PermitEmptyPasswords no", "AllowUsers sook",
        "MaxStartups 64:30:128", "LoginGraceTime 30", "StrictModes no", "UseDNS no", "",
    ]), encoding="utf-8")
    authorized_keys = root / "authorized_keys"
    authorized_keys.write_text(Path(str(client_key) + ".pub").read_text(encoding="ascii"), encoding="ascii")
    secured = subprocess.run(["icacls.exe", str(authorized_keys), "/inheritance:r", "/grant:r",
                              "SYSTEM:F", f"{os.environ.get('USERNAME', 'UNKNOWN')}:F"],
                             capture_output=True, text=True, timeout=15)
    if secured.returncode != 0:
        raise RuntimeError(f"could not secure authorized_keys ACL: {secured.stderr.strip()}")
    known_hosts = root / "known_hosts"
    known_hosts.write_text(f"[127.0.0.1]:{port} {Path(str(host_key) + '.pub').read_text(encoding='ascii')}", encoding="ascii")
    client_config = root / "ssh_config"
    client_config.write_text("\n".join([
        "Host ferryx-causal-private", "  HostName 127.0.0.1", "  User sook",
        f"  Port {port}", "  ClearAllForwardings yes", "  BatchMode yes", "  PreferredAuthentications publickey",
        "  PubkeyAuthentication yes", "  PasswordAuthentication no", "  KbdInteractiveAuthentication no",
        f"  IdentityFile {client_key}", "  IdentitiesOnly yes", "  StrictHostKeyChecking yes",
        f"  UserKnownHostsFile {known_hosts}", "  GlobalKnownHostsFile NUL",
        "  ConnectionAttempts 1", "  ConnectTimeout 12", "",
    ]), encoding="utf-8")
    manifest = {"port": port, "host": "127.0.0.1", "username": "sook",
                "sshd": str(sshd), "ssh": str(ssh), "private_host_key": str(host_key),
                "private_client_key": str(client_key),
                "known_hosts": str(known_hosts), "client_config": str(client_config),
                "authorized_keys": str(authorized_keys), "config": str(config),
                "pidfile": str(root / "sshd.pid"), "ready_file": str(root / "sshd-ready.log"),
                "task_name": "FerryxSshCausal-" + uuid.uuid4().hex,
                "sshd_directory": str(sshd.parent), "token_context": "SYSTEM", "production_ssh_port": 22}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return manifest


def start(root: Path, manifest: dict, timeout: float) -> int:
    action = f'cmd.exe /d /c "cd /d ""{manifest["sshd_directory"]}"" && ""{manifest["sshd"]}"" -D -E ""{manifest["ready_file"]}"" -f ""{manifest["config"]}"""'
    schedule_time = dt.datetime.now() + dt.timedelta(days=365)
    scheduled = schedule_time.strftime("%H:%M")
    schedule_date = schedule_time.strftime("%m/%d/%Y")
    try:
        Path(manifest["ready_file"]).write_text("", encoding="utf-8")
        event_errors: list[BaseException] = []
        def monitor_event() -> None:
            try:
                wait_for_file_event(root, Path(manifest["ready_file"]).name, timeout)
            except BaseException as error:
                event_errors.append(error)
        event_thread = threading.Thread(target=monitor_event, name="sshd-readiness-event", daemon=True)
        event_thread.start()
        subprocess.run(["schtasks.exe", "/Create", "/TN", manifest["task_name"], "/SC", "ONCE",
                        "/ST", scheduled, "/SD", schedule_date, "/RU", "SYSTEM", "/RL", "HIGHEST", "/TR", action, "/F"],
                       check=True, capture_output=True, text=True, timeout=20)
        subprocess.run(["schtasks.exe", "/Run", "/TN", manifest["task_name"]],
                       check=True, capture_output=True, text=True, timeout=20)
        disabled = subprocess.run(["schtasks.exe", "/Change", "/TN", manifest["task_name"], "/DISABLE"],
                                  capture_output=True, text=True, timeout=15)
        if disabled.returncode != 0:
            raise RuntimeError(f"could not disable unique future task trigger: {disabled.stderr.strip()}")
        event_thread.join(timeout + 2)
        if event_thread.is_alive() or event_errors:
            raise TimeoutError(f"private sshd readiness event failed: {event_errors!r}")
        pidfile = Path(manifest["pidfile"])
        server_pid = int(pidfile.read_text(encoding="ascii").strip())
        processes = snapshot()
        server = next((item for item in processes if int(item["ProcessId"]) == server_pid), None)
        if not server or Path(str(server["ExecutablePath"])).resolve() != Path(manifest["sshd"]).resolve():
            raise RuntimeError("private pidfile did not resolve to expected sshd executable")
        manifest["server_pid"] = server_pid
        manifest["server_creation_time"] = str(server["CreationDate"])
        manifest["server_executable"] = str(server["ExecutablePath"])
        manifest["baseline_process_pids"] = sorted(int(item["ProcessId"]) for item in processes)
        manifest["owned_processes"] = [{"pid": server_pid, "creation_time": str(server["CreationDate"]),
                                        "executable": str(server["ExecutablePath"]), "parent_pid": int(server["ParentProcessId"])}]
        owned_snapshot = snapshot()
        for pid in descendants(server_pid, owned_snapshot):
            child = next((item for item in owned_snapshot if int(item["ProcessId"]) == pid), None)
            if child:
                manifest["owned_processes"].append({"pid": pid, "creation_time": str(child["CreationDate"]),
                                                     "executable": str(child["ExecutablePath"]),
                                                     "parent_pid": int(child["ParentProcessId"])})
        (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        record(root, kind="ready", server_pid=server_pid, port=manifest["port"], host=manifest["host"],
               task_name=manifest["task_name"], token_context="SYSTEM")
        (root / "READY").write_text(timestamp() + "\n", encoding="ascii")
        print(json.dumps({"ready": True, "port": manifest["port"], "host": manifest["host"],
                          "private_host_key": manifest["private_host_key"],
                          "private_client_key": manifest["private_client_key"],
                          "identity_file": manifest["private_client_key"],
                          "known_hosts": manifest["known_hosts"], "client_config": manifest["client_config"],
                          "evidence_dir": str(root), "task_name": manifest["task_name"]}, indent=2))
        return 0
    except BaseException:
        try:
            if (root / "manifest.json").is_file():
                manifest.update(json.loads((root / "manifest.json").read_text(encoding="utf-8")))
            cleanup(root, manifest)
        except BaseException as cleanup_error:
            record(root, kind="startup_cleanup_error", error=repr(cleanup_error))
        raise


def trial(root: Path, manifest: dict, label: str, timeout: float, cancel: bool = False) -> dict:
    command = [manifest["ssh"], "-F", manifest["client_config"], "-vv", "-o", f"ConnectTimeout={int(timeout)}"]
    if cancel:
        command.extend(["-o", "PubkeyAuthentication=no", "-o", "PreferredAuthentications=none"])
    command.append("ferryx-causal-private")
    if not cancel:
        command.append("cmd.exe /d /c exit 0")
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.PIPE, text=True, bufsize=1)
    lines: queue.Queue[str | None] = queue.Queue()
    captured: list[str] = []

    def read_lines() -> None:
        assert process.stderr is not None
        try:
            for line in process.stderr:
                captured.append(line)
                lines.put(line)
        finally:
            lines.put(None)

    reader = threading.Thread(target=read_lines, name=f"ssh-stderr-{process.pid}", daemon=True)
    reader.start()
    deadline, kex = time.monotonic() + timeout, False
    outcome = "timeout"
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            try:
                line = lines.get(timeout=remaining)
            except queue.Empty:
                break
            if line is None:
                process.wait(timeout=max(0.1, remaining))
                outcome = "client_exited"
                break
            if re.search(r"SSH2_MSG_KEXINIT received", line):
                kex = True
                if cancel:
                    outcome = "cancelled_at_observed_kexinit_received"
                    break
            if process.poll() is not None:
                outcome = "client_exited"
                break
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=3)
        reader.join(timeout=3)
        if reader.is_alive():
            raise TimeoutError(f"SSH stderr reader did not finish for client PID {process.pid}")
        if process.stderr:
            process.stderr.close()
    if not cancel and outcome == "client_exited" and process.returncode != 0:
        outcome = f"client_exit_{process.returncode}"
    processes = snapshot()
    pids = descendants(int(manifest["server_pid"]), processes)
    cpus = ps_json("Get-Process -Id " + ",".join(str(pid) for pid in pids) + " -ErrorAction SilentlyContinue | Select-Object Id,CPU | ConvertTo-Json -Compress") if pids else []
    value = {"kind": "trial", "trial": label, "outcome": outcome, "kex_observed": kex,
             "client_pid": process.pid, "server_child_pids": pids, "server_child_cpu_seconds": cpus}
    record(root, **value)
    (root / f"{label}-{process.pid}.stderr.log").write_text("".join(captured), encoding="utf-8")
    if cancel and not kex:
        raise RuntimeError(f"cancellation trial {label} exited without observed SSH2_MSG_KEXINIT received")
    if not cancel and (outcome != "client_exited" or process.returncode != 0):
        raise RuntimeError(f"authenticated trial {label} did not complete exit 0: {outcome}, rc={process.returncode}")
    return value


def trials(root: Path, manifest: dict, timeout: float) -> None:
    for index in range(1, 21):
        trial(root, manifest, f"sequential-{index:02d}", timeout)
    errors: list[BaseException] = []
    errors_lock = threading.Lock()
    for wave in range(1, 6):
        errors.clear()
        barrier = threading.Barrier(4)
        def concurrent_trial(number: int, wave_number: int) -> None:
            try:
                barrier.wait(timeout=timeout)
                trial(root, manifest, f"concurrent-{wave_number}-{number}", timeout)
            except BaseException as error:
                with errors_lock:
                    errors.append(error)

        workers = [threading.Thread(target=concurrent_trial, args=(n, wave)) for n in range(1, 5)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join(timeout + 5)
        if any(worker.is_alive() for worker in workers):
            for worker in workers:
                if worker.is_alive():
                    worker.join(3)
            raise TimeoutError(f"concurrent wave {wave} exceeded bounded join")
        if errors:
            raise RuntimeError(f"concurrent wave {wave} failed: {errors!r}")
    for index in range(1, 21):
        trial(root, manifest, f"auth-cancel-{index:02d}", timeout, cancel=True)
        trial(root, manifest, f"auth-cancel-retry-{index:02d}", timeout)


def cleanup(root: Path, manifest: dict) -> None:
    name = manifest.get("task_name")
    processes = snapshot()
    root_pid = manifest.get("server_pid")
    owned = manifest.get("owned_processes", [])
    if root_pid:
        known = {int(item["pid"]) for item in owned}
        server_identity = next((item for item in processes if int(item["ProcessId"]) == int(root_pid)), None)
        if server_identity and (str(server_identity["CreationDate"]) != str(manifest.get("server_creation_time")) or
                                str(server_identity.get("ExecutablePath", "")).lower() != str(manifest.get("server_executable", "")).lower()):
            raise RuntimeError(f"isolated sshd PID {root_pid} identity changed; refusing process-tree cleanup")
        tree_pids = descendants(int(root_pid), processes)
        for pid in tree_pids:
            if pid in known:
                continue
            process = next((item for item in processes if int(item["ProcessId"]) == pid), None)
            if process:
                owned.append({"pid": pid, "creation_time": str(process["CreationDate"]),
                              "executable": str(process["ExecutablePath"]),
                              "parent_pid": int(process["ParentProcessId"])})
                known.add(pid)
        owned.sort(key=lambda entry: len(descendants(int(entry["pid"]), processes)), reverse=True)
    manifest["owned_processes"] = owned
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    validated: list[dict] = []
    for item in owned:
        found = ps_json(f"Get-CimInstance Win32_Process -Filter 'ProcessId={int(item['pid'])}' | Select-Object ProcessId,ParentProcessId,CreationDate,ExecutablePath | ConvertTo-Json -Compress")
        if not found:
            continue
        process = found[0]
        same = (str(process.get("CreationDate")) == str(item["creation_time"]) and
                str(process.get("ExecutablePath", "")).lower() == str(item["executable"]).lower() and
                int(process.get("ParentProcessId", -1)) == int(item["parent_pid"]))
        if not same:
            raise RuntimeError(f"owned PID {item['pid']} identity changed; refusing termination")
        validated.append(item)
    handles = open_owned_processes(validated) if validated else []
    task_error: BaseException | None = None
    try:
        if name:
            end = subprocess.run(["schtasks.exe", "/End", "/TN", name], capture_output=True, text=True, timeout=12)
            if end.returncode != 0:
                raise RuntimeError(f"could not end unique task {name}: {end.stderr.strip()}")
            delete = subprocess.run(["schtasks.exe", "/Delete", "/TN", name, "/F"], capture_output=True, text=True, timeout=12)
            if delete.returncode != 0:
                raise RuntimeError(f"could not delete unique task {name}: {delete.stderr.strip()}")
            record(root, kind="task_stopped_deleted", task_name=name)
    except BaseException as error:
        task_error = error
        record(root, kind="task_cleanup_error", task_name=name, error=repr(error))
    terminated = terminate_owned_processes(validated, handles, 10) if validated else []
    if task_error:
        raise task_error
    record(root, kind="owned_processes_terminated", pids=terminated)
    remaining_processes = snapshot()
    leftovers = []
    for item in owned:
        match = next((process for process in remaining_processes
                      if int(process["ProcessId"]) == int(item["pid"]) and
                      str(process["CreationDate"]) == str(item["creation_time"]) and
                      str(process.get("ExecutablePath", "")).lower() == str(item["executable"]).lower()), None)
        if match:
            leftovers.append(int(item["pid"]))
    if leftovers:
        raise RuntimeError(f"owned fixture process tree remains after cleanup: {leftovers}")
    task_check = subprocess.run(["schtasks.exe", "/Query", "/TN", name], capture_output=True, text=True, timeout=12) if name else None
    if task_check and task_check.returncode == 0:
        raise RuntimeError(f"unique scheduled task still exists after deletion: {name}")
    record(root, kind="cleanup_verified", terminated_pids=terminated, remaining_owned_pids=[], task_absent=True)
    (root / "READY").unlink(missing_ok=True)
    (root / "CLEANED").write_text(timestamp() + "\n", encoding="ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    launch = sub.add_parser("start")
    launch.add_argument("--sshd")
    launch.add_argument("--ssh")
    launch.add_argument("--ssh-keygen")
    launch.add_argument("--port", type=int, default=40222)
    launch.add_argument("--timeout", type=float, default=15)
    launch.add_argument("--output", type=Path, default=Path.cwd() / "ssh-causal-evidence")
    run = sub.add_parser("trials")
    run.add_argument("--evidence", type=Path, required=True)
    run.add_argument("--timeout", type=float, default=12)
    stop = sub.add_parser("cleanup")
    stop.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("run this harness on Windows maho-win")
    try:
        if args.action == "start":
            if not 1024 <= args.port <= 65535 or not 2 <= args.timeout <= 60:
                parser.error("port must be 1024..65535 and startup timeout 2..60 seconds")
            binaries = find_binary(args.sshd, "sshd"), find_binary(args.ssh, "ssh"), find_binary(args.ssh_keygen, "ssh-keygen")
            default_output = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData")) / "Ferryx" / "ssh-causal-evidence"
            output = args.output if args.output != Path.cwd() / "ssh-causal-evidence" else default_output
            output = output.resolve()
            output.mkdir(parents=True, exist_ok=True)
            root = output / f"run-{dt.datetime.now().strftime('%Y%m%d-%H%M%S')}-{uuid.uuid4().hex[:8]}"
            manifest = prepare(root, *binaries[:2], binaries[2], args.port)
            return start(root, manifest, args.timeout)
        root = args.evidence.resolve()
        manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
        if not (root / "READY").is_file():
            raise RuntimeError("isolated listener is not marked READY")
        if args.action == "trials":
            trials(root, manifest, args.timeout)
            record(root, kind="trials_complete", sequential=20, concurrent_waves=5,
                   concurrent_per_wave=4, auth_cancel_retry_pairs=20)
        else:
            cleanup(root, manifest)
        return 0
    except (OSError, RuntimeError, subprocess.SubprocessError, TimeoutError) as error:
        print(f"ERROR: {error}\n{traceback.format_exc()}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
