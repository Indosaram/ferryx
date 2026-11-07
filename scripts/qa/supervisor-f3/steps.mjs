// Step recording shared by the lanes: one step per (lane, scenario) with timings, named
// assertions and hashed raw artifacts. A step that throws is recorded as fail (blocked for a
// LaneBlocked), never retried and never dropped.
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { LANE_SCHEMA } from './contract.mjs';

export class LaneBlocked extends Error {
  constructor(code, detail) { super(`${code}: ${detail}`); this.code = code; this.detail = detail; }
}
export class StepFailed extends Error {
  constructor(code, detail) { super(`${code}: ${detail}`); this.code = code; this.detail = detail; }
}

/** A lane's receipt plus its artifact writer rooted at `<evidence>/<lane>/`. */
export function newLane(name, evidenceDir) {
  const dir = join(evidenceDir, name);
  mkdirSync(dir, { recursive: true });
  const receipt = { schema: LANE_SCHEMA, lane: name, blocked: null, machine: null, candidate: null, steps: [], cleanup: [], startedAt: new Date().toISOString() };
  const write = (kind, name, bytes) => {
    const path = join(dir, name);
    mkdirSync(dirname(path), { recursive: true });
    const buffer = Buffer.isBuffer(bytes) ? bytes : Buffer.from(typeof bytes === 'string' ? bytes : JSON.stringify(bytes, null, 2));
    writeFileSync(path, buffer);
    return { kind, path: relative(evidenceDir, path), sha256: createHash('sha256').update(buffer).digest('hex'), bytes: buffer.length };
  };
  const log = (line) => write('log', `lane-log/${Date.now()}-${Math.random().toString(16).slice(2, 8)}.txt`, `${new Date().toISOString()} ${line}\n`);
  return { name, dir, receipt, write, log };
}

/** Preserve the same complete identity in every host lane and its raw artifact. */
export function recordIdentity(lane, who) {
  Object.assign(lane.receipt, who);
  lane.write('identity', 'identity.json', who);
}

/** Runs `body(ctx)` as the step for `scenarioId`; returns the recorded step. */
export async function runStep(lane, scenarioId, body) {
  const startedAt = new Date();
  const step = { scenarioId, status: 'fail', code: null, detail: null, assertions: [], artifacts: [] };
  const ctx = {
    assert(id, ok, observed) {
      step.assertions.push({ id, ok: ok === true, observed });
      if (ok !== true) throw new StepFailed('ASSERTION_FAILED', `${id}: ${JSON.stringify(observed).slice(0, 2000)}`);
    },
    artifact(kind, name, bytes) {
      const entry = lane.write(kind, `${scenarioId}/${name}`, bytes);
      step.artifacts.push(entry);
      return entry;
    },
  };
  try {
    await body(ctx);
    step.status = 'pass';
  } catch (error) {
    step.status = error instanceof LaneBlocked ? 'blocked' : 'fail';
    step.code = error.code ?? 'UNEXPECTED_ERROR';
    step.detail = String(error.detail ?? error.stack ?? error).slice(0, 4000);
  }
  const endedAt = new Date();
  Object.assign(step, { startedAt: startedAt.toISOString(), endedAt: endedAt.toISOString(), durationMs: endedAt - startedAt });
  lane.receipt.steps.push(step);
  return step;
}

/** Steps for scenarios a lane never reached, so the validator and the report see them. */
export function markNotRun(lane, scenarioIds, reason) {
  for (const scenarioId of scenarioIds) {
    if (lane.receipt.steps.some((step) => step.scenarioId === scenarioId)) continue;
    lane.receipt.steps.push({ scenarioId, status: lane.receipt.blocked ? 'blocked' : 'not-run', code: 'NOT_REACHED', detail: reason, assertions: [], artifacts: [] });
  }
}

