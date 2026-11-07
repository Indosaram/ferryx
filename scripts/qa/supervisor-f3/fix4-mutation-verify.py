#!/usr/bin/env python3
"""CI-only F6-4 and F1 portability mutation evidence."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[3]
out = root / 'fix4-receipts'
out.mkdir(exist_ok=True)
qa = root / 'scripts/qa'
mutations = []
helper = qa / 'supervisor-f3/host-helper.mjs'
test = qa / 'supervisor-f3/gui-checkout.test.mjs'
for method in ['activateGui', 'guiBounds']:
    for case in ['candidate', 'system', 'sibling prefix', 'symlink outside', 'unrecorded checkout', 'ancestor checkout', 'missing checkout', 'relative checkout', 'relative executable', 'symlink checkout']:
        old = f"executable = guiExecutable(executable, checkout, '{'ACTIVATE_REFUSED' if method == 'activateGui' else 'BOUNDS_REFUSED'}');"
        new = "throw fail('MUTATED', 'candidate rejected');" if case == 'candidate' else 'executable = executable;'
        mutations.append((f'F6-4-{method}-{case.replace(" ", "-")}', helper, old, new, test, f'{method} {case}'))
# N3.3: the coarse `throw fail('MUTATED')` above replaces the whole guard. These two mutations
# delete or weaken exactly one half of it, so each half is shown to be load-bearing on its own.
mutations.append(('F6-4-guiExecutable-symlink-half-only', helper,
                 "  if (root !== checkout || gitTopLevel(root) !== root || !inside(root, binary)) throw refused();",
                 "  if (gitTopLevel(root) !== root || !inside(root, binary)) throw refused();",
                 test, 'activateGui symlink checkout'))
mutations.append(('F6-4-guiExecutable-no-realpath', helper,
                 '    binary = realpathSync(executable);',
                 '    binary = executable;',
                 test, 'activateGui symlink outside'))
source = qa / 'unified-supervisor/f1-git-objects.mjs'
f1test = qa / 'unified-supervisor/f1-git-objects.test.mjs'
for case in ['explicit', 'repository']:
    mutations.append((f'F1-{case}', source, 'return { path: canonical, reason: null };', "return { path: null, reason: 'mutated' };", f1test, f'F1 git objects {case}'))
for case in ['missing', 'relative', 'empty']:
    mutations.append((f'F1-{case}', source, 'return { path: null, reason: error.message };', 'return { path: override, reason: null };', f1test, f'F1 git objects {case}'))
# N3.2: the bwrap precondition must be the only thing that flips readiness, so the mutation
# forces it true and the dedicated test has to go red on that alone.
mutations.append(('F1-bwrap', qa / 'unified-supervisor/f1-isolation-check.mjs',
                  'bwrap: exists(bwrapPath),', 'bwrap: true,', f1test, 'F1 bubblewrap alone'))
summary = []
# Strict RED detection. `'expect(' in red.stdout` was non-discriminating: bun prints
# "N expect() calls" even for a passing run, so that guard accepted a mutation that never
# turned anything red. Require an actual bun test-failure line for the pattern under test.
import re

def red_is_real(stdout: str, pattern: str) -> bool:
    escaped = re.escape(pattern)
    return re.search(rf'^\(fail\)\s.*{escaped}', stdout, re.MULTILINE) is not None

for name, path, old, new, test, pattern in mutations:
    original = path.read_text()
    assert original.count(old) == 1, (name, 'anchor not unique')
    command = ['bun', 'test', str(test), '--test-name-pattern', pattern]
    try:
        path.write_text(original.replace(old, new))
        red = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (out / f'{name}-RED.log').write_text(red.stdout)
        assert red.returncode != 0 and red_is_real(red.stdout, pattern), (name, red.stdout[-4000:])
    finally:
        path.write_text(original)
    green = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    (out / f'{name}-GREEN.log').write_text(green.stdout)
    assert green.returncode == 0, (name, green.stdout)
    summary.append(f'{name} RED_EXIT={red.returncode} GREEN_EXIT={green.returncode}')
    (out / 'mutation-exits.txt').write_text('\n'.join(summary) + '\n')
    print(summary[-1], flush=True)
