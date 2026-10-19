// Per-host fixtures for the daemon-lifetime QA harness (plan todo 12).
//
// run.mjs runs on this Mac and drives each target through one host agent
// (scripts/qa/daemon-lifetime/host-agent.mjs) started locally (mac) or over SSH (omarchy,
// maho-win). Shell and builds go over SSH; GUI actions on remote hosts go only through MahoRD
// (maho-client --host <ip> --pairing-id <id> --mcp). The user's own MacBook desktop is never
// driven: on mac the harness only launches, quits and terminates the QA bundle it built.

export const HOSTS = Object.freeze({
  mac: Object.freeze({
    platform: 'mac',
    name: 'this-mac',
    transport: { kind: 'local' },
    canonical: '/Volumes/T9-Mac/project/ferryx',
    worktreeRoot: '/Volumes/T9-Mac/project/ferryx-wt',
    cargoTarget: '/tmp/ferryx-daemon-lifetime-qa-target',
    evidenceRoot: '/Volumes/T9-Mac/project/ferryx-wt/.evidence/daemon-independent-lifetime-20261017',
    runRoot: '/tmp/ferryx-qa-daemon-lifetime',
    node: 'node',
    gui: { kind: 'mac-local' },
    exeSuffix: '',
  }),
  linux: Object.freeze({
    platform: 'linux',
    name: 'omarchy',
    transport: { kind: 'ssh', target: 'omarchy', address: '100.91.254.71', user: 'indo' },
    canonical: '/home/indo/code/ferryx',
    worktreeRoot: '/home/indo/code/ferryx-wt',
    cargoTarget: '/home/indo/code/ferryx-wt/.target-w5qa-20261019',
    guiEnv: { WAYLAND_DISPLAY: 'wayland-1', DISPLAY: ':0', XDG_RUNTIME_DIR: '/run/user/1000' },
    evidenceRoot: '/home/indo/code/ferryx-wt/.evidence/daemon-independent-lifetime-20261017',
    runRoot: '/tmp/ferryx-qa-daemon-lifetime',
    node: 'node',
    gui: { kind: 'mahord', host: '100.91.254.71', pairingId: 'AF12D209-AB76-401E-BDB2-0D80E01FC2C8' },
    exeSuffix: '',
  }),
  windows: Object.freeze({
    platform: 'windows',
    name: 'maho-win',
    transport: { kind: 'ssh', target: 'maho-win', address: '100.68.224.49', user: 'sook', shell: 'cmd' }, // sshd DefaultShell is cmd.exe (measured 2026-10-19)
    canonical: 'C:\\Users\\sook\\ferryx',
    worktreeRoot: 'C:\\Users\\sook\\ferryx-wt',
    cargoTarget: 'C:\\Users\\sook\\ferryx-wt\\.target-w5qa-20261019',
    buildEnv: { ZIG: 'C:\\Users\\sook\\ferryx-wt\\.zig-0.16.0\\zig.exe' },
    evidenceRoot: 'C:\\Users\\sook\\ferryx-wt\\.evidence\\daemon-independent-lifetime-20261017',
    runRoot: 'C:\\Users\\sook\\ferryx-wt\\.qa-daemon-lifetime',
    node: 'node',
    // The 4F8ADF2E pairing is stale; 9929DC13 is the one that completes a handshake (2026-10-15).
    gui: { kind: 'mahord', host: '100.68.224.49', pairingId: '9929DC13-3841-4182-A399-9E1323F961A2', physical: { width: 3840, height: 1600 } },
    exeSuffix: '.exe',
  }),
});

export function hostFor(platform, overrides = {}) {
  const base = HOSTS[platform];
  if (!base) throw new Error(`unknown platform ${platform}; expected mac|linux|windows`);
  return { ...base, ...overrides, transport: { ...base.transport, ...(overrides.transport ?? {}) }, gui: { ...base.gui, ...(overrides.gui ?? {}) } };
}

/** Worktree slug and paths the plan's AGENTS.md rule requires on every host. */
export function worktreePaths(host, slug) {
  const sep = host.platform === 'windows' ? '\\' : '/';
  return {
    worktree: `${host.worktreeRoot}${sep}${slug}`,
    evidence: `${host.evidenceRoot}${sep}q12-${host.platform}`,
  };
}

/**
 * The artifacts todo 12 needs, each a separate build of the same command:
 *  A        normal QA debug bundle (salt A)
 *  B, C     same version, different FERRYX_QA_BUILD_SALT, so generation keys differ (Q07, Q08)
 *  bump     --features local-split-qa,qa-protocol-bump (protocol 6) for Q09
 *  v1bump   handover_v1_rev worktree, same bump features, for Q08b old owner
 *  baseline pre-plan baseline rev, plain cargo build daemon only, for Q13
 */
export function buildPlan({ host, runDir, worktree, v1Worktree, baselineWorktree }) {
  const targetEnv = { ...(host.buildEnv ?? {}), CARGO_TARGET_DIR: host.cargoTarget };
  const config = host.platform === 'windows' ? `${runDir}\\tauri.qa.json` : `${runDir}/tauri.qa.json`;
  const tauri = (features, salt, cwd) => ({
    cwd,
    command: 'cargo',
    // Linux and Windows run target/debug/ferryx directly; only macOS uses the .app bundle.
    args: ['tauri', 'build', '--debug', ...(host.platform === 'mac' ? [] : ['--no-bundle']), '--features', features, '--config', config],
    env: { ...targetEnv, FERRYX_QA_BUILD_SALT: salt },
  });
  return {
    A: tauri('local-split-qa', 'A', worktree),
    B: tauri('local-split-qa', 'B', worktree),
    C: tauri('local-split-qa', 'C', worktree),
    bump: tauri('local-split-qa,qa-protocol-bump', 'BUMP', worktree),
    v1bump: v1Worktree ? tauri('local-split-qa,qa-protocol-bump', 'V1', v1Worktree) : null,
    baseline: baselineWorktree
      ? { cwd: baselineWorktree, command: 'cargo', args: ['build', '--manifest-path', 'src-tauri/Cargo.toml'], env: { ...(host.buildEnv ?? {}), CARGO_TARGET_DIR: `${host.cargoTarget}-baseline` } }
      : null,
  };
}

/** Refuses any build that is not the plan's isolated QA debug build. */
export function assertQaBuild(step) {
  const args = step.args.join(' ');
  if (step.args[0] === 'tauri') {
    if (!step.args.includes('--debug')) throw new Error('QA bundles are debug builds only');
    if (!/--features [^ ]*local-split-qa/.test(args)) throw new Error('QA bundles need local-split-qa');
    if (!step.args.includes('--config')) throw new Error('QA bundles need the tauri.qa.json overlay');
  } else if (step.args[0] === 'build') {
    if (step.args.includes('--release') || step.args.includes('-r')) throw new Error('release builds are forbidden');
  } else {
    throw new Error(`unexpected cargo step ${args}`);
  }
  if (!step.env.CARGO_TARGET_DIR) throw new Error('QA builds use a private CARGO_TARGET_DIR');
  return step;
}
