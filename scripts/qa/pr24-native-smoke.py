#!/usr/bin/env python3
"""Bounded actual-app X11 smoke, ONLY inside a pre-created offline network namespace.

No production state is seeded and no UI/backend call is mocked. Screenshots record
startup, keyboard-driven real PTY output, and terminal/image/search screenshots.
A PTY receipt verifies execution; rendering and search results require visual review.
The caller creates the network namespace, enables only loopback, drops privileges,
and supplies an empty ambient environment. Never run this to evade a local sandbox.
"""
import argparse
import base64
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import shutil
import shlex
import secrets
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


def parse_capture_metrics(output):
    fields = output.strip().split()
    if len(fields) != 4:
        raise RuntimeError('invalid screenshot metrics')
    width, height, colors = map(int, fields[:3])
    entropy = float(fields[3])
    if min(width, height, colors) <= 0 or not math.isfinite(entropy) or not 0 <= entropy <= 1:
        raise RuntimeError('invalid screenshot metric values')
    return {'width': width, 'height': height, 'colors': colors, 'entropy': entropy}


def meaningful_content(metrics):
    # Conservative blank-frame rejection, NOT proof of functional app content.
    # Metrics cover the central 80% to exclude window borders/title decorations.
    return (metrics['width'] >= 200 and metrics['height'] >= 100
            and metrics['colors'] >= 16 and metrics['entropy'] >= .02)


def nonblank_post_startup_content(metrics):
    # Once strict first-run readiness has settled, an empty workspace is legitimately
    # sparse: reviewed GUI3 has 329 colors and entropy 0.0135897. Later captures only
    # reject blank/invalid pixels; transitions, PTY receipts, and visual review remain
    # independent requirements. Do not reuse this weaker predicate for startup polling.
    return (metrics['width'] >= 200 and metrics['height'] >= 100
            and metrics['colors'] >= 16 and math.isfinite(metrics['entropy'])
            and 0 < metrics['entropy'] <= 1)


def changed_pixels(before, after):
    if len(before) != len(after) or len(before) % 4:
        raise RuntimeError('incomparable screenshot pixel buffers')
    # Ignore tiny antialiasing differences; 500 changed pixels rejects a caret-only update.
    return sum(any(abs(a - b) >= 16 for a, b in zip(before[i:i + 3], after[i:i + 3]))
               for i in range(0, len(before), 4))


def terminal_payload(marker):
    # 16x16 real RGB pixels: red/green above blue/yellow, displayed at 16x8 cells.
    colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]
    rgb = bytes(channel for y in range(16) for x in range(16)
                for channel in colors[(2 if y >= 8 else 0) + (1 if x >= 8 else 0)])
    return (marker.encode('ascii') + b'\r\n'
            + b'\x1b_Ga=T,f=24,s=16,v=16,i=7024,c=16,r=8,q=2;'
            + base64.b64encode(rgb) + b'\x1b\\\r\n')


def fixture_source(marker, receipt):
    payload = terminal_payload(marker)
    return ("import json, os, sys\nfrom pathlib import Path\n"
            "assert os.isatty(1), 'fixture stdout must be a real PTY'\n"
            f"sys.stdout.buffer.write({payload!r})\nsys.stdout.buffer.flush()\n"
            f"receipt = Path({str(receipt)!r})\n"
            f"data = {{'marker': {marker!r}, 'pty_stdout': True, 'pid': os.getpid(), 'image_pixels': 256}}\n"
            "temporary = receipt.with_suffix('.tmp')\n"
            "temporary.write_text(json.dumps(data))\ntemporary.replace(receipt)\n")


def fixture_command(script):
    return 'python3 ' + shlex.quote(str(script))


def validated_receipt(path, marker):
    data = json.loads(path.read_text())
    if (data.get('marker') != marker or data.get('pty_stdout') is not True
            or not isinstance(data.get('pid'), int) or data['pid'] <= 0
            or data.get('image_pixels') != 256):
        raise RuntimeError('invalid real-PTY fixture receipt')
    return data


