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
import re
from pathlib import Path
import signal
import stat
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


class SmokeInterrupted(BaseException):
    """Termination must bypass best-effort diagnostic `except Exception` blocks."""
    def __init__(self, signum):
        self.signum = signum
        super().__init__(f"interrupted by signal {signum}")


def interrupted(signum, _frame):
    raise SmokeInterrupted(signum)


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
    # Recalibrated after the onboarding wizard grew (agent install help, phone and first-worktree
    # steps): the footer, and with it Skip setup, now sits at y=716 in the reviewed screenshot.
    return 530, 716


def xprop_window_ids(output, property_name):
    match = re.fullmatch(re.escape(property_name) + r"\(WINDOW\): window id #\s*(.*)", output.strip())
    if not match:
        return []
    values = match.group(1).strip()
    if not values:
        return []
    tokens = [value.strip() for value in values.split(',')]
    if not all(re.fullmatch(r"0x[0-9a-fA-F]+", token) for token in tokens):
        return []
    return [int(token, 16) for token in tokens]


def xprop_supported_atoms(output):
    match = re.fullmatch(r"_NET_SUPPORTED\(ATOM\) = (.*)", output.strip())
    return set(value.strip() for value in match.group(1).split(',')) if match else set()


def wm_check_ready(root_check, child_check, supported):
    root_ids = xprop_window_ids(root_check, '_NET_SUPPORTING_WM_CHECK')
    return (len(root_ids) == 1 and root_ids[0] != 0
            and xprop_window_ids(child_check, '_NET_SUPPORTING_WM_CHECK') == root_ids
            and '_NET_ACTIVE_WINDOW' in xprop_supported_atoms(supported))


def bounded_command_failure(error):
    stderr = error.stderr or ''
    if isinstance(stderr, bytes):
        stderr = stderr.decode('utf-8', errors='replace')
    return {'returncode': getattr(error, 'returncode', None),
            'timed_out': isinstance(error, subprocess.TimeoutExpired), 'stderr': stderr[:2048]}


OWNED_DEBUG_LOGS = ('ferryx-switch-debug.jsonl', 'ferryx-switch-debug-render.jsonl')
MAX_DEBUG_TAIL = 1024 * 1024


def owned_debug_tail(directory, name, limit=MAX_DEBUG_TAIL):
    if name not in OWNED_DEBUG_LOGS or not 0 < limit <= MAX_DEBUG_TAIL:
        raise ValueError('unapproved debug log or tail bound')
    directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        if os.fstat(directory_fd).st_uid != os.geteuid():
            raise RuntimeError('debug directory is not owned by this run user')
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory_fd)
        try:
            info = os.fstat(descriptor)
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_nlink != 1:
                raise RuntimeError('debug log must be an owned regular single-link file')
            start = max(0, info.st_size - limit)
            os.lseek(descriptor, start, os.SEEK_SET)
            data = os.read(descriptor, limit)
            # Avoid a truncated leading JSON record when the log exceeded the bound.
            if start:
                data = data.partition(b'\n')[2]
            return data
        finally:
            os.close(descriptor)
    finally:
        os.close(directory_fd)


def parse_x_numeric(output):
    value = output.strip()
    if not re.fullmatch(r'[0-9]+', value) or not 0 <= int(value) <= 0xffffffff:
        raise ValueError('expected only a numeric X window/PID value')
    return int(value)


def ctrl_m_diagnostic_allowed(receipt_exists, snapshot):
    pid, window = snapshot.get('app_pid'), snapshot.get('owned_window')
    return (not receipt_exists and isinstance(pid, int) and pid > 0
            and isinstance(window, int) and window > 0
            and snapshot.get('active_window') == window
            and snapshot.get('resolved_focus_window') == window
            and snapshot.get('active_window_owner_pid') == pid
            and snapshot.get('resolved_focus_window_owner_pid') == pid)


def terminal_smoke_status(original_return_failed):
    return 'FAILED_RETURN_KEY_DIAGNOSTIC' if original_return_failed else 'CAPTURED_REQUIRES_VISUAL_REVIEW'


class WaitTimeout(RuntimeError):
    pass


LINUX_SEARCH_CHORD = 'ctrl+shift+f'