/**
 * Ends one run-spawned process by its exact identity through the agent that spawned it, the
 * way native-configured-adapters' host does: probe; if live, subscribe to its exit FIRST, then
 * `terminate {pid, executable, processStart}`, then await that exit. The entry proves the
 * outcome (killed / already-exited); any error is recorded as reported-not-killed, which the
 * validator fails.
 *
 * `exitProof` (darwin GUI): the agent cannot subscribe to a non-child darwin pid (its `exit`
 * falls through to Linux pidfd), so exit is proven only by the launcher's own exit receipt
 * (receipts/gui-launch.json.exit.json) naming the same pid and processStart. Without that
 * receipt nothing is recorded as gone, whatever probe says.
 */
export async function stopExact(agent, { resource, receipt, exitProof = null, deadlineMs = 60_000 }) {
  const { pid, executable, processStart } = receipt ?? {};
  if (!pid || !executable || !processStart) return { resource, pid, action: 'reported-not-killed', error: 'receipt lacks pid, executable or processStart' };
  const proven = async () => {
    const exit = await exitProof().catch(() => null);
    return exit && exit.status === 'exited' && exit.pid === pid && exit.processStart === processStart ? exit : null;
  };
  try {
    const signal = AbortSignal.timeout(deadlineMs);
    if (exitProof) {
      const early = await proven();
      if (early) return { resource, pid, processStart, action: 'already-exited', proof: 'exit-receipt', exit: early };
      const live = await agent.call('probe', { pid }, signal).catch((error) => ({ probeError: String(error?.message ?? error) }));
      if (!live.probeError && !live.exited) await agent.call('terminate', { pid, executable, processStart }, signal);
      const exit = await until(`${resource} exit receipt`, proven, Boolean, { deadlineMs, gapMs: 1_000 }).catch(() => null);
      if (!exit) return { resource, pid, processStart, action: 'reported-not-killed', error: `no exit receipt with this pid and processStart${live.probeError ? `; probe failed: ${live.probeError}` : ''}` };
      return { resource, pid, processStart, action: live.exited ? 'already-exited' : 'killed', proof: 'exit-receipt', exit };
    }
    const live = await agent.call('probe', { pid }, signal);
    if (live.exited) return { resource, pid, processStart, action: 'already-exited' };
    const exited = agent.call('exit', { pid, processStart }, signal);
    // Handled now so a terminate failure cannot leave it rejecting unobserved; awaited below.
    exited.catch(() => {});
    const terminated = await agent.call('terminate', { pid, executable, processStart }, signal);
    await exited;
    return { resource, pid, processStart, action: terminated?.exited ? 'already-exited' : 'killed' };
  } catch (error) {
    return { resource, pid, processStart, action: 'reported-not-killed', error: String(error?.message ?? error) };
  }
}

/** One-at-a-time section: `run(fn)` waits for every earlier section to settle first. */
export function mutex() {
  let tail = Promise.resolve();
  return (fn) => {
    const result = tail.then(fn);
    tail = result.catch(() => {});
    return result;
  };
}

/** A promise plus its resolver, for one lane to hold until another records a fact. */
export function latch() {
  let open;
  const promise = new Promise((resolveLatch) => { open = resolveLatch; });
  return { promise, open };
}

/**
 * Bounded wait for a product state: each try is a fresh read of the real surface or daemon.
 * The gap only paces the reads of a remote surface that offers no change subscription (a
 * MahoRD frame, a WDA source, a daemon read over SSH); the deadline decides failure.
 */
export async function until(label, read, accept, { deadlineMs, gapMs = 1_000 }) {
  const end = Date.now() + deadlineMs;
  let last;
  for (;;) {
    last = await read();
    if (accept(last)) return last;
    if (Date.now() >= end) throw new StepFailed('DEADLINE', `${label} not observed within ${deadlineMs} ms; last: ${JSON.stringify(last).slice(0, 1500)}`);
    await new Promise((resolve) => { setTimeout(resolve, gapMs); });
  }
}
