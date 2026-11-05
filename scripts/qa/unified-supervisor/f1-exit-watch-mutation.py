#!/usr/bin/env python3
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[3]
source = root / 'scripts/qa/unified-supervisor/f1-exit-watch.mjs'
out = root / 'f1watch-receipts'
out.mkdir(exist_ok=True)
original = source.read_text()
mutations = [
    ('selection', 'proc capability selection', "elif ${kqueue}:", 'elif True:'),
    ('exit', 'proc real child exit', "print('EXIT',flush=True); break", "print('TIMEOUT',flush=True); break"),
    ('reuse', 'proc reused pid', "if current()!=stamp:", 'if current() is None:'),
    ('deadline', 'proc deadline', "print('TIMEOUT',flush=True); break", "print('EXIT',flush=True); break"),
    ('stamp-change', 'proc start stamp change', "if current()!=stamp:\n    print('EXIT',flush=True); break", "if current() is None:\n    print('EXIT',flush=True); break"),
    ('unobservable', 'proc absent pid', "  with open('%s/self/stat'%root) as stream: stream.read()\n", ''),
    ('malformed', 'proc malformed identity', "fields=stream.read().rsplit(') ',1)[1].split()", "fields=('S '+'0 '*18+'123').split()"),
    ('signal', 'proc host-helper signal', 'os.kill(pid,getattr(signal,sig))', 'pass'),
]
receipts = []
for name, pattern, anchor, replacement in mutations:
    assert anchor in original, name
    command = ['bun', 'test', 'scripts/qa/unified-supervisor/f1-exit-watch.test.mjs', '--test-name-pattern', pattern]
    try:
        source.write_text(original.replace(anchor, replacement))
        red = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=30)
        (out / (name + '-RED.log')).write_text(red.stdout + red.stderr)
        assert red.returncode != 0 and 'expect(' in red.stderr, name + ' mutation did not fail an assertion'
    finally:
        source.write_text(original)
    green = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=30)
    (out / (name + '-GREEN.log')).write_text(green.stdout + green.stderr)
    receipt = '{} RED_EXIT={} GREEN_EXIT={}\n'.format(name, red.returncode, green.returncode)
    receipts.append(receipt)
    print(receipt, end='', flush=True)
    (out / 'mutation-exits.txt').write_text(''.join(receipts))
    assert green.returncode == 0, green.stdout + green.stderr
