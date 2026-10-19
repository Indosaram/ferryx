// OS-job queries and QA-scoped job actions, all through the host agent (which runs the guard).

import { parseLaunchctlPrint, launchctlProgram, parseSystemctlShow, windowsTaskTree, parseScheduledTaskState } from './os-jobs.mjs';
import { qaSupervisorName } from './guards.mjs';

export function createJobs({ host, agent, runId, layout, uid = null }) {
  const platform = host.platform;
  const base = qaSupervisorName(runId);

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

  /** Windows: the watchdog (--daemon-supervisor) running the staged executable and its daemon child. */
  async function windowsTree(stagedRoot) {
    const rows = await agent.call('table');
    const under = rows.filter((r) => r.executable && r.executable.toLowerCase().startsWith(stagedRoot.toLowerCase()));
    const watchdog = under.find((r) => /--daemon-supervisor/.test(r.commandLine));
    if (!watchdog) return { watchdog: null, daemon: null, rows: under };
    return { ...windowsTaskTree(under, { watchdogPid: watchdog.pid }), rows: under };
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
    <string>${layout.logs}/legacy.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>${layout.logs}/legacy.stderr.log</string>
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
      await agent.call('exec', { command: 'install', args: ['-D', '-m', '0644', '/dev/stdin', file], input: `[Unit]\nDescription=Ferryx QA legacy daemon ${runId}\n\n[Service]\nType=simple\nExecStart=${executable} --daemon\nKillMode=process\nRestart=on-failure\n${envLines}\n\n[Install]\nWantedBy=default.target\n` });
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'daemon-reload'] });
      await agent.call('exec', { command: 'systemctl', args: ['--user', 'enable', `${base}.service`] });
      return agent.call('exec', { command: 'systemctl', args: ['--user', 'start', `${base}.service`] });
    }
    const args = ['--daemon', ...Object.entries(env).flatMap(([k, v]) => ['--env', `${k}=${v}`])];
    void args; // the pre-plan baseline daemon has no watchdog --env forwarding; run it via cmd with env
    const cmd = `cmd /c "${Object.entries(env).map(([k, v]) => `set ${k}=${v}&&`).join('')} "${executable}" --daemon"`;
    await agent.call('exec', { command: 'schtasks', args: ['/Create', '/TN', task(null), '/SC', 'ONLOGON', '/TR', cmd, '/F'] });
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
        await agent.call('exec', { command: 'sh', args: ['-c', 'rm -f -- "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$1"', 'sh', unitName] });
      }
      return true;
    }
    await agent.call('exec', { command: 'schtasks', args: ['/End', '/TN', name] });
    await agent.call('exec', { command: 'schtasks', args: ['/Delete', '/TN', name, '/F'] });
    return true;
  }

  return { inspect, windowsTree, listQaJobs, start, installLegacy, remove, jobName, label, unit, task, base };
}
