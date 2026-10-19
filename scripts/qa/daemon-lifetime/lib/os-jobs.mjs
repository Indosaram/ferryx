// Pure parsers for OS job state (Q00 and every job-pid assertion).

/** launchctl print gui/<uid>/<label>: NotRegistered | Running(pid) | RegisteredIdle | Unverifiable. */
export function parseLaunchctlPrint({ status, stdout = '', stderr = '' }) {
  const text = `${stdout}\n${stderr}`;
  if (status !== 0) {
    return /Could not find service/i.test(text) ? { state: 'NotRegistered', pid: null } : { state: 'Unverifiable', pid: null, detail: text.trim() };
  }
  const pid = /^\s*pid = (\d+)\s*$/m.exec(stdout);
  if (pid) return { state: 'Running', pid: Number(pid[1]) };
  if (/^\s*state = /m.test(stdout)) return { state: 'RegisteredIdle', pid: null };
  return { state: 'Unverifiable', pid: null, detail: stdout.trim() };
}

/** Program path from launchctl print (the "program = " line), used to prove the staged helper runs. */
export function launchctlProgram(stdout = '') {
  const m = /^\s*program = (.+)$/m.exec(stdout);
  return m ? m[1].trim() : null;
}

/** systemctl --user show -p MainPID -p ActiveState -p ExecMainStartTimestampMonotonic <unit>. */
export function parseSystemctlShow(stdout = '') {
  const props = {};
  for (const line of String(stdout).split(/\r?\n/)) {
    const i = line.indexOf('=');
    if (i > 0) props[line.slice(0, i)] = line.slice(i + 1);
  }
  const pid = Number(props.MainPID ?? 0);
  return { activeState: props.ActiveState ?? null, pid: pid > 0 ? pid : null, unitFileState: props.UnitFileState ?? null, props };
}

/**
 * Windows: the watchdog task's pid and its daemon child, from a CIM process table filtered by the
 * QA executable path. rows: [{ pid, ppid, executable, commandLine }].
 */
export function windowsTaskTree(rows, { watchdogPid }) {
  const watchdog = rows.find((r) => r.pid === watchdogPid) ?? null;
  const children = rows.filter((r) => r.ppid === watchdogPid);
  const daemon = children.find((r) => /--daemon(\s|$)/.test(r.commandLine ?? '') && !/--daemon-supervisor/.test(r.commandLine ?? '')) ?? null;
  return { watchdog, daemon };
}

/** Get-ScheduledTask output (ConvertTo-Json of TaskName, TaskPath, State). Empty/garbled = unknown. */
export function parseScheduledTaskState(stdout = '') {
  const text = String(stdout).trim();
  if (!text) return { state: 'Unknown', reason: 'empty' };
  try {
    const value = JSON.parse(text);
    const row = Array.isArray(value) ? value[0] : value;
    const state = row?.State;
    const names = { 0: 'Unknown', 1: 'Disabled', 2: 'Queued', 3: 'Ready', 4: 'Running' };
    const named = typeof state === 'number' ? names[state] : state;
    return named ? { state: String(named) } : { state: 'Unknown', reason: 'no State field' };
  } catch {
    return { state: 'Unknown', reason: 'not json' };
  }
}

/** Mutating launchctl/systemctl/schtasks lines in a daemon or harness log (Q04/Q05 zero-mutation proof). */
export function supervisorMutations(lines) {
  const re = /\b(launchctl\s+(bootstrap|bootout|kickstart|unload|load)|systemctl\s+--user\s+(start|stop|restart|enable|disable)|schtasks\s+\/(Run|End|Delete|Create))\b/i;
  return lines.filter((l) => re.test(l));
}
