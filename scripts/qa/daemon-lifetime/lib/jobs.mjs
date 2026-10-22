// OS-job queries and QA-scoped job actions, all through the host agent (which runs the guard).

import { parseLaunchctlPrint, launchctlProgram, parseSystemctlShow, selectWindowsTree, parseScheduledTaskState } from './os-jobs.mjs';
import { qaSupervisorName } from './guards.mjs';
import { hjoin } from './hostpath.mjs';

/** stdout/stderr of the Q13 legacy daemon job, on every platform (the baseline prints FERRYX_DAEMON_READY to stdout). */
export function legacyLogFiles(layout) {
  return { stdout: hjoin(layout.logs, 'legacy.stdout.log'), stderr: hjoin(layout.logs, 'legacy.stderr.log') };
}

export function createJobs({ host, agent, runId, layout, uid = null }) {
  const platform = host.platform;
  const base = qaSupervisorName(runId);
  const legacyLogs = legacyLogFiles(layout);

  const label = (key) => (key ? `${base}.${key}` : base);
  const unit = (key) => (key ? `${base}-${key}` : base);
  const task = (key) => (key ? `Ferryx\\${base}-${key}` : `Ferryx\\${base}`);
  const jobName = (key) => (platform === 'mac' ? label(key) : platform === 'linux' ? unit(key) : task(key));

  /** { state, pid, program?, raw } for one generation (key) or the legacy QA job (key null). */
  async function inspect(key) {
    if (platform === 'mac') {
      const r = await agent.call('exec', { command: 'launchctl', args: ['print', `gui/${uid}/${label(key)}`] });
      return { ...parseLaunchctlPrint(r), program: launchctlProgram(r.stdout), raw: r };
    }
    if (platform === 'linux') {
      const r = await agent.call('exec', { command: 'systemctl', args: ['--user', 'show', '-p', 'MainPID', '-p', 'ActiveState', '-p', 'UnitFileState', '-p', 'ExecMainStartTimestampMonotonic', `${unit(key)}.service`] });
      const s = parseSystemctlShow(r.stdout);
      return { state: s.activeState === 'active' ? 'Running' : s.activeState ?? 'Unverifiable', pid: s.pid, unitFileState: s.unitFileState, raw: r };
    }
    const q = await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-Command',
      `$t = Get-ScheduledTask -TaskPath '\\Ferryx\\' -TaskName '${task(key).split('\\')[1]}' -ErrorAction SilentlyContinue; if ($t) { [ordered]@{ TaskName = $t.TaskName; State = [string]$t.State } | ConvertTo-Json -Compress }`] });
    const st = parseScheduledTaskState(q.stdout);
    return { state: st.state, pid: null, raw: q };
  }

  /**
   * Windows: the watchdog (--daemon-supervisor) running the staged executable and its daemon child.
   * With ownerPid, the tree is the one whose daemon is that owner (selectWindowsTree).
   */
  async function windowsTree(stagedRoot, { ownerPid = null } = {}) {
    const rows = await agent.call('table');
    const under = rows.filter((r) => r.executable && r.executable.toLowerCase().startsWith(stagedRoot.toLowerCase()));
    return { ...selectWindowsTree(under, { ownerPid }), rows: under };
  }

  /** Lists QA-named jobs of this run (for teardown and leftover checks). */
  async function listQaJobs() {
    if (platform === 'mac') {
      const r = await agent.call('exec', { command: 'launchctl', args: ['list'] });
      return r.stdout.split('\n').map((l) => l.trim().split(/\s+/).pop()).filter((n) => n && (n === base || n.startsWith(`${base}.`)));
    }
    if (platform === 'linux') {
      const r = await agent.call('exec', { command: 'systemctl', args: ['--user', 'list-units', '--all', '--plain', '--no-legend', '--no-pager'] });
      const units = r.stdout.split('\n').map((l) => l.trim().split(/\s+/)[0]).filter((n) => n && n.startsWith(base));
      const files = await agent.call('exec', { command: 'systemctl', args: ['--user', 'list-unit-files', '--plain', '--no-legend', '--no-pager'] });
      for (const l of files.stdout.split('\n')) { const n = l.trim().split(/\s+/)[0]; if (n && n.startsWith(base) && !units.includes(n)) units.push(n); }
      return units;
    }
    const r = await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-Command',
      `Get-ScheduledTask -TaskPath '\\Ferryx\\' -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -like '${base}*' } | ForEach-Object { 'Ferryx\\' + $_.TaskName }`] });
    return r.stdout.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  }

  /** Explicit QA job start (Q04b, Q10b). Plain kickstart; never -k. */
  async function start(key) {
    if (platform === 'mac') return agent.call('exec', { command: 'launchctl', args: ['kickstart', `gui/${uid}/${label(key)}`] });
    if (platform === 'linux') return agent.call('exec', { command: 'systemctl', args: ['--user', 'start', '--no-block', `${unit(key)}.service`] });
    return agent.call('exec', { command: 'schtasks', args: ['/Run', '/TN', task(key)] });
  }

  /** Registers the QA legacy single job (Q13) running the baseline daemon binary. */
  async function installLegacy({ executable, env }) {
    if (platform === 'mac') {
      const plist = `${layout.launchAgents}/${base}.plist`;
      const envXml = Object.entries(env).map(([k, v]) => `        <key>${k}</key>\n        <string>${v}</string>`).join('\n');
      const content = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${base}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${executable}</string>
        <string>--daemon</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
${envXml}
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>AbandonProcessGroup</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${legacyLogs.stdout}</string>
    <key>StandardErrorPath</key>
    <string>${legacyLogs.stderr}</string>
</dict>
</plist>
`;
      await agent.call('writeFile', { file: plist, content });
      return agent.call('exec', { command: 'launchctl', args: ['bootstrap', `gui/${uid}`, plist] });
    }
    if (platform === 'linux') {
      const home = (await agent.call('exec', { command: 'sh', args: ['-c', 'printf %s "$HOME"'] })).stdout;
      const file = `${home}/.config/systemd/user/${base}.service`;
      const envLines = Object.entries(env).map(([k, v]) => `Environment="${k}=${v}"`).join('\n');
      // Not 'install -D /dev/stdin': the agent's stdin is a socket, and opening /dev/stdin on a
      // socket fails with ENXIO (measured on omarchy), so the unit file was never written.
      // Same capture as the mac plist: a baseline daemon crash leaves its stderr in the run's logs.
      const output = `StandardOutput=append:${legacyLogs.stdout}\nStandardError=append:${legacyLogs.stderr}`;
      const written = await agent.call('exec', { command: 'sh', args: ['-c', 'umask 022; mkdir -p "$(dirname "$1")" && cat > "$1"', 'sh', file], input: `[Unit]\nDescription=Ferryx QA legacy daemon ${runId}\n\n[Service]\nType=simple\nExecStart=${executable} --daemon\nKillMode=process\nRestart=on-failure\n${envLines}\n${output}\n\n[Install]\nWantedBy=default.target\n` });
      if (written.status !== 0) return written;
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'daemon-reload'] });
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'enable', `${base}.service`] });
      return agent.call('exec', { command: 'systemctl', args: ['--user', 'start', `${base}.service`] });
    }
    const args = ['--daemon', ...Object.entries(env).flatMap(([k, v]) => ['--env', `${k}=${v}`])];
    void args; // the pre-plan baseline daemon has no watchdog --env forwarding; run it via cmd with env
    // schtasks /TR is limited to 261 characters, and inline `set` chains with three run-dir paths
    // plus the baseline exe exceed it. A launcher script in the run dir keeps /TR short.
    const launcher = `${layout.root}\\legacy-daemon.cmd`;
    const script = ['@echo off', ...Object.entries(env).map(([k, v]) => `set "${k}=${v}"`), `"${executable}" --daemon 1>>"${legacyLogs.stdout}" 2>>"${legacyLogs.stderr}"`, ''].join('\r\n');
    await agent.call('writeFile', { file: launcher, content: script });
    // The legacy task's recorded definition, as the baseline (493fae45 supervisor_windows.rs
    // ensure_with) wrote it before every /Create: <LOCALAPPDATA>\Ferryx\supervisor\<name>.xml in
    // UTF-16. HEAD migrates the legacy task only by that record (migrate_legacy_task_with), so a
    // bare /SC ONLOGON task was never migrated (run-int2-all Q13).
    const user = (await agent.call('exec', { command: 'cmd.exe', args: ['/d', '/c', 'echo %USERDOMAIN%\\%USERNAME%'] })).stdout.trim();
    const xmlFile = `${layout.root}\\legacy-task.xml`;
    await agent.call('writeFile', { file: xmlFile, content: legacyTaskXml(user, launcher) });
    const recordName = `${task(null).replace('\\', '-')}.xml`;
    const ps = `${layout.root}\\legacy-task-record.ps1`;
    await agent.call('writeFile', { file: ps, content: [
      "$ErrorActionPreference = 'Stop'",
      "$dir = Join-Path $env:LOCALAPPDATA 'Ferryx\\supervisor'",
      'New-Item -ItemType Directory -Force -Path $dir | Out-Null',
      `$out = Join-Path $dir '${recordName}'`,
      `[IO.File]::WriteAllText($out, [IO.File]::ReadAllText('${xmlFile}'), [Text.Encoding]::Unicode)`,
      'Write-Output $out', ''].join('\r\n') });
    const recorded = await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', ps] });
    if (recorded.status !== 0) return recorded;
    const created = await agent.call('exec', { command: 'schtasks', args: ['/Create', '/TN', task(null), '/XML', recorded.stdout.trim(), '/F'] });
    if (created.status !== 0) return created;
    return agent.call('exec', { command: 'schtasks', args: ['/Run', '/TN', task(null)] });
  }

  /** Teardown of one QA job of this run. Called only after its owner pid was killed or exited. */
  async function remove(name) {
    if (platform === 'mac') {
      await agent.call('exec', { command: 'launchctl', args: ['bootout', `gui/${uid}/${name}`] });
      await agent.call('remove', { target: `${layout.launchAgents}/${name}.plist` });
      return true;
    }
    if (platform === 'linux') {
      const unitName = name.endsWith('.service') || name.endsWith('.scope') ? name : `${name}.service`;
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'disable', unitName] });
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'stop', unitName] });
      // The product writes the generation unit file into the user's real unit dir (Linux keeps the
      // real HOME, guards.mjs isolationEnv). disable/stop leave that file behind, so list-unit-files
      // still reports the unit after teardown. Remove the QA-named file itself; it is allowed as a
      // QA root because its name carries this run id.
      if (unitName.endsWith('.service')) {
        if (!unitName.startsWith(`${base}-`) && unitName !== `${base}.service`) throw new Error(`refusing to delete non-QA unit file ${unitName}`);
        // The QA env prefix drop-in (installQaEnvDropIn) carries the run id in its name too.
        await agent.call('exec', { command: 'sh', args: ['-c', 'd="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"; rm -f -- "$d/$1"; rm -rf -- "$d/$2"', 'sh', unitName, `${base}-.service.d`] });
      }
      return true;
    }
    await agent.call('exec', { command: 'schtasks', args: ['/End', '/TN', name] });
    await agent.call('exec', { command: 'schtasks', args: ['/Delete', '/TN', name, '/F'] });
    // The legacy definition record installLegacy wrote, if the product left it behind.
    if (name === task(null)) {
      await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-Command', `Remove-Item -LiteralPath (Join-Path $env:LOCALAPPDATA 'Ferryx\supervisor\${name.replace('\\', '-')}.xml') -ErrorAction SilentlyContinue`] });
    }
    return true;
  }

  /**
   * Linux: a systemd prefix drop-in (`<base>-.service.d`, applied to every `<base>-*.service`)
   * giving each generation unit of this run its QA-only variables. The
   * handover_v1_rev build (24cbfd25) renders its units without them (its forwarded_env predates
   * d9dd5334's QA_NAME_KEYS), so its daemon runs isolated without a QA supervisor name,
   * supervisor::supported() is false, and RequestUpgrade answers UPGRADE_REFUSED ("No daemon
   * supervisor is supported in this runtime"). HEAD forwards the same variables itself.
   */
  async function installQaEnvDropIn(_key, env) {
    if (platform !== 'linux') throw new Error('QA env drop-ins are a Linux unit feature');
    const name = `${base}-.service`;
    return agent.call('exec', {
      command: 'sh',
      args: ['-c', 'umask 022; d="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$1.d"; mkdir -p "$d" && cat > "$d/qa-env.conf"', 'sh', name],
      input: qaEnvDropIn(env),
    });
  }

  return { inspect, windowsTree, listQaJobs, start, installLegacy, installQaEnvDropIn, remove, jobName, label, unit, task, base };
}

/** The baseline task definition (493fae45 render_task_xml), launching the QA legacy launcher. */
export function legacyTaskXml(user, command) {
  const esc = (t) => String(t).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  return `<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Keeps the Ferryx terminal daemon running for this user and restarts it after a crash.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>${esc(user)}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>${esc(user)}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>${esc(command)}</Command>
    </Exec>
  </Actions>
</Task>
`;
}

/** QA-only variables a QA-named unit needs (HEAD supervisor_linux QA_NAME_KEYS + QA_BARRIER_KEYS). */
export const QA_UNIT_ENV_KEYS = ['FERRYX_DAEMON_SUPERVISOR_NAME', 'FERRYX_QA_BARRIER_DIR', 'FERRYX_QA_RUN_ID', 'FERRYX_QA_SSH_KNOWN_HOSTS'];

export function qaEnvDropIn(env) {
  const lines = QA_UNIT_ENV_KEYS.filter((k) => env[k]).map((k) => `Environment="${k}=${env[k]}"`);
  if (!lines.some((l) => l.startsWith('Environment="FERRYX_DAEMON_SUPERVISOR_NAME='))) throw new Error('QA env drop-in needs FERRYX_DAEMON_SUPERVISOR_NAME');
  return `[Service]\n${lines.join('\n')}\n`;
}
