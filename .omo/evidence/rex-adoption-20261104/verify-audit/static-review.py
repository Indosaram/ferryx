#!/usr/bin/env python3
"""Read-only source assertions. Not a build, test suite, or runtime QA."""
from pathlib import Path
import json
checks = []
def check(name, ok, location, observed):
    checks.append(dict(assertion=name, passed=ok, location=location, observed=observed))
gate = Path('src-tauri/src/daemon/pane_grants.rs').read_text()
check('remote terminal writes and resizes are explicitly pane-gated',
      'DaemonRequest::RemoteWrite' in gate and 'DaemonRequest::RemoteResize' in gate,
      'src-tauri/src/daemon/pane_grants.rs:195', 'RemoteWrite/RemoteResize absent; wildcard returns None')
server = Path('src-tauri/src/daemon/server.rs').read_text()
check('unidentified process transport does not bypass mutation gates',
      'PeerOrigin::TokenAuthenticated | PeerOrigin::Unknown) && gate != Gate::OwnerOnly' not in server,
      'src-tauri/src/daemon/server.rs:6647', 'Unknown shares compatibility bypass with TokenAuthenticated')
runtime = Path('src-tauri/src/daemon/plugins/runtime.rs').read_text()
check('plugin processes are registered for socket-level classification',
      'pane_registry' in runtime or 'PaneRegistry' in runtime,
      'src-tauri/src/daemon/plugins/runtime.rs:37-49', 'spawned child pid only sent to lifecycle observer; no pane registry insertion')
hits=[]
for p in Path('src-tauri/src/daemon').rglob('*.rs'):
    for n,line in enumerate(p.read_text().splitlines(),1):
        if 'leafId' in line or 'leaf_id' in line: hits.append(f'{p}:{n}: {line.strip()}')
check('F1 literal no leafId in daemon', not hits,
      'src-tauri/src/daemon/layout_migration.rs:120', hits)
print(json.dumps({'kind':'static source review assertions, NOT runtime QA','checks':checks}, indent=2))
raise SystemExit(0 if all(c['passed'] for c in checks) else 1)