def search_review_requirements():
    # Pixel variation alone cannot distinguish a search overlay from terminal output.
    # No browser/IPC mock or guessed accessibility result may turn capture into PASS.
    return {'automated_search_verified': False, 'status': 'REQUIRES_VISUAL_REVIEW',
            'required_checks': ['search overlay visible before query typing',
                                'unique marker query and 1/1 count visible',
                                'Escape closes the overlay',
                                'reopened overlay again shows marker and 1/1'],
            'shortcut': LINUX_SEARCH_CHORD}


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
    dirs = {name: root / name for name in ['home', 'data', 'runtime', 'session', 'xdg', 'config', 'cache', 'tmp']}
    for path in dirs.values():
        path.mkdir(mode=0o700)
    env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'LANG': 'C.UTF-8',
           'HOME': str(dirs['home']),
           'TMPDIR': str(dirs['tmp']), 'TMP': str(dirs['tmp']), 'TEMP': str(dirs['tmp']), 'XDG_RUNTIME_DIR': str(dirs['xdg']),
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
        raise WaitTimeout('timed out: ' + description)

    def focus_snapshot(stage):
        snapshot = {'stage': stage, 'monotonic_seconds': time.monotonic(),
                    'app_pid': app.pid, 'owned_window': int(window)}
        for field, command in [('focus_window', ['getwindowfocus', '-f']),
                               ('active_window', ['getactivewindow']),
                               ('resolved_focus_window', ['getwindowfocus'])]:
            try:
                value = parse_x_numeric(subprocess.run(['xdotool', *command], env=env, check=True,
                        capture_output=True, text=True, timeout=2).stdout)
                snapshot[field] = value
                if value:
                    snapshot[field + '_owner_pid'] = parse_x_numeric(
                        subprocess.run(['xdotool', 'getwindowpid', str(value)], env=env, check=True,
                            capture_output=True, text=True, timeout=2).stdout)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                snapshot[field + '_error'] = bounded_command_failure(error)
            except ValueError:
                snapshot[field + '_error'] = {'code': 'INVALID_NUMERIC_METADATA'}
        result.setdefault('focus_snapshots', []).append(snapshot)
        return snapshot

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
        wm = spawn('openbox', ['openbox', '--sm-disable'])
        # EWMH sections 3.1/3.2/3.10: observe readiness, not arbitrary startup sleeps.
        # https://specifications.freedesktop.org/wm/latest-single/
        # No WM PID property is required by this standard. This owned X server has
        # only the Openbox process launched above; also require that child alive.
        def property_value(target, name):
            response = subprocess.run(['xprop', *target, name], env=env,
                                      capture_output=True, text=True, timeout=2)
            if response.returncode:
                result['wm_last_probe_error'] = {'returncode': response.returncode,
                                                'stderr': response.stderr[:2048]}
                return ''
            return response.stdout[:16384]

        def wm_ready():
            if wm.poll() is not None:
                raise RuntimeError('owned window manager exited before readiness')
            root_check = property_value(['-root'], '_NET_SUPPORTING_WM_CHECK')
            ids = xprop_window_ids(root_check, '_NET_SUPPORTING_WM_CHECK')
            if len(ids) != 1 or ids[0] == 0:
                return False
            child_check = property_value(['-id', hex(ids[0])], '_NET_SUPPORTING_WM_CHECK')
            supported = property_value(['-root'], '_NET_SUPPORTED')
            ready = wm_check_ready(root_check, child_check, supported)
            if ready:
                result['wm_ready'] = {'check_window': ids[0], 'supports_active_window': True}
            return ready
        wait_for(wm_ready, 'owned EWMH window-manager readiness', 15)

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
        def managed_window_ready():
            if wm.poll() is not None or app.poll() is not None:
                raise RuntimeError('owned app/window manager exited before window management')
            clients = xprop_window_ids(property_value(['-root'], '_NET_CLIENT_LIST'), '_NET_CLIENT_LIST')
            return int(window) in clients
        wait_for(managed_window_ready, 'owned app in EWMH managed-client list', 15)
        try:
            run(['xdotool', 'windowactivate', '--sync', window], env=env)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            result['activation_error'] = bounded_command_failure(error)
            raise RuntimeError('owned-window activation failed; see bounded activation_error diagnostics') from None

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
        focus_snapshot('before_fixture_typing')
        run(['xdotool', 'type', '--clearmodifiers', '--delay', '5', command], env=env)
        focus_snapshot('before_fixture_return')
        run(['xdotool', 'key', '--clearmodifiers', 'Return'], env=env)
        focus_snapshot('after_fixture_return')
        try:
            wait_for(receipt_path.is_file, 'actual PTY fixture receipt', 30)
        except WaitTimeout as original_error:
            result['original_return_failure'] = {'error': str(original_error), 'wait_seconds': 30}
            result['status'] = terminal_smoke_status(True)
            try:
                capture_pair('04-pty-receipt-failed')
            except Exception as diagnostic_error:
                result['receipt_failure_capture_error'] = str(diagnostic_error)
            (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            # Controlled diagnostic only: never retype or run the fixture outside the
            # app. A late/invalid receipt or uncertain/foreign focus forbids Ctrl+M.
            snapshot = focus_snapshot('before_ctrl_m_diagnostic')
            if not ctrl_m_diagnostic_allowed(os.path.lexists(receipt_path), snapshot):
                result['ctrl_m_diagnostic'] = {'status': 'REFUSED_UNSAFE_OR_LATE_RECEIPT'}
                raise RuntimeError('original Return failed; controlled Ctrl+M diagnostic refused') from None
            result['ctrl_m_diagnostic'] = {'status': 'ONE_INPUT_SENT', 'method': 'CTRL_M'}
            run(['xdotool', 'key', '--clearmodifiers', 'ctrl+m'], env=env)
            focus_snapshot('after_ctrl_m_diagnostic')
            wait_for(receipt_path.is_file, 'controlled Ctrl+M diagnostic receipt', 10)
            result['pty_receipt'] = validated_receipt(receipt_path, marker)
            result['ctrl_m_diagnostic']['status'] = 'VALID_RECEIPT_NOT_RETURN_SUCCESS'
            result['pty_receipt_method'] = 'CTRL_M_DIAGNOSTIC'
        else:
            # Invalid receipts are failures, never grounds for sending another key.
            try:
                result['pty_receipt'] = validated_receipt(receipt_path, marker)
            except Exception:
                try:
                    capture_pair('04-invalid-pty-receipt')
                except Exception as diagnostic_error:
                    result['receipt_failure_capture_error'] = str(diagnostic_error)
                raise
            result['pty_receipt_method'] = 'RETURN'

        shutil.copyfile(receipt_path, evidence / 'pty-receipt.json')
        time.sleep(2)
        capture('04-terminal-marker-image')
        result['search_review'] = search_review_requirements()
        run(['xdotool', 'key', '--clearmodifiers', LINUX_SEARCH_CHORD], env=env)
        time.sleep(1)
        capture('04b-search-overlay-before-query')
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
        run(['xdotool', 'key', '--clearmodifiers', LINUX_SEARCH_CHORD], env=env)
        time.sleep(1)
        capture('07b-search-overlay-before-repeat-query')
        # TerminalSearchOverlay focuses/selects its input on mount. Do not send
        # Ctrl+A speculatively: if the overlay is absent it would edit the shell line.
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
            'automated_search_verified': False,
        }
        # Numeric metadata only; no argv/environment or credentials are recorded.
        for _ in range(10):
            if app.poll() is not None:
                raise RuntimeError('app exited during smoke')
            sample = run(['ps', '-o', 'pid=,ppid=,pcpu=,rss=,time=', '-p', str(app.pid)]).stdout.strip()
            result['samples'].append({'monotonic_seconds': time.monotonic(), 'app_ps': sample})
            time.sleep(.5)
        result['status'] = terminal_smoke_status('original_return_failure' in result)
        if 'original_return_failure' in result:
            raise RuntimeError('original Return failed; later Ctrl+M diagnostics cannot make this run pass')
    except SmokeInterrupted as error:
        result['error'] = str(error)
        result['interrupted_signal'] = error.signum
        if 'original_return_failure' in result:
            result['status'] = terminal_smoke_status(True)
        raise
    except Exception as error:
        result['error'] = str(error)
        if 'original_return_failure' in result:
            result['status'] = terminal_smoke_status(True)
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
        # Product debug.rs sanitizes these records before persistence. Collect only
        # the two known logs from this run's private TMPDIR, never shared /tmp files.
        result['owned_debug_logs'] = []
        for name in OWNED_DEBUG_LOGS:
            try:
                tail = owned_debug_tail(dirs['tmp'], name)
                (evidence / name).write_bytes(tail)
                result['owned_debug_logs'].append({'file': name, 'tail_bytes': len(tail)})
            except FileNotFoundError:
                result['owned_debug_logs'].append({'file': name, 'status': 'NOT_PRESENT'})
            except (OSError, RuntimeError) as error:
                result['owned_debug_logs'].append({'file': name, 'status': 'REFUSED_OR_UNREADABLE',
                                                  'error_type': type(error).__name__})
        (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
