#!/usr/bin/env python3
"""Exercise unmodified rendered configurations with real, prebuilt binaries.

For an ephemeral Linux CI runner only: bb_runner executes unsandboxed commands.
All endpoints are loopback/Unix sockets. This does not deploy a physical fleet,
compile Rust, prove multi-host throughput, or test production authentication.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import uuid

from configure import render, write_output


def stop(process):
    # Only groups created by this harness, never process-name/global cleanup.
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            process.wait(timeout=5)
            return
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=5)


def cleanup_processes(processes, logs):
    errors = []
    for process in reversed(processes):
        try:
            stop(process)
        except Exception as error:
            errors.append(f'process {process.pid}: {error}')
    for log in logs:
        try:
            log.close()
        except Exception as error:
            errors.append(f'log close: {error}')
    return errors


def marker_indices(cell, nonce):
    found = set()
    for output in (cell / 'buck-out').rglob('worker-*.txt'):
        if output.is_file():
            lines = output.read_text().splitlines()
            for index in range(8):
                if len(lines) == 2 and lines[0] and lines[1] == f'buildbarn-smoke-{index}-{nonce}':
                    found.add(index)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    bins = args.bin_dir.resolve()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    processes, logs, results = [], [], []
    result = {'status': 'failed', 'scope': 'loopback Linux transport, two sequential workers', 'checks': results}
    with tempfile.TemporaryDirectory(prefix='bb-ci-', dir='/tmp') as temp:
        root = Path(temp)
        home = root / 'home'
        home.mkdir(mode=0o700)
        env = {'PATH': '/usr/local/bin:/usr/bin:/bin', 'HOME': str(home), 'TMPDIR': temp, 'LANG': 'C.UTF-8'}
        profile = {'coordinator': {'address': '127.0.0.1', 'state_dir': str(root / 'coordinator')},
                   'workers': {name: {'os': 'linux', 'arch': 'x86_64', 'toolchain': 'shell-v1',
                                      'concurrency': 2, 'state_dir': str(root / name)}
                               for name in ('a', 'b')}}
        files = render(None, profile)
        config = root / 'config'
        write_output(config, files)
        manifest = files['manifest.json']
        for directory in manifest['coordinator_directories'] + [d for w in manifest['workers'].values() for d in w['directories']]:
            Path(directory).mkdir(parents=True, mode=0o700, exist_ok=True)
        cell = config / 'buck2/linux-x86_64-shell-v1'

        def start(name, binary, relative):
            log = (evidence / f'{name}.log').open('w')
            logs.append(log)
            p = subprocess.Popen([str(bins / binary), str(config / relative)], cwd=root,
                                 env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            processes.append(p)
            return p

        def ready(process, endpoint):
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f'service exited before readiness: {endpoint}')
                try:
                    if isinstance(endpoint, int):
                        with socket.create_connection(('127.0.0.1', endpoint), timeout=0.5):
                            return
                    else:
                        with socket.socket(socket.AF_UNIX) as sock:
                            sock.settimeout(0.5)
                            sock.connect(str(endpoint))
                            return
                except OSError:
                    time.sleep(0.1)
            raise RuntimeError(f'service readiness timeout: {endpoint}')

        def build(label, nonce):
            with (evidence / f'{label}.log').open('w') as log:
                p = subprocess.Popen([str(bins / 'buck2'), 'build', '//:smoke', '--remote-only',
                                      '--no-remote-cache', '--materializations', 'all', '-c', f'smoke.nonce={nonce}'],
                                     cwd=cell, env=env, stdout=log, stderr=subprocess.STDOUT,
                                     start_new_session=True)
                try:
                    return p.wait(timeout=150)
                except subprocess.TimeoutExpired:
                    raise RuntimeError(f'{label}: timed out (not a successful negative check)')
                finally:
                    stop(p)

        try:
            # Fail before launching if another service owns one of our fixed ports.
            sockets = []
            try:
                for port in range(8980, 8985):
                    sock = socket.socket()
                    sockets.append(sock)
                    sock.bind(('127.0.0.1', port))
            finally:
                for sock in sockets:
                    sock.close()
            storage = start('storage', 'bb_storage', 'coordinator/storage.json')
            ready(storage, 8981)
            scheduler = start('scheduler', 'bb_scheduler', 'coordinator/scheduler.json')
            ready(scheduler, 8982)
            ready(scheduler, 8983)
            frontend = start('frontend', 'bb_storage', 'coordinator/frontend.json')
            ready(frontend, 8980)
            for name in ('a', 'b'):
                runner = start(f'runner-{name}', 'bb_runner', f'workers/{name}/runner.json')
                ready(runner, root / name / 'runner.sock')
                worker = start(f'worker-{name}', 'bb_worker', f'workers/{name}/worker.json')
                nonce = uuid.uuid4().hex
                code = build(f'worker-{name}', nonce)
                if code != 0 or marker_indices(cell, nonce) != set(range(8)):
                    raise RuntimeError(f'worker {name}: remote-only build/output verification failed ({code})')
                if worker.poll() is not None or runner.poll() is not None:
                    raise RuntimeError(f'worker {name} exited unexpectedly')
                results.append({'worker': name, 'nonce': nonce, 'outputs_verified': 8})
                stop(worker)
                stop(runner)
            nonce = uuid.uuid4().hex
            code = build('no-workers', nonce)
            if code == 0 or marker_indices(cell, nonce):
                raise RuntimeError('remote-only build unexpectedly succeeded/materialized outputs without workers')
            if any(p.poll() is not None for p in (storage, scheduler, frontend)):
                raise RuntimeError('negative test invalid: coordinator process exited')
            results.append({'no_workers': 'failed as required', 'exit_code': code, 'nonce': nonce})
            result['status'] = 'passed'
        except Exception as error:
            result['error'] = str(error)
            raise
        finally:
            # Always attempt every cleanup and persist evidence, even if one fails.
            cleanup_errors = []
            try:
                killed = subprocess.run([str(bins / 'buck2'), 'kill'], cwd=cell, env=env,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                        timeout=15, check=False)
                if killed.returncode:
                    cleanup_errors.append(f'buck2 kill returned {killed.returncode}')
            except (OSError, subprocess.TimeoutExpired) as error:
                cleanup_errors.append(f'buck2 kill: {error}')
            cleanup_errors.extend(cleanup_processes(processes, logs))
            if cleanup_errors:
                result['cleanup_errors'] = cleanup_errors
                result['status'] = 'failed'
            (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            if cleanup_errors:
                raise RuntimeError('cleanup incomplete; see result.json; discard ephemeral runner')



if __name__ == '__main__':
    main()
