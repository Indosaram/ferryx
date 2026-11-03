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
