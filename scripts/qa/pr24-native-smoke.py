#!/usr/bin/env python3
"""Bounded actual-app X11 smoke, ONLY inside a pre-created offline network namespace.

No production state is seeded and no UI/backend call is mocked. Screenshots record
startup and injected Add Project/Escape keys; this is NOT terminal/search/image QA.
The caller creates the network namespace, enables only loopback, drops privileges,
and supplies an empty ambient environment. Never run this to evade a local sandbox.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, capture_output=True, text=True, timeout=8, **kwargs)


def assert_offline_namespace():
    if os.geteuid() == 0:
        raise RuntimeError('app must run after dropping root privileges')
    links = json.loads(run(['ip', '-j', 'link', 'show']).stdout)
    if not links or {x['ifname'] for x in links} != {'lo'}:
        raise RuntimeError('refusing: network namespace must contain only loopback')
    for family in ['-4', '-6']:
        routes = json.loads(run(['ip', family, '-j', 'route', 'show', 'table', 'all']).stdout)
        if any(x.get('dev') != 'lo' or x.get('gateway') for x in routes):
            raise RuntimeError('refusing: non-loopback route exists')
    return {'interfaces': links, 'namespace': os.readlink('/proc/self/ns/net')}


def interrupted(signum, _frame):
    raise RuntimeError(f"interrupted by signal {signum}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['binary', 'ui-dist', 'evidence-dir', 'isolation-root']:
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    network = assert_offline_namespace()
    binary, dist = args.binary.resolve(), args.ui_dist.resolve()
    if not binary.is_file() or not (dist / 'index.html').is_file():
        raise RuntimeError('built binary and built UI index.html are required')
    evidence, root = args.evidence_dir.resolve(), args.isolation_root.resolve()
    evidence.mkdir(mode=0o700, parents=True, exist_ok=False)
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    dirs = {name: root / name for name in ['home', 'data', 'runtime', 'session', 'xdg', 'config', 'cache']}
    for path in dirs.values():
        path.mkdir(mode=0o700)
    env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'LANG': 'C.UTF-8',
           'HOME': str(dirs['home']), 'XDG_RUNTIME_DIR': str(dirs['xdg']),
           'XDG_CONFIG_HOME': str(dirs['config']), 'XDG_CACHE_HOME': str(dirs['cache']),
           'FERRYX_DATA_DIR': str(dirs['data']), 'FERRYX_RUNTIME_DIR': str(dirs['runtime']),
           'FERRYX_SESSION_DIR': str(dirs['session']), 'GDK_BACKEND': 'x11',
           'LIBGL_ALWAYS_SOFTWARE': '1'}
    children, logs = [], []
    with binary.open('rb') as stream:
        binary_sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
    result = {'scope': 'actual-app startup and injected-key screenshots only',
              'terminal_search_image_qa': 'NOT_RUN',
              'resource_scope': 'app leader only; excludes WebKit, daemon, compositor and descendants', 'network': network,
              'binary_sha256': binary_sha256,
              'screenshots': [], 'samples': [], 'status': 'FAILED'}

    def spawn(name, argv):
        stream = (evidence / (name + '.log')).open('wb')
        logs.append(stream)
        child = subprocess.Popen(argv, env=env, stdout=stream, stderr=subprocess.STDOUT,
                                 stdin=subprocess.DEVNULL, start_new_session=True)
        children.append(child)
        return child

    def wait_for(predicate, description, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(.2)
        raise RuntimeError('timed out: ' + description)

    def capture(name):
        path = evidence / (name + '.png')
        run(['import', '-window', window, str(path)], env=env)
        if not path.is_file() or path.stat().st_size == 0:
            raise RuntimeError('empty screenshot')
        result['screenshots'].append({'file': path.name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})

    for signum in [signal.SIGTERM, signal.SIGHUP, signal.SIGINT]:
        signal.signal(signum, interrupted)
    try:
        # Xvfb chooses its own free display; no foreign display/daemon is reused.
        spawn('xvfb', ['Xvfb', '-displayfd', '1', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'])
        def display_ready():
            for line in (evidence / 'xvfb.log').read_text(errors='replace').splitlines():
                if line.isdigit():
                    env['DISPLAY'] = ':' + line
                    return True
            return False
        wait_for(display_ready, 'owned X server')
        spawn('dbus', ['dbus-daemon', '--session', '--nofork', '--print-address=1'])
        def bus_ready():
            for line in (evidence / 'dbus.log').read_text(errors='replace').splitlines():
                if line.startswith('unix:'):
                    env['DBUS_SESSION_BUS_ADDRESS'] = line
                    return True
            return False
        wait_for(bus_ready, 'owned session bus')
        spawn('openbox', ['openbox', '--sm-disable'])
        # Workflow builds tauri/custom-protocol: the real built UI is embedded.
        # No development server or network-access substitution is used.
        app = spawn('app', [str(binary)])
        windows = []
        def window_ready():
            if app.poll() is not None:
                raise RuntimeError('app exited before showing a window')
            found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(app.pid)],
                                   env=env, capture_output=True, text=True, timeout=5)
            windows[:] = found.stdout.split()
            return bool(windows)
        wait_for(window_ready, 'visible app-owned window', 45)
        if len(windows) != 1:
            raise RuntimeError('ambiguous owned top-level window')
        window = windows[0]
        run(['xdotool', 'windowactivate', '--sync', window], env=env)
        time.sleep(3)
        capture('01-startup')
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+o'], env=env)
        time.sleep(1)
        capture('02-add-project-key')
        run(['xdotool', 'key', '--clearmodifiers', 'Escape'], env=env)
        time.sleep(1)
        capture('03-escape-key')
        # Numeric metadata only; no argv/environment or credentials are recorded.
        for _ in range(10):
            if app.poll() is not None:
                raise RuntimeError('app exited during smoke')
            sample = run(['ps', '-o', 'pid=,ppid=,pcpu=,rss=,time=', '-p', str(app.pid)]).stdout.strip()
            result['samples'].append({'monotonic_seconds': time.monotonic(), 'app_ps': sample})
            time.sleep(.5)
        result['status'] = 'CAPTURED_REQUIRES_VISUAL_REVIEW'
    except Exception as error:
        result['error'] = str(error)
        raise
    finally:
        # Repeated termination must not interrupt bounded cleanup/artifact persistence.
        for signum in [signal.SIGTERM, signal.SIGHUP, signal.SIGINT]:
            signal.signal(signum, signal.SIG_IGN)
        result['cleanup_errors'] = []
        (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        for child in reversed(children):
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 3
        for child in reversed(children):
            try:
                child.wait(timeout=max(.01, deadline - time.monotonic()))
            except (subprocess.TimeoutExpired, OSError):
                pass
        # Kill owned groups even if their leaders exited but descendants remain.
        for child in reversed(children):
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                child.wait(timeout=3)
            except (subprocess.TimeoutExpired, OSError) as error:
                result['cleanup_errors'].append(str(error))
        for stream in logs:
            try:
                stream.close()
            except OSError as error:
                result['cleanup_errors'].append(str(error))
        (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
