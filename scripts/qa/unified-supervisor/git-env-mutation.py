#!/usr/bin/env python3
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[3]
helper = root / 'scripts/qa/unified-supervisor/git-env.mjs'
out = root / 'git-env-receipts'
out.mkdir(exist_ok=True)
command = ['bun', 'test', 'scripts/qa/unified-supervisor/git-env.test.mjs', '--test-name-pattern', 'ambient git repository']
original = helper.read_text()
anchor = 'for (const key of REPOSITORY_ENV) delete env[key];'
assert original.count(anchor) == 1
try:
    helper.write_text(original.replace(anchor, '/* mutation: retain ambient repository environment */'))
    red = subprocess.run(command, cwd=root, capture_output=True, text=True)
    (out / 'RED.log').write_text(red.stdout + red.stderr)
    assert red.returncode != 0 and 'expect(' in red.stderr, 'scrub mutation did not fail an assertion'
finally:
    helper.write_text(original)
green = subprocess.run(command, cwd=root, capture_output=True, text=True)
(out / 'GREEN.log').write_text(green.stdout + green.stderr)
receipt = f'GIT_ENV RED_EXIT={red.returncode} GREEN_EXIT={green.returncode}\n'
(out / 'mutation-exits.txt').write_text(receipt)
print(receipt, end='')
assert green.returncode == 0, green.stdout + green.stderr
