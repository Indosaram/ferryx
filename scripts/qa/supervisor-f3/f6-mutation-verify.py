#!/usr/bin/env python3
"""CI-only mutation receipts: each regression must reject its disabled production check."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[3]
qa = root / 'scripts/qa/supervisor-f3'
out = root / 'f6-receipts'
out.mkdir(exist_ok=True)
# Keep receipts out of the checkout's cleanliness proof.
exclude = subprocess.check_output(['git', 'rev-parse', '--git-path', 'info/exclude'], cwd=root, text=True).strip()
with open(root / exclude, 'a') as stream:
    stream.write('\nf6-receipts/\n')
mutations = [
    ('F6-1', 'steps.mjs', 'Object.assign(lane.receipt, who);',
     'Object.assign(lane.receipt, { machine: who.machine, candidate: who.candidate });',
     ['bun', 'test', str(qa / 'provenance.test.mjs'), '--test-name-pattern', 'F6-1 real session writer']),
    ('F6-2', 'contract.mjs', 'export function remoteAgentDefect(lane, revision) {',
     'export function remoteAgentDefect(lane, revision) { return null; }\nfunction disabledRemoteAgentDefect(lane, revision) {',
     ['bun', 'test', str(qa / 'provenance.test.mjs'), '--test-name-pattern', 'F6-2']),
    ('F6-3', 'contract.mjs', "export const cleanupLeaks = (lane) => (lane?.cleanup ?? []).filter((entry) => !CLEANUP_ACTIONS.has(entry.action));",
     'export const cleanupLeaks = (lane) => [];',
     ['bun', 'test', str(qa / 'seams.test.mjs'), '--test-name-pattern', 'a failed session host, a wrong stamp or a leaked serve endpoint']),
]
summary = []
for name, filename, old, new, command in mutations:
    path = qa / filename
    original = path.read_text()
    assert original.count(old) == 1, (name, 'mutation anchor not unique')
    try:
        path.write_text(original.replace(old, new))
        red = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (out / f'{name}-RED.log').write_text(red.stdout)
        print(red.stdout, flush=True)
        assert red.returncode != 0 and 'expect(' in red.stdout, (name, 'mutation did not fail an assertion')
    finally:
        path.write_text(original)
    green = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    (out / f'{name}-GREEN.log').write_text(green.stdout)
    print(green.stdout, flush=True)
    summary.append(f'{name} RED_EXIT={red.returncode} GREEN_EXIT={green.returncode}')
    (out / 'mutation-exits.txt').write_text('\n'.join(summary) + '\n')
    assert green.returncode == 0, (name, 'restored regression not green')
print('\n'.join(summary))
