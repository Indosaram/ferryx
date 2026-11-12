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
    name: 'maho-mac',
    // kind 'local': the lane runs on maho-mac itself; an ssh hop back to this same host times out
    // ('ssh: connect to host 100.65.239.35 port 22: Operation timed out', run 20261109t125235z),
    // exactly as it did on omarchy before that lane went local.
    transport: { kind: 'local' },
    address: '100.65.239.35',
    user: 'i552267',
    canonical: '/Users/i552267/code/ferryx',
    worktreeRoot: '/Users/i552267/code/ferryx-wt',
    cargoTarget: '/tmp/ferryx-daemon-lifetime-qa-target',
    evidenceRoot: '/Users/i552267/code/ferryx-wt/.evidence/daemon-independent-lifetime-20261017',
    runRoot: '/tmp/ferryx-qa-daemon-lifetime',
    node: '/opt/homebrew/bin/node',
    buildEnv: { ZIG: '/Users/i552267/tools/zig-aarch64-macos-0.17.0/zig' },
    gui: { kind: 'mac-local' },
    exeSuffix: '',
  }),
  linux: Object.freeze({
    platform: 'linux',
    name: 'omarchy',
    // kind 'local': the lane runs on omarchy itself; an ssh hop back to this same host timed out
    // ('ssh: connect to host omarchy port 22: Connection timed out', run 20261109t085910z).
    transport: { kind: 'local' },
    address: '100.91.254.71',
    user: 'indo',
    canonical: '/home/indo/code/ferryx',
    worktreeRoot: '/home/indo/code/ferryx-wt',
    cargoTarget: '/home/indo/code/ferryx-wt/.target-w5qa-20261019',
    // Drive the GUI through Xwayland, not the native Wayland socket. On this host Hyprland's
    // wp_linux_drm_syncobj_v1 path kills every new toplevel with
    //   wl_display.error(wp_linux_drm_syncobj_surface_v1, 4, "Missing acquire timeline")
    // which GDK reports as "Error 71 (Protocol error) dispatching to Wayland display" (measured
    // 2026-11-10 on runs 133936z/141803z/142912z and on a direct binary launch; the same binary
    // stayed up for the full 12 s timeout under DISPLAY=:0 + GDK_BACKEND=x11). Pin X11 so the
    // harness does not depend on that compositor path.
    guiEnv: {
      DISPLAY: ':0',
      XDG_RUNTIME_DIR: '/run/user/1000',
      GDK_BACKEND: 'x11',
    },
    evidenceRoot: '/home/indo/code/ferryx-wt/.evidence/daemon-independent-lifetime-20261017',
    runRoot: '/tmp/ferryx-qa-daemon-lifetime',
    // Bulk run storage (artifacts, data with the staged daemon-runtime copies, logs) goes on the home
    // filesystem: omarchy's /tmp is a 16 GiB tmpfs with usrquota, and r3b's staging copy failed there
    // with EDQUOT (os error 122). Only the runtime dir (sockets, owner.json) stays under runRoot, so
    // handover-<uuid>.sock keeps a path well inside the 108-byte sun_path limit.
    storageRoot: '/home/indo/code/ferryx-wt/.qaruns',
    node: 'node',
    // CARGO_BUILD_JOBS=1: omarchy has 31 GiB RAM and six QA builds share one target dir, each
    // rustc mapping the multi-hundred-MB libferryx_lib.rlib. Parallel rustc pushed the box into
    // 64 GiB of swap and earlyoom killed a sibling, which deadlocked the bump build on the cargo
    // jobserver (run 20261108t222710z: rustc sat in futex_do_wait with 34s CPU after 38min while
    // build-bump.log froze at 08:14). One job keeps peak RSS under the limit.
    // CARGO_BUILD_BUILD_DIR is deliberately not set: omarchy's ~/.cargo/config.toml pins
    // build.build-dir to /mnt/build/ws{workspace-root}, and this worktree's entry there is a
    // symlink to /home (set 2026-11-09 after that SSD hit 100% and failed the bump build with
    // "LLVM ERROR: IO failure on output stream: Disk quota exceeded", runs 20261109t015929z and
    // 20261109t064625z). Redirecting the variable itself would invalidate every cached absolute
    // path in the build dir and force a full rebuild.
    buildEnv: { CARGO_BUILD_JOBS: '1' },
    // The handover_v1 tree predates the ghostty Zig upgrade and pins Zig 0.16.0,
    // while master requires 0.17.0. They cannot share one ZIG value.
    v1BuildEnv: { ZIG: '/home/indo/.local/zig-0.16.0/zig' },
    gui: { kind: 'mahord', host: '100.91.254.71', pairingId: 'AF12D209-AB76-401E-BDB2-0D80E01FC2C8' },
    exeSuffix: '',
  }),
  windows: Object.freeze({
    platform: 'windows',
    name: 'maho-win',
    // kind 'local': the lane runs inside the maho-win session itself; an ssh hop back to this same
    // host looped through sshd and died with 'connect to host maho-win port 22: Connection timed out'.
    transport: { kind: 'local' },
    canonical: 'C:\\Users\\sook\\ferryx',
    worktreeRoot: 'C:\\Users\\sook\\ferryx-wt',
    cargoTarget: 'C:\\Users\\sook\\ferryx-wt\\.target-dlq1107-20261108',
    // CARGO_INCREMENTAL=0: six QA builds share one target dir, and incremental caches filled
    // maho-win's C: (r4 v1bump failed with os error 112 after A, B, C and bump had passed).
    // CARGO_BUILD_JOBS=2: six parallel rustc processes each map the multi-hundred-MB
    // libferryx_lib.rlib, and the combined commit charge exhausts the Windows pagefile
    // (E0786 / "os error 1455", paging file too small). Two jobs keep it under the limit.
    buildEnv: {
      ZIG: 'C:\\Users\\sook\\ferryx-wt\\.zig-0.17.0\\zig.exe',
      CARGO_INCREMENTAL: '0',
      CARGO_BUILD_JOBS: '1',
    },
    evidenceRoot: 'C:\\Users\\sook\\ferryx-wt\\.evidence\\daemon-independent-lifetime-20261017',
    // The handover_v1 tree predates the ghostty Zig upgrade and pins Zig 0.16.0, while master
    // requires 0.17.0, so v1bump cannot reuse buildEnv's ZIG. Without this the v1bump build fails in
    // the ghostty build script and Q08b blocks with ARTIFACT_MISSING on every Windows run
    // (run 20261112t072829z). Both toolchains already live under the worktree root on maho-win.
    v1BuildEnv: { ZIG: 'C:\\Users\\sook\\ferryx-wt\\.zig-0.16.0\\zig.exe' },
    // No '-' or '_' in the 8.3 form: the GUI launch types a short path through MahoRD, which drops
    // both on maho-win. Measured: C:\Users\sook\ferryx-wt\qaruns\<runId>\l1.cmd ->
    // C:\Users\sook\FERRYX~1\qaruns\202610~1\l1.cmd. The old .qa-daemon-lifetime was QA-DAE~1.
    runRoot: 'C:\\Users\\sook\\ferryx-wt\\qaruns',
    node: 'node',
    // The 4F8ADF2E/9929DC13 pairings are stale; A73C40A7 is the record present in the host's
    // client-pairings.json after the 2026-11-08 re-registration.
    gui: { kind: 'mahord', host: '100.68.224.49', pairingId: 'A73C40A7-38A9-4B48-B17B-C1FCE4B61133', physical: { width: 3840, height: 1600 } },
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
  const tauri = (features, salt, cwd, envOverride) => ({
    cwd,
    command: 'cargo',
    // Linux and Windows run target/debug/ferryx directly; only macOS uses the .app bundle.
    args: ['tauri', 'build', '--debug', ...(host.platform === 'mac' ? [] : ['--no-bundle']), '--features', features, '--config', config],
    env: { ...targetEnv, ...(envOverride ?? {}), FERRYX_QA_BUILD_SALT: salt },
  });
  return {
    A: tauri('local-split-qa', 'A', worktree),
    B: tauri('local-split-qa', 'B', worktree),
    C: tauri('local-split-qa', 'C', worktree),
    bump: tauri('local-split-qa,qa-protocol-bump', 'BUMP', worktree),
    v1bump: v1Worktree ? tauri('local-split-qa,qa-protocol-bump', 'V1', v1Worktree, host.v1BuildEnv) : null,
    // The pre-plan baseline is the same revision generation as v1bump, so it pins the older
    // toolchain too: without host.v1BuildEnv the ghostty build script rejects the current Zig
    // ('Incompatible Zig version 0.17.0: libghostty-vt requires Zig 0.16.0', run 175303z).
    baseline: baselineWorktree
      ? { cwd: baselineWorktree, command: 'cargo', args: ['build', '--manifest-path', 'src-tauri/Cargo.toml'], env: { ...(host.buildEnv ?? {}), ...(host.v1BuildEnv ?? {}), CARGO_TARGET_DIR: `${host.cargoTarget}-baseline` } }
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
