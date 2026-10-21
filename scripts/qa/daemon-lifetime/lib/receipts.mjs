// todo 8 QA receipts the harness consumes, named exactly as daemon/qa_producers.rs writes them.
// Every line of <barriers>/<name>.receipt.jsonl carries runId, operationId, producerPid and the
// settlement fields of its producer.

import path from 'node:path';

/** qa_producers.rs AUTHORITY_STAGE: one line per handover state transition (note_authority_stage). */
export const AUTHORITY_STAGE = 'handover-authority';
/** The arm a run writes so the daemon's channel has an operation id for the authority receipts. */
export const AUTHORITY_ARM = 'handover-authority';
/** qa_producers.rs HANDOVER_FAULT_REQUEST: { runId, operationId, fault } for the next handover. */
export const HANDOVER_FAULT_REQUEST = 'handover-fault.request.json';
/** qa_producers.rs HANDOVER_TRANSFER / ROLLBACK_RELINQUISHMENT receipt names. */
export const HANDOVER_TRANSFER = 'handover-transfer';
export const ROLLBACK_RELINQUISHMENT = 'rollback-relinquishment';

/** Every stage name server.rs passes to note_authority_stage. */
export const CANDIDATE_STAGES = Object.freeze(['candidate-admitted', 'sessions-received', 'authority-validated', 'commit-sent', 'authority-adopted', 'authority-recovered', 'candidate-aborted']);
export const PREDECESSOR_STAGES = Object.freeze(['predecessor-prepared', 'predecessor-committed', 'predecessor-aborted']);
/** Candidate stages sampled before CommitHandoverOk: the candidate must have accepted nothing yet. */
export const PRE_COMMIT_STAGES = Object.freeze(['candidate-admitted', 'sessions-received', 'authority-validated', 'commit-sent']);
/** The three inherited listeners whose accept counters each stage reports (AuthorityListenerRole). */
export const LISTENER_ROLES = Object.freeze(['canonical', 'control', 'agentState']);

export function receiptFile(layout, name) {
  return path.join(layout.barriers, `${name}.receipt.jsonl`);
}

export function parseReceipts(text, runId) {
  return String(text ?? '')
    .split(/\r?\n/)
    .filter((line) => line.trim())
    .map((line) => { try { return JSON.parse(line); } catch { return null; } })
    .filter((r) => r && r.runId === runId);
}

/**
 * The authority stages appended after `from` lines, in order. A predecessor that installed its
 * channel before the runner armed this upgrade may carry an older operation id, so a step is
 * scoped by position in the append-only receipt file, not by operation id.
 */
export function authorityStages(text, runId, from = 0) {
  return parseReceipts(String(text ?? '').split(/\r?\n/).filter((line) => line.trim()).slice(from).join('\n'), runId)
    .filter((r) => typeof r.stage === 'string');
}

export function receiptCount(text) {
  return String(text ?? '').split(/\r?\n/).filter((line) => line.trim()).length;
}

/**
 * client.rs DAEMON_CLIENT_BUSY_RECEIPT (e4bc115f): one line per DAEMON_BUSY a QA client receives,
 * correlated to the armed daemon-listener-stall operation.
 */
export const DAEMON_CLIENT_BUSY = 'daemon-client-busy';

/** waitLog pattern for a DAEMON_BUSY line of this run and operation (base keys precede the settlement). */
export function busyReceiptPattern(runId, operationId) {
  return `"runId":"${runId}","operationId":"${operationId}",[^\\n]*"code":"DAEMON_BUSY"`;
}

/** The pid of an `OwnerState::Alive { pid, .. }` Debug string, else null. */
export function aliveOwnerPid(ownerState) {
  const m = /^Alive \{ pid: (\d+),/.exec(String(ownerState ?? ''));
  return m ? Number(m[1]) : null;
}

/**
 * The run's DAEMON_BUSY receipts for one operation, and the one (if any) that reports a silent
 * handshake to the Alive owner whose pid the runner recorded before the stall.
 */
export function busyVerdict(text, { runId, operationId, pidBefore }) {
  const busy = parseReceipts(text, runId).filter((r) => r.operationId === operationId && r.code === 'DAEMON_BUSY');
  const match = busy.find((r) => r.details?.type === 'daemonOwnerBusy' && r.details?.cause === 'handshakeSilent' && pidBefore != null && aliveOwnerPid(r.details?.ownerState) === pidBefore);
  return { correlated: busy.length, match: match ?? null, receipts: busy.map((r) => ({ producerPid: r.producerPid, details: r.details })) };
}

/** waitLog pattern for one stage written by one process (base keys precede the settlement). */
export function stagePattern(pid, stage) {
  return `"producerPid":${Number(pid)},[^\\n]*"stage":"${stage}"`;
}

/**
 * The plan's todo 8 invariant over one operation's stages: before commit the candidate accepted
 * nothing on any inherited listener, a probe process could never take either lock, and the public
 * socket path never disappeared.
 */
export function authorityInvariant(stages, candidatePid) {
  const own = stages.filter((s) => s.producerPid === candidatePid);
  const preCommit = own.filter((s) => PRE_COMMIT_STAGES.includes(s.stage));
  const accepted = preCommit.filter((s) => LISTENER_ROLES.some((role) => Number(s.acceptCounts?.[role] ?? 0) !== 0));
  const lockFree = stages.filter((s) => s.probeFlockRefused?.legacy !== true || (s.probeFlockRefused?.persistent != null && s.probeFlockRefused.persistent !== true));
  const socketGone = stages.filter((s) => s.publicSocketPresent !== true);
  return {
    stages: stages.map((s) => `${s.producerPid}:${s.stage}`),
    candidatePreCommit: preCommit.map((s) => s.stage),
    acceptedBeforeCommit: accepted.map((s) => ({ stage: s.stage, acceptCounts: s.acceptCounts })),
    lockFreeAt: lockFree.map((s) => ({ stage: s.stage, probeFlockRefused: s.probeFlockRefused })),
    socketMissingAt: socketGone.map((s) => s.stage),
  };
}
