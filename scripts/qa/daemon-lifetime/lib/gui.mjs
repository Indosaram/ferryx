// GUI driver per platform. Every launch is of a QA artifact the harness built and identity-checked;
// every quit, kill or termination targets only the recorded pid / QA bundle identifier.
//   mac:     direct exec of <QA.app>/Contents/MacOS/ferryx; quit by Apple Event to the QA bundle id;
//            forceTerminate through NSRunningApplication for the QA bundle id (osascript JXA).
//   linux:   systemd-run --user --scope --unit=<qa>-gui-<tag> (Q03 kills that scope); quit via MahoRD.
//   windows: launcher.ps1 started through MahoRD Win+R; Q03 TerminateJobObject; quit via MahoRD.

import path from 'node:path';
import { assertBundleIdentifier, assertQaExecutable, qaBundleIdentifier, qaSupervisorName } from './guards.mjs';

export function macGuiExecutable(appPath) {
  return path.join(appPath, 'Contents', 'MacOS', 'ferryx');
}

/** JXA that terminates only running applications with exactly this bundle identifier. */
export function macForceTerminateScript(bundleId) {
  return [
    "ObjC.import('AppKit');",
    `const apps = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('${bundleId}');`,
    'const out = [];',
    'for (let i = 0; i < apps.count; i++) { const a = apps.objectAtIndex(i); out.push({ pid: a.processIdentifier, terminated: a.forceTerminate }); }',
    'JSON.stringify(out);',
  ].join('\n');
}

export function macQuitScript(bundleId) {
  return `tell application id "${bundleId}" to quit`;
}

export function linuxScopeUnit(runId, tag) {
  return `${qaSupervisorName(runId)}-gui-${tag}.scope`;
}

export function createGuiDriver({ host, agent, runId, layout, allowedRoots, forbidden, mahord = null, guiEnv = {} }) {
  const platform = host.platform;
  const bundleId = qaBundleIdentifier(runId);
  let launches = 0;

  async function verifiedBundle(appPath) {
    const r = await agent.call('exec', { command: '/usr/libexec/PlistBuddy', args: ['-c', 'Print :CFBundleIdentifier', path.join(appPath, 'Contents', 'Info.plist')] });
    assertBundleIdentifier(r.stdout.trim(), runId);
  }

  async function launch(artifact, { env, tag = `g${++launches}` } = {}) {
    const exe = artifact.guiExecutable;
    assertQaExecutable(exe, { allowedRoots, forbidden, platform });
    const fullEnv = { ...guiEnv, ...env };
    if (platform === 'mac') {
      await verifiedBundle(artifact.appPath);
      const entry = await agent.call('spawn', { command: exe, args: [], env: fullEnv, role: `gui-${tag}`, executable: exe, logFile: path.join(layout.logs, `gui-${tag}.log`) });
      return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime };
    }
    if (platform === 'linux') {
      const unit = linuxScopeUnit(runId, tag);
      const spawned = await agent.call('spawn', {
        command: 'systemd-run', args: ['--user', '--scope', `--unit=${unit}`, '--', exe], env: fullEnv,
        role: `gui-${tag}`, executable: exe, logFile: path.join(layout.logs, `gui-${tag}.log`), record: false,
      });
      // systemd-run --scope execs the GUI in place, so the spawned pid becomes the GUI.
      const ready = await agent.call('waitLog', { file: path.join(layout.logs, `gui-${tag}.log`), pattern: 'Running scope as unit', deadlineMs: 30_000 });
      const entry = await agent.call('record', { pid: spawned.pid, role: `gui-${tag}`, executable: exe });
      return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime, scope: unit, scopeReady: Boolean(ready.matched) };
    }
    // windows: the launcher is started on the interactive desktop through MahoRD.
    if (!mahord) throw new Error('Windows GUI launch needs MahoRD');
    const envFile = `${layout.root}\\gui-${tag}.env.json`;
    await agent.call('writeFile', { file: envFile, content: JSON.stringify(fullEnv) });
    const cmd = `powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "${layout.root}\\launcher.ps1" -Exe "${exe}" -RunDir "${layout.root}" -EnvFile "${envFile}" -Tag ${tag}`;
    await mahord.click(3000, 1580); // blank taskbar: focus the shell so Win+R reaches it
    await mahord.key('Meta+r');
    await mahord.type(cmd);
    await mahord.key('Return');
    const published = `${layout.root}\\gui-${tag}.json`;
    const ok = await agent.call('waitPath', { file: published, deadlineMs: 60_000 });
    if (!ok) throw new Error(`launcher did not publish ${published}`);
    const info = JSON.parse(await agent.call('readFile', { file: published }));
    const entry = await agent.call('record', { pid: info.pid, role: `gui-${tag}`, executable: exe });
    return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime, jobAssigned: info.jobAssigned };
  }

  /** Normal quit; resolves with the exit event of the recorded pid. */
  async function quit(gui) {
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    if (platform === 'mac') {
      await agent.call('exec', { command: '/usr/bin/osascript', args: ['-e', macQuitScript(bundleId)] });
    } else {
      if (!mahord) throw new Error('remote GUI quit needs MahoRD');
      await mahord.key(platform === 'windows' ? 'Alt+F4' : 'ctrl+q');
    }
    return exited;
  }

  /** Q02: SIGKILL / Stop-Process of the recorded GUI pid only. */
  async function kill(gui) {
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    await agent.call('kill', { pid: gui.pid, signal: 'SIGKILL' });
    return exited;
  }

  /** Q03: OS-attributed termination. */
  async function forceTerminate(gui) {
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    let detail;
    if (platform === 'mac') {
      const check = await agent.call('check', { pid: gui.pid });
      if (check !== 'match') throw new Error(`GUI pid ${gui.pid} identity is ${check}`);
      detail = await agent.call('exec', { command: '/usr/bin/osascript', args: ['-l', 'JavaScript', '-e', macForceTerminateScript(bundleId)] });
    } else if (platform === 'linux') {
      detail = await agent.call('exec', { command: 'systemctl', args: ['--user', 'kill', '--signal=SIGKILL', gui.scope] });
    } else {
      await agent.call('writeFile', { file: `${layout.root}\\gui-${gui.tag}.terminate.request`, content: '1' });
      detail = { requested: true };
    }
    return { exited: await exited, detail };
  }

  return { launch, quit, kill, forceTerminate, bundleId };
}