def guarded_skip_setup_point(width, height, expected_pid, owner_pid, window_id, active_window_id):
    # Calibrated against the reviewed first-run screenshot for this fixed Xvfb fixture.
    # WelcomeWizard.tsx's visible onboarding-skip button invokes normal onSkip; this
    # is NOT the sign-in, Continue, install-CLI, or permission-grant affordance.
    # Refuse changed geometry/ownership rather than scale coordinates or guess.
    if (width, height) != (1278, 850):
        raise RuntimeError('Skip setup coordinate requires reviewed 1278x850 client geometry')
    if expected_pid <= 0 or owner_pid != expected_pid or active_window_id != window_id:
        raise RuntimeError('Skip setup requires the active app-owned window')
    return 530, 599


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
    result = {'scope': 'actual-app startup, real-PTY fixture execution, and terminal/image/search screenshots',
              'terminal_search_image_qa': 'NOT_RUN',
              'resource_scope': 'app leader only; excludes WebKit, daemon, compositor and descendants', 'network': network,
              'binary_sha256': binary_sha256,
              'screenshots': [], 'readiness_probes': [], 'samples': [], 'status': 'FAILED'}

    def spawn(name, argv, cwd=None):
        stream = (evidence / (name + '.log')).open('wb')
        logs.append(stream)
        child = subprocess.Popen(argv, env=env, stdout=stream, stderr=subprocess.STDOUT,
                                 stdin=subprocess.DEVNULL, start_new_session=True, cwd=cwd)
        children.append(child)
        return child

    def wait_for(predicate, description, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(.2)
        raise RuntimeError('timed out: ' + description)

    pixel_frames = {}

    def capture_pair(name, record=True):
        path = evidence / (name + '.png')
        root_path = evidence / (name + '-root.png')
        run(['import', '-window', window, str(path)], env=env)
        run(['import', '-window', 'root', str(root_path)], env=env)
        for image_path in [path, root_path]:
            if not image_path.is_file() or image_path.stat().st_size == 0:
                raise RuntimeError('empty screenshot')
        metrics = parse_capture_metrics(run([
            'convert', str(path), '-gravity', 'center', '-crop', '80%x80%+0+0',
            '+repage', '-format', '%w %h %k %[entropy]', 'info:'], env=env).stdout)
        if record:
            pixels = subprocess.run(['convert', str(path), '-gravity', 'center',
                '-crop', '80%x80%+0+0', '+repage', '-depth', '8', 'rgba:-'],
                env=env, check=True, capture_output=True, timeout=8).stdout
            if len(pixels) != metrics['width'] * metrics['height'] * 4:
                raise RuntimeError('unexpected screenshot pixel buffer size')
            pixel_frames[name] = pixels
            for image_path in [path, root_path]:
                result['screenshots'].append({'file': image_path.name,
                    'sha256': hashlib.sha256(image_path.read_bytes()).hexdigest()})
            result['screenshots'][-2]['center_metrics'] = metrics
            result['screenshots'][-2]['center_rgba_sha256'] = hashlib.sha256(pixels).hexdigest()
        return metrics

    def capture(name, startup=False):
        metrics = capture_pair(name)
        valid = meaningful_content(metrics) if startup else nonblank_post_startup_content(metrics)
        if not valid:
            raise RuntimeError(f'blank or insufficient rendered content: {name}: {metrics}')

    def await_rendered_content():
        end = time.monotonic() + 60
        consecutive = 0
        while time.monotonic() < end:
            if app.poll() is not None:
                raise RuntimeError('app exited while waiting for rendered content')
            metrics = capture_pair('readiness-last', record=False)
            ready = meaningful_content(metrics)
            result['readiness_probes'].append({'monotonic_seconds': time.monotonic(),
                                               'center_metrics': metrics, 'nonblank': ready})
            consecutive = consecutive + 1 if ready else 0
            if not ready:
                for suffix in ['.png', '-root.png']:
                    shutil.copyfile(evidence / ('readiness-last' + suffix),
                                    evidence / ('readiness-last-rejected' + suffix))
            if consecutive >= 2:
                return
            time.sleep(1)
        raise RuntimeError('render readiness timed out after 60s; last owned/root frames preserved')

    for signum in [signal.SIGTERM, signal.SIGHUP, signal.SIGINT]:
        signal.signal(signum, interrupted)
    try:
        fixture_dir = root / 'fixture-repo'
        fixture_dir.mkdir(mode=0o700)
        run(['git', 'init', '--initial-branch=main', str(fixture_dir)], env=env)
        run(['git', '-C', str(fixture_dir), '-c', 'user.name=Ferryx QA',
             '-c', 'user.email=qa@invalid', '-c', 'commit.gpgsign=false',
             'commit', '--allow-empty', '-m', 'Owned offline QA fixture'], env=env)
        marker = 'FERRYX-QA-' + secrets.token_hex(8)
        receipt_path = root / 'pty-receipt.json'
        fixture_script = fixture_dir / 'fixture.py'
        fixture_script.write_text(fixture_source(marker, receipt_path))
        result['fixture'] = {'marker': marker, 'expected_search_count': '1/1',
                             'expected_image': 'red/green above blue/yellow quadrants'}
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
        app = spawn('app', [str(binary)], cwd=fixture_dir)
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
        await_rendered_content()
        capture('01-startup', startup=True)
        geometry = run(['xdotool', 'getwindowgeometry', '--shell', window], env=env).stdout
        values = dict(line.split('=', 1) for line in geometry.splitlines() if '=' in line)
        owner_pid = int(run(['xdotool', 'getwindowpid', window], env=env).stdout.strip())
        active_window = int(run(['xdotool', 'getactivewindow'], env=env).stdout.strip())
        x, y = guarded_skip_setup_point(int(values['WIDTH']), int(values['HEIGHT']),
                                       app.pid, owner_pid, int(window), active_window)
        screenshot_size = run(['identify', '-format', '%w %h',
                               str(evidence / '01-startup.png')], env=env).stdout.strip()
        if screenshot_size != '1278 850':
            raise RuntimeError('Skip setup screenshot geometry differs from reviewed client')
        # Exactly one visible normal-UI Skip setup click. Never enter account details,
        # submit Sign in/Continue, or fabricate onboarding state through IPC/storage.
        run(['xdotool', 'mousemove', '--sync', '--window', window, str(x), str(y),
             'click', '1'], env=env)
        result['setup_action'] = {'action': 'visible Skip setup click', 'window': window,
                                  'client_x': x, 'client_y': y}
        time.sleep(2)
        capture('01b-setup-skipped')
        skipped_changes = changed_pixels(pixel_frames['01-startup'], pixel_frames['01b-setup-skipped'])
        result['setup_action']['changed_pixels'] = skipped_changes
        if skipped_changes < 500:
            raise RuntimeError('Skip setup click did not produce sufficient visible change')
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+o'], env=env)
        time.sleep(1)
        capture('02-add-project-key')
        run(['xdotool', 'key', '--clearmodifiers', 'Escape'], env=env)
        time.sleep(1)
        capture('03-escape-key')
        result['interaction_pixel_changes'] = {
            'add_project': changed_pixels(pixel_frames['01b-setup-skipped'], pixel_frames['02-add-project-key']),
            'escape': changed_pixels(pixel_frames['02-add-project-key'], pixel_frames['03-escape-key']),
        }
        if any(count < 500 for count in result['interaction_pixel_changes'].values()):
            raise RuntimeError('injected key produced insufficient visible content change; screenshots require review')
        # Normal product keyboard path: no direct IPC or fabricated persisted state.
        # shortcuts.ts workspace.select1 maps to Alt+1 on Linux; App.tsx routes it
        # through handleSelectWorktreeByIndex. Registration alone does not select it.
        run(['xdotool', 'key', '--clearmodifiers', 'alt+1'], env=env)
        time.sleep(1)
        capture('03b-worktree-selected')
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+t'], env=env)
        time.sleep(3)
        capture('03c-terminal-before-input')

        command = fixture_command(fixture_script)
        if marker in command:
            raise RuntimeError('marker must not appear in the echoed shell command')
        run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', command], env=env)
        run(['xdotool', 'key', '--clearmodifiers', 'Return'], env=env)
        try:
            wait_for(receipt_path.is_file, 'actual PTY fixture receipt', 30)
            result['pty_receipt'] = validated_receipt(receipt_path, marker)
        except Exception:
            # Preserve the actual terminal/UI even if still blank or refused. Do not
            # retry the command, execute the fixture ourselves, or hide the failure.
            try:
                capture_pair('04-pty-receipt-failed')
            except Exception as diagnostic_error:
                result['receipt_failure_capture_error'] = str(diagnostic_error)
            raise

        shutil.copyfile(receipt_path, evidence / 'pty-receipt.json')
        time.sleep(2)
        capture('04-terminal-marker-image')
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+f'], env=env)
        run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', marker], env=env)
        time.sleep(2)
        capture('05-terminal-search')
        run(['xdotool', 'key', '--clearmodifiers', 'Escape'], env=env)
        time.sleep(1)
        capture('06-search-cancelled')
        geometry = run(['xdotool', 'getwindowgeometry', '--shell', window], env=env).stdout
        values = dict(line.split('=', 1) for line in geometry.splitlines() if '=' in line)
        width, height = int(values['WIDTH']), int(values['HEIGHT'])
        run(['xdotool', 'windowsize', '--sync', window, str(max(640, width - 80)),
             str(max(480, height - 60))], env=env)
        time.sleep(1)
        run(['xdotool', 'windowsize', '--sync', window, str(width), str(height)], env=env)
        time.sleep(2)
        capture('07-image-after-redraw')
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+f'], env=env)
        run(['xdotool', 'key', '--clearmodifiers', 'ctrl+a'], env=env)
        run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', marker], env=env)
        time.sleep(2)
        capture('08-search-repeat')
        result['terminal_search_image_qa'] = {
            'pty_execution': 'VERIFIED_RECEIPT_WITH_ISATTY',
            'marker_rendering': 'REQUIRES_VISUAL_REVIEW',
            'image_quadrants': 'REQUIRES_VISUAL_REVIEW',
            'image_after_resize_redraw': 'REQUIRES_VISUAL_REVIEW',
            'search_cancel_and_repeat': 'REQUIRES_VISUAL_REVIEW',
            'search_expected_1_of_1': 'REQUIRES_VISUAL_REVIEW',
        }
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
