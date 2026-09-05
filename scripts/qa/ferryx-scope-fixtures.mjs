import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, realpathSync, lstatSync, rmSync } from 'node:fs';
import { join, dirname, basename, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

export const scenarios = ['foundation', 'design', 'mobile', 'push'];
export const gates = {
  foundation: ['waiting-independent-focus', 'exact-pane-navigation', 'closed-target-rejected', 'local-unchanged', 'ssh-jailed-cwd', 'bridge-reconnect-same-pid-epoch-replay', 'older-history-exact-resume-dedupe'],
  design: ['native-child-element-area-pixels', 'dom-css-preview', 'overlay-absent', 'cancel-no-send', 'frozen-target-once', 'remote-agent-image-marker', 'navigation-resize-iframe-errors'],
  mobile: ['api-cli-lifecycle', 'physical-phone-desktop-closed', 'two-host-collision-safe', 'exclusive-handoff', 'real-approval-question-interrupt', 'two-client-winner', 'draft-files-ime'],
  push: ['trusted-https', 'physical-device-background-waiting', 'physical-device-provider-complete', 'exact-task-return', 'dedupe-expired-deny-unsubscribe-revoke-gone'],
};

export function validateRoot(input) {
  if (typeof input !== 'string' || !input.startsWith('/')) throw new Error('UNSAFE_ROOT');
  const root = resolve(input);
  const parent = realpathSync('/tmp');
  if (dirname(root) !== parent || !/^ferryx-qa\.[A-Za-z0-9]+$/.test(basename(root))) throw new Error('UNSAFE_ROOT');
  const stat = lstatSync(root);
  if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o077) || stat.uid !== process.getuid()) throw new Error('UNSAFE_ROOT');
  const path = join(root, 'manifest.json');
  const marker = lstatSync(path);
  if (!marker.isFile() || marker.isSymbolicLink() || (marker.mode & 0o077)) throw new Error('UNSAFE_MANIFEST');
  const manifest = JSON.parse(readFileSync(path, 'utf8'));
  if (manifest.schema !== 1 || manifest.root !== root || manifest.owner !== 'ferryx-scope-qa' || !/^[a-f0-9-]{36}$/.test(manifest.id)) throw new Error('UNSAFE_MANIFEST');
  for (const name of ['runtime', 'session', 'data', 'home', 'repos', 'artifacts']) {
    const child = join(root, name);
    if (realpathSync(child) !== child || !lstatSync(child).isDirectory() || (lstatSync(child).mode & 0o077)) throw new Error('UNSAFE_CHILD');
  }
  return manifest;
}

export function prepare() {
  const root = mkdtempSync(join(realpathSync('/tmp'), 'ferryx-qa.'));
  try {
    for (const name of ['runtime', 'session', 'data', 'home', 'repos', 'artifacts']) mkdirSync(join(root, name), { mode: 0o700 });
    for (const name of ['qa-one', 'qa-two']) {
      mkdirSync(join(root, 'repos', name), { mode: 0o700 });
      writeFileSync(join(root, 'repos', name, 'sentinel.txt'), `FERRYX_QA_${name}\n`, { mode: 0o600 });
    }
    const manifest = { schema: 1, owner: 'ferryx-scope-qa', id: randomUUID(), root, repositories: 'seeded-not-committed', integration: null };
    writeFileSync(join(root, 'manifest.json'), JSON.stringify(manifest, null, 2), { mode: 0o600, flag: 'wx' });
    const env = { HOME: join(root, 'home'), CODEX_HOME: join(root, 'home', 'codex'), CLAUDE_CONFIG_DIR: join(root, 'home', 'claude'), FERRYX_RUNTIME_DIR: join(root, 'runtime'), FERRYX_SESSION_DIR: join(root, 'session'), FERRYX_DATA_DIR: join(root, 'data'), FERRYX_AGENT_STATE_SOCKET: join(root, 'runtime', 'agent.sock') };
    writeFileSync(join(root, 'env.json'), JSON.stringify(env, null, 2), { mode: 0o600, flag: 'wx' });
    return manifest;
  } catch (error) {
    rmSync(root, { recursive: true });
    throw error;
  }
}

export function cleanup(root) {
  const manifest = validateRoot(root);
  // Never adopt PIDs, sockets, or externally supplied cleanup commands.
  rmSync(root, { recursive: true });
  return { event: 'cleanup', id: manifest.id, root, removed: true, processesSignalled: [], leadSshTouched: false };
}

export function validateEvidence(scenario, evidence) {
  if (!scenarios.includes(scenario)) throw new Error('INVALID_SCENARIO');
  if (scenario === 'push' && (!evidence.device?.physical || !evidence.device?.backgroundReceipt)) throw new Error('DEVICE_PUSH_EVIDENCE_REQUIRED');
  // Imported assertions cannot establish causality. Only an integrated driver may
  // generate pass observations; that adapter is intentionally not fabricated here.
  throw new Error(`PRODUCT_ADAPTER_UNAVAILABLE:${gates[scenario].join(',')}`);
}
