#!/usr/bin/env node
/**
 * Phase 4 handover decision-record accounting audit.
 *
 * SCOPE, stated so this can never be mistaken for acceptance:
 *   This validates the *accounting* of a handover decision record - the fields `record_decision`
 *   writes, the by-construction declaration `DecisionAccounting` carries, the ledger it embeds,
 *   and the settled-decision mapping `recorded_decision` performs. It starts no daemon, opens no
 *   socket, spawns no process, and reads no clock, so it proves nothing about serving, PTY
 *   ownership, or the three Phase 4 failure injections. Those need
 *   `scripts/qa/phase4-handover-failure-injection.acceptance.mjs` (a concurrent, separately
 *   owned file this one neither imports nor replaces) or the in-tree `#[tokio::test]` cases in
 *   `src-tauri/src/daemon/handover_accounting_tests.rs`. What this adds is (1) a deterministic
 *   checker a captured record set can be run through, and (2) the per-case record obligations
 *   stated as data, so real evidence is judged against the plan rather than against prose.
 *
 * Plan: `.omo/plans/local-pty-crash-survival-20261116.md` (rev 5), "Phase 4 - handover accounting
 * observability", including the (a)/(b)/(c) acceptance split.
 *
 * Usage:
 *   node scripts/qa/phase4-handover-contract-audit.mjs --self-check [--json]
 *   node scripts/qa/phase4-handover-contract-audit.mjs --audit-dir <runtime dir>
 *        [--legacy-socket <exact path>] [--allow-undeclared] [--json]
 *
 * Exit codes: 0 every check passed, 1 at least one violation, 2 usage error.
 */

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { pathToFileURL } from 'node:url';

// ---------------------------------------------------------------------------------------------
// Contract constants. Each names the production item it mirrors; drift here is an audit bug.
// ---------------------------------------------------------------------------------------------

/** `HandoverState` serde values (`handover_transaction.rs`, `rename_all = "snake_case"`). */
export const HANDOVER_STATES = Object.freeze([
  'active',
  'preparing',
  'frozen',
  'transferring',
  'commit_ready',
  'retired',
]);

/** `HandoverState::is_pre_commit`: states no durable decision may be in. */
export const PRE_COMMIT_STATES = Object.freeze(['preparing', 'frozen', 'transferring', 'commit_ready']);

/** `HandoverState::is_terminal`: the irreversible post-commit decision. */
export const RETIRED = 'retired';
/** The state a durable abort settles in (with a rollback reason). */
export const ACTIVE = 'active';

/** `DecisionAccounting` serde values (`rename_all = "snake_case"`). */
export const ACCOUNTING = Object.freeze({
  SUCCESSOR_IDENTITY_UNUSED: 'successor_identity_unused',
  SESSION_INVENTORY_UNUSED: 'session_inventory_unused',
  SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED: 'successor_identity_and_inventory_unused',
});

export const ACCOUNTING_VALUES = Object.freeze(Object.values(ACCOUNTING));

/** What `record_decision(false)` writes, verbatim: an abort decision is identified by it. */
export const ABORT_ROLLBACK_REASON = 'predecessor abort decision';

/** Typed warning event `DECISION_ACCOUNTING_UNPOPULATED` in `daemon/handover.rs`. */
export const ACCOUNTING_EVENT = 'handover.decision_accounting_unpopulated';

/**
 * `write_decision` uses `legacy.with_extension(..)`, which REPLACES the `.sock` extension: a
 * legacy socket `handover-a1.sock` writes `handover-a1.transaction`, staging in
 * `handover-a1.transaction.pending` and renaming that onto the final name.
 */
export const DECISION_SUFFIX = '.transaction';
export const PENDING_SUFFIX = '.transaction.pending';

/** `SessionKind` serde values. */
export const SESSION_KINDS = Object.freeze(['pty', 'direct_ssh', 'paired_daemon']);

// ---------------------------------------------------------------------------------------------
// Field derivations, each a direct mirror of one production function.
// ---------------------------------------------------------------------------------------------

const isZero = (value) => Number(value ?? 0) === 0;

const inventoryOf = (record) => (Array.isArray(record?.sessionInventory) ? record.sessionInventory : []);

/** `HandoverManager::unpopulated_accounting`, in its own check order. */
export function unpopulatedField(record) {
  if (isZero(record?.successorPid)) return 'successorPid';
  if (isZero(record?.successorEpoch)) return 'successorEpoch';
  if (inventoryOf(record).length === 0) return 'sessionInventory';
  return null;
}

/**
 * `HandoverManager::accounting_declaration`: the declaration a record truthfully owes, derived
 * from the record itself. A populated inventory is never declared unused; a fully accounted
 * record declares nothing.
 */
export function deriveDeclaration(record) {
  const identityUnused = isZero(record?.successorPid) || isZero(record?.successorEpoch);
  const inventoryUnused = inventoryOf(record).length === 0;
  if (identityUnused && inventoryUnused) return ACCOUNTING.SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED;
  if (identityUnused) return ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED;
  if (inventoryUnused) return ACCOUNTING.SESSION_INVENTORY_UNUSED;
  return null;
}

/**
 * `HandoverManager::recorded_decision`: `'Retired'` for a commit, `'Active'` for an abort that
 * recorded its reason, `null` when the record settles nothing.
 */
export function settledState(record) {
  if (record == null || typeof record !== 'object') return null;
  if (record.state === RETIRED) return 'Retired';
  if (record.state === ACTIVE && record.rollbackReason != null) return 'Active';
  return null;
}

/**
 * True only for a record that settled something. The declaration exists so that a zero
 * `successorPid` or an empty inventory is never read as "never committed"; this is the accessor
 * that keeps that distinction.
 */
export function hasDurableDecision(record) {
  return settledState(record) !== null;
}

// ---------------------------------------------------------------------------------------------
// Record audit.
// ---------------------------------------------------------------------------------------------

function auditInventory(record, fail) {
  const inventory = record?.sessionInventory;
  if (inventory !== undefined && !Array.isArray(inventory)) {
    fail('INVENTORY_NOT_AN_ARRAY', 'sessionInventory', `got ${typeof inventory}`);
    return;
  }
  const seen = new Set();
  for (const [index, entry] of inventoryOf(record).entries()) {
    const field = `sessionInventory[${index}]`;
    if (entry == null || typeof entry !== 'object') {
      fail('INVENTORY_ENTRY_NOT_AN_OBJECT', field, `got ${typeof entry}`);
      continue;
    }
    if (typeof entry.sessionId !== 'string' || entry.sessionId.length === 0) {
      fail('INVENTORY_ENTRY_UNNAMED', `${field}.sessionId`, `got ${JSON.stringify(entry.sessionId)}`);
    } else if (seen.has(entry.sessionId)) {
      fail('INVENTORY_ENTRY_DUPLICATE', `${field}.sessionId`, `${entry.sessionId} appears twice`);
    } else {
      seen.add(entry.sessionId);
    }
    if (!SESSION_KINDS.includes(entry.sessionKind)) {
      fail('INVENTORY_KIND_UNKNOWN', `${field}.sessionKind`, `got ${JSON.stringify(entry.sessionKind)}`);
      continue;
    }
    const roles = Array.isArray(entry.expectedFdRoles) ? entry.expectedFdRoles : null;
    if (roles === null) {
      fail('INVENTORY_ROLES_NOT_AN_ARRAY', `${field}.expectedFdRoles`, `got ${typeof entry.expectedFdRoles}`);
      continue;
    }
    // `SessionInventoryEntry::pty` names its own master role; `::paired_daemon` names none.
    if (entry.sessionKind === 'pty') {
      const owns = roles.some((role) => role?.role === 'pty_master' && role?.session_id === entry.sessionId);
      if (!owns) {
        fail('INVENTORY_ROLE_MISMATCH', `${field}.expectedFdRoles`, `a pty entry must name its own pty_master role`);
      }
    }
    if (entry.sessionKind === 'paired_daemon' && roles.length !== 0) {
      fail('INVENTORY_ROLE_MISMATCH', `${field}.expectedFdRoles`, `a paired_daemon entry carries no fd roles`);
    }
  }
}

function auditLedger(record, fail) {
  const ledger = record?.frameReceiptLedger;
  if (ledger == null) return;
  if (ledger.transferId !== record.transferId) {
    fail('LEDGER_TRANSFER_ID', 'frameReceiptLedger.transferId', `${JSON.stringify(ledger.transferId)} is not the record's transferId`);
  }
  const records = ledger.records ?? {};
  const sequences = Object.keys(records).map(Number).sort((left, right) => left - right);
  let expected = 0;
  for (const sequence of sequences) {
    if (!Number.isInteger(sequence) || sequence < 0) {
      fail('LEDGER_SEQUENCE_NOT_A_NUMBER', 'frameReceiptLedger.records', `key ${JSON.stringify(String(sequence))}`);
      continue;
    }
    if (sequence !== expected) {
      fail('LEDGER_SEQUENCE_GAP', 'frameReceiptLedger.records', `sequence ${sequence} where ${expected} was next`);
    }
    expected = sequence + 1;
    const entry = records[String(sequence)] ?? {};
    if (entry.frameId?.transferId !== record.transferId) {
      fail('LEDGER_FRAME_TRANSFER_ID', `frameReceiptLedger.records[${sequence}].frameId.transferId`, 'not the record transferId');
    }
    if (Number(entry.frameId?.frameSequence) !== sequence) {
      fail('LEDGER_FRAME_SEQUENCE', `frameReceiptLedger.records[${sequence}].frameId.frameSequence`, `key is ${sequence}`);
    }
    if (Number(entry.ack?.frameSequence) !== sequence) {
      fail('LEDGER_ACK_SEQUENCE', `frameReceiptLedger.records[${sequence}].ack.frameSequence`, `key is ${sequence}`);
    }
    if (entry.ack?.transferId !== record.transferId) {
      fail('LEDGER_ACK_TRANSFER_ID', `frameReceiptLedger.records[${sequence}].ack.transferId`, 'not the record transferId');
    }
  }
  if (Number(ledger.nextExpectedSequence ?? 0) !== expected) {
    fail(
      'LEDGER_NEXT_SEQUENCE',
      'frameReceiptLedger.nextExpectedSequence',
      `${JSON.stringify(ledger.nextExpectedSequence)} although ${expected} is next for ${sequences.length} record(s)`,
    );
  }
}

/**
 * Audits one decision record.
 *
 * @param {object} record parsed `<legacy>.transaction` JSON
 * @param {{legacySocketPath?: string|null, requireDeclaration?: boolean}} [options]
 *   `legacySocketPath` asserts the full-path identity `recorded_decision` enforces.
 *   `requireDeclaration` (default true) treats a missing `decisionAccounting` on an unpopulated
 *   record as a violation; pass false to grade a record written by an older peer as a warning.
 */
export function auditTransaction(record, options = {}) {
  const { legacySocketPath = null, requireDeclaration = true } = options;
  const errors = [];
  const warnings = [];
  const fail = (code, field, detail) => errors.push({ code, field, detail });

  if (record == null || typeof record !== 'object' || Array.isArray(record)) {
    fail('RECORD_NOT_AN_OBJECT', '', `a decision file must hold a JSON object, got ${Array.isArray(record) ? 'array' : typeof record}`);
    return {
      ok: false,
      errors,
      warnings,
      state: null,
      rollbackReason: null,
      settled: null,
      decided: false,
      declared: null,
      derived: null,
      unpopulated: null,
      event: null,
    };
  }

  // `recorded_decision` refuses the read when the record names any other socket.
  if (typeof record.transferId !== 'string' || record.transferId.length === 0) {
    fail('TRANSFER_ID_MISSING', 'transferId', `got ${JSON.stringify(record.transferId)}`);
  } else if (legacySocketPath !== null) {
    if (record.transferId !== legacySocketPath) {
      fail('IDENTITY_MISMATCH', 'transferId', `${record.transferId} is not ${legacySocketPath}`);
    }
  } else if (!record.transferId.endsWith('.sock')) {
    fail('IDENTITY_NOT_A_SOCKET', 'transferId', `${record.transferId} does not name a socket`);
  }

  if (!HANDOVER_STATES.includes(record.state)) {
    fail('STATE_UNKNOWN', 'state', `got ${JSON.stringify(record.state)}`);
  } else if (PRE_COMMIT_STATES.includes(record.state)) {
    fail(
      'DECISION_NOT_SETTLED',
      'state',
      `a record in state ${record.state} settles nothing; record_decision writes only retired, or active with a rollback reason`,
    );
  }

  if (record.state === RETIRED && record.rollbackReason != null) {
    fail('RETIRED_WITH_ROLLBACK_REASON', 'rollbackReason', `a committed record carries ${JSON.stringify(record.rollbackReason)}`);
  }
  if (record.state === ACTIVE && record.rollbackReason == null) {
    fail(
      'ABORT_WITHOUT_REASON',
      'rollbackReason',
      'an active record on disk is an abort, and record_decision always writes its reason',
    );
  }

  if (record.predecessorPid !== undefined && isZero(record.predecessorPid)) {
    fail('PREDECESSOR_PID_ZERO', 'predecessorPid', 'the writer records its own pid');
  }

  const declared = record.decisionAccounting ?? null;
  const derived = deriveDeclaration(record);
  if (declared !== null && !ACCOUNTING_VALUES.includes(declared)) {
    fail('ACCOUNTING_UNKNOWN', 'decisionAccounting', `got ${JSON.stringify(declared)}`);
  } else if (declared !== null && declared !== derived) {
    fail(
      'ACCOUNTING_CONTRADICTS_RECORD',
      'decisionAccounting',
      `declares ${declared}, but the record derives ${derived ?? 'nothing unpopulated'}`,
    );
  } else if (declared === null && derived !== null) {
    const detail = `${derived}: the zero identity or empty inventory here is by construction, not evidence of a missing commit`;
    if (requireDeclaration) fail('ACCOUNTING_UNDECLARED', 'decisionAccounting', detail);
    else warnings.push({ code: 'ACCOUNTING_UNDECLARED', field: 'decisionAccounting', detail });
  }

  auditInventory(record, fail);
  auditLedger(record, fail);

  const settled = settledState(record);
  return {
    ok: errors.length === 0,
    errors,
    warnings,
    state: record.state,
    rollbackReason: record.rollbackReason ?? null,
    settled,
    decided: settled !== null,
    declared,
    derived,
    unpopulated: unpopulatedField(record),
    event: derived === null ? null : ACCOUNTING_EVENT,
  };
}

/**
 * Audits every decision record in a captured set, plus any write that never became durable.
 *
 * @param {{files?: Record<string,string>, pending?: string[], legacySocketPath?: string|null,
 *          requireDeclaration?: boolean}} set
 *   `files` maps `legacy-<id>.transaction` basenames to their text; `pending` lists surviving
 *   `legacy-<id>.transaction.pending` staging files.
 */
export function auditDecisionSet(set = {}) {
  const { files = {}, pending = [], legacySocketPath = null, requireDeclaration = true } = set;
  const records = [];
  for (const [name, text] of Object.entries(files)) {
    let record;
    try {
      record = JSON.parse(text);
    } catch (error) {
      records.push({
        name,
        ok: false,
        errors: [{ code: 'RECORD_NOT_JSON', field: '', detail: String(error?.message ?? error) }],
        warnings: [],
        settled: null,
        decided: false,
      });
      continue;
    }
    const audit = auditTransaction(record, { legacySocketPath, requireDeclaration });
    // `with_extension("transaction")` swapped the socket's own extension, so the socket a record
    // must sit beside is its basename with `.sock` restored.
    const beside = name.endsWith(DECISION_SUFFIX) ? `${name.slice(0, -DECISION_SUFFIX.length)}.sock` : null;
    const named = basename(String(record?.transferId ?? ''));
    if (beside !== null && named !== basename(beside)) {
      audit.errors.push({
        code: 'IDENTITY_BESIDE_WRONG_SOCKET',
        field: 'transferId',
        detail: `${name} holds transferId ${JSON.stringify(record?.transferId)}; its socket is ${beside}`,
      });
      audit.ok = false;
    }
    records.push({ name, ...audit });
  }
  const incomplete = pending.map((name) => ({
    name,
    ok: false,
    errors: [
      {
        code: 'DECISION_WRITE_INCOMPLETE',
        field: '',
        detail: 'a staging file survived; the rename that makes the decision durable never happened',
      },
    ],
    warnings: [],
    settled: null,
    decided: false,
  }));
  const all = [...records, ...incomplete];
  return {
    ok: all.every((entry) => entry.ok),
    records: all,
    settled: records.map((entry) => entry.settled).filter((state) => state !== null),
    // Absence of a record is a legitimate state (no decision yet), never a violation by itself.
    empty: all.length === 0,
  };
}

// ---------------------------------------------------------------------------------------------
// The plan's per-case record obligations, as data. Accounting only: a case that satisfies every
// obligation here has still proven nothing about serving.
// ---------------------------------------------------------------------------------------------

const NO_DECISION = Object.freeze({ expect: 'no-decision' });
const settledAs = (state, rollbackReason) => Object.freeze({ expect: 'settled', state, rollbackReason });

export const CASE_OBLIGATIONS = Object.freeze({
  // (a) successor dies BEFORE the decision: nothing durable at injection, the abort settles the
  // predecessor's own record, the retry commits, and the earlier abort record is never rewritten.
  'a-successor-dies-before-decision': Object.freeze({
    'at-injection': NO_DECISION,
    'after-abort': settledAs(ACTIVE, ABORT_ROLLBACK_REASON),
    'after-retry-commit': settledAs(RETIRED, null),
    'first-attempt-after-retry': settledAs(ACTIVE, ABORT_ROLLBACK_REASON),
  }),
  // (b) successor dies AFTER the decision: the commit is durable and carries no rollback reason.
  'b-successor-dies-after-decision': Object.freeze({
    'after-decision': settledAs(RETIRED, null),
  }),
  // (c) predecessor dies BEFORE the decision: the abort leaves no record behind at all.
  'c-predecessor-dies-before-decision': Object.freeze({
    'at-injection': NO_DECISION,
  }),
});

/**
 * Judges one obligation against the record audited for that moment.
 *
 * @param {{expect: string, state?: string, rollbackReason?: string|null}} obligation
 * @param {null|object} audited a record, or null when no record existed at that moment
 */
export function auditObligation(obligation, audited) {
  const errors = [];
  if (obligation.expect === 'no-decision') {
    if (audited != null) {
      errors.push({ code: 'DECISION_TOO_EARLY', field: '', detail: `a record existed where the plan expects none` });
    }
    return { ok: errors.length === 0, errors };
  }
  if (audited == null) {
    errors.push({ code: 'DECISION_MISSING', field: '', detail: `the plan expects a settled ${obligation.state} record` });
    return { ok: false, errors };
  }
  if (!audited.ok) {
    errors.push({ code: 'RECORD_INVALID', field: '', detail: `the record failed its own audit: ${audited.errors.map((entry) => entry.code).join(',')}` });
  }
  if (audited.settled !== (obligation.state === RETIRED ? 'Retired' : 'Active')) {
    errors.push({ code: 'DECISION_UNEXPECTED', field: 'state', detail: `settled as ${audited.settled ?? 'nothing'}, expected ${obligation.state}` });
  }
  if ('rollbackReason' in obligation && (audited.rollbackReason ?? null) !== obligation.rollbackReason) {
    errors.push({
      code: 'ROLLBACK_REASON_UNEXPECTED',
      field: 'rollbackReason',
      detail: `got ${JSON.stringify(audited.rollbackReason ?? null)}, expected ${JSON.stringify(obligation.rollbackReason)}`,
    });
  }
  return { ok: errors.length === 0, errors };
}

// ---------------------------------------------------------------------------------------------
// Self-check fixtures. Deterministic: in-memory records only, no clock, no process, no socket.
// ---------------------------------------------------------------------------------------------

const TRANSFER = '/tmp/fx4-audit/handover-0001.sock';

function fixture(overrides = {}) {
  return {
    transferId: TRANSFER,
    clientRequestId: TRANSFER,
    predecessorPid: 4242,
    predecessorEpoch: 0,
    successorPid: 0,
    successorEpoch: 0,
    sessionInventory: [
      { sessionId: 'session-1', sessionKind: 'pty', expectedFdRoles: [{ role: 'pty_master', session_id: 'session-1' }] },
    ],
    expectedFdRoles: [{ role: 'pty_master', session_id: 'session-1' }],
    frameReceiptLedger: { transferId: TRANSFER, records: {}, nextExpectedSequence: 0 },
    commitNonce: null,
    state: RETIRED,
    fingerprint: TRANSFER,
    manifestDigest: null,
    rollbackReason: null,
    result: null,
    decisionAccounting: ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED,
    ...overrides,
  };
}

const codes = (audit) => audit.errors.map((entry) => entry.code);
const describe = (audit) => JSON.stringify({ errors: audit.errors, warnings: audit.warnings });
const withoutAccounting = ({ decisionAccounting, ...rest }) => rest;

function runSelfCheck() {
  const results = [];
  const check = (name, body) => {
    try {
      body();
      results.push({ name, ok: true });
    } catch (error) {
      results.push({ name, ok: false, detail: String(error?.message ?? error) });
    }
  };

  check('a committed record with an unpopulated identity and a populated inventory is valid and decided', () => {
    const audit = auditTransaction(fixture(), { legacySocketPath: TRANSFER });
    assert.equal(audit.ok, true, describe(audit));
    assert.equal(audit.settled, 'Retired');
    assert.equal(audit.decided, true);
    assert.equal(audit.derived, ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED);
    assert.equal(audit.unpopulated, 'successorPid');
    assert.deepEqual(audit.warnings, []);
  });

  check('a zero successorPid is never read as "never committed"', () => {
    const audit = auditTransaction(fixture({ successorPid: 0, successorEpoch: 0 }), { legacySocketPath: TRANSFER });
    assert.equal(audit.ok, true, describe(audit));
    assert.equal(hasDurableDecision(fixture()), true);
    assert.equal(audit.event, ACCOUNTING_EVENT);
  });

  check('an abort record settles Active with its own reason and declares both fields unused', () => {
    const audit = auditTransaction(
      fixture({
        state: ACTIVE,
        rollbackReason: ABORT_ROLLBACK_REASON,
        sessionInventory: [],
        expectedFdRoles: [],
        decisionAccounting: ACCOUNTING.SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED,
      }),
      { legacySocketPath: TRANSFER },
    );
    assert.equal(audit.ok, true, describe(audit));
    assert.equal(audit.settled, 'Active');
    assert.equal(audit.derived, ACCOUNTING.SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED);
  });

  check('a committed record carrying a rollback reason is refused', () => {
    const audit = auditTransaction(fixture({ rollbackReason: ABORT_ROLLBACK_REASON }), { legacySocketPath: TRANSFER });
    assert.equal(audit.ok, false);
    assert.deepEqual(codes(audit), ['RETIRED_WITH_ROLLBACK_REASON']);
    assert.equal(audit.settled, 'Retired', 'the settled mapping still reports what the state says');
  });

  check('an active record without a reason settles nothing and is refused', () => {
    const audit = auditTransaction(
      fixture({ state: ACTIVE, rollbackReason: null, decisionAccounting: ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED }),
      { legacySocketPath: TRANSFER },
    );
    assert.equal(audit.ok, false);
    assert.deepEqual(codes(audit), ['ABORT_WITHOUT_REASON']);
    assert.equal(audit.settled, null);
    assert.equal(audit.decided, false);
  });

  check('a declaration that contradicts a fully accounted record is refused', () => {
    const audit = auditTransaction(
      fixture({ successorPid: 7, successorEpoch: 3, decisionAccounting: ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED }),
      { legacySocketPath: TRANSFER },
    );
    assert.equal(audit.ok, false);
    assert.deepEqual(codes(audit), ['ACCOUNTING_CONTRADICTS_RECORD']);
    assert.equal(audit.derived, null);
  });

  check('an undeclared unpopulated record is an error for this build and a warning for an older peer', () => {
    const record = withoutAccounting(fixture());
    const strict = auditTransaction(record, { legacySocketPath: TRANSFER });
    assert.deepEqual(codes(strict), ['ACCOUNTING_UNDECLARED']);
    assert.equal(strict.ok, false);
    const lenient = auditTransaction(record, { legacySocketPath: TRANSFER, requireDeclaration: false });
    assert.equal(lenient.ok, true, describe(lenient));
    assert.deepEqual(lenient.warnings.map((entry) => entry.code), ['ACCOUNTING_UNDECLARED']);
    assert.equal(lenient.settled, 'Retired', 'an undeclared record is still a decision');
  });

  check('a record naming another socket is an identity mismatch', () => {
    const audit = auditTransaction(fixture(), { legacySocketPath: '/tmp/fx4-audit/handover-0002.sock' });
    assert.deepEqual(codes(audit), ['IDENTITY_MISMATCH']);
  });

  check('a pre-commit state on disk settles nothing', () => {
    const audit = auditTransaction(fixture({ state: 'transferring' }), { legacySocketPath: TRANSFER });
    assert.deepEqual(codes(audit), ['DECISION_NOT_SETTLED']);
    assert.equal(audit.settled, null);
  });

  check('an unknown state value is refused, including the capitalized spelling', () => {
    const audit = auditTransaction(fixture({ state: 'Retired' }), { legacySocketPath: TRANSFER });
    assert.deepEqual(codes(audit), ['STATE_UNKNOWN']);
    assert.equal(audit.settled, null);
  });

  check('a ledger sequence gap and a wrong next sequence are both refused', () => {
    const frame = (sequence) => ({
      frameId: { transferId: TRANSFER, frameSequence: sequence, sessionOrdinal: 0, fdRoles: [], payloadDigest: `d${sequence}` },
      ack: { transferId: TRANSFER, frameSequence: sequence, status: 'accepted', fdRolesAccepted: [], timestampMs: 0 },
    });
    const gapped = fixture({
      frameReceiptLedger: { transferId: TRANSFER, records: { 1: frame(1) }, nextExpectedSequence: 2 },
    });
    assert.deepEqual(codes(auditTransaction(gapped)), ['LEDGER_SEQUENCE_GAP']);
    const coherent = fixture({
      frameReceiptLedger: { transferId: TRANSFER, records: { 0: frame(0), 1: frame(1) }, nextExpectedSequence: 2 },
    });
    assert.deepEqual(codes(auditTransaction(coherent)), [], describe(auditTransaction(coherent)));
    const stale = fixture({ frameReceiptLedger: { transferId: TRANSFER, records: { 0: frame(0) }, nextExpectedSequence: 0 } });
    assert.deepEqual(codes(auditTransaction(stale)), ['LEDGER_NEXT_SEQUENCE']);
  });

  check('a pty inventory entry that does not name its own master role is refused', () => {
    const audit = auditTransaction(
      fixture({
        sessionInventory: [{ sessionId: 'session-1', sessionKind: 'pty', expectedFdRoles: [{ role: 'pty_master', session_id: 'other' }] }],
      }),
    );
    assert.deepEqual(codes(audit), ['INVENTORY_ROLE_MISMATCH']);
  });

  check('a duplicated inventory entry is refused', () => {
    const entry = { sessionId: 'session-1', sessionKind: 'paired_daemon', expectedFdRoles: [] };
    const audit = auditTransaction(fixture({ sessionInventory: [entry, entry] }));
    assert.deepEqual(codes(audit), ['INVENTORY_ENTRY_DUPLICATE']);
  });

  check('a surviving staging file means the decision never became durable', () => {
    const set = auditDecisionSet({ pending: ['handover-0001.transaction.pending'] });
    assert.equal(set.ok, false);
    assert.deepEqual(set.records.map((entry) => entry.errors[0].code), ['DECISION_WRITE_INCOMPLETE']);
    assert.equal(set.settled.length, 0);
  });

  check('an empty set is not a violation: no record is a legitimate moment', () => {
    const set = auditDecisionSet({});
    assert.equal(set.ok, true);
    assert.equal(set.empty, true);
  });

  check('a record filed beside the socket it names is accepted by the set audit', () => {
    const set = auditDecisionSet({ files: { 'handover-0001.transaction': JSON.stringify(fixture()) } });
    assert.equal(set.ok, true, JSON.stringify(set.records[0]?.errors));
    assert.equal(set.records[0].settled, 'Retired');
  });

  check('a record filed beside the wrong socket is refused by the set audit', () => {
    const set = auditDecisionSet({ files: { 'handover-0002.transaction': JSON.stringify(fixture()) } });
    assert.equal(set.ok, false);
    assert.deepEqual(set.records[0].errors.map((entry) => entry.code), ['IDENTITY_BESIDE_WRONG_SOCKET']);
  });

  check('the plan case (a) obligations accept the record set the plan describes', () => {
    const obligations = CASE_OBLIGATIONS['a-successor-dies-before-decision'];
    const abort = auditTransaction(fixture({ state: ACTIVE, rollbackReason: ABORT_ROLLBACK_REASON }), { legacySocketPath: TRANSFER });
    const commit = auditTransaction(fixture(), { legacySocketPath: TRANSFER });
    assert.equal(auditObligation(obligations['at-injection'], null).ok, true);
    assert.equal(auditObligation(obligations['after-abort'], abort).ok, true);
    assert.equal(auditObligation(obligations['after-retry-commit'], commit).ok, true);
    assert.equal(auditObligation(obligations['first-attempt-after-retry'], abort).ok, true);
  });

  check('the plan case (a) obligations reject a commit where the abort is owed', () => {
    const obligations = CASE_OBLIGATIONS['a-successor-dies-before-decision'];
    const commit = auditTransaction(fixture(), { legacySocketPath: TRANSFER });
    const verdict = auditObligation(obligations['after-abort'], commit);
    assert.equal(verdict.ok, false);
    assert.deepEqual(verdict.errors.map((entry) => entry.code), ['DECISION_UNEXPECTED', 'ROLLBACK_REASON_UNEXPECTED']);
    assert.equal(auditObligation(obligations['at-injection'], commit).ok, false);
  });

  check('the plan case (c) obligation rejects any record before the decision', () => {
    const obligations = CASE_OBLIGATIONS['c-predecessor-dies-before-decision'];
    const commit = auditTransaction(fixture(), { legacySocketPath: TRANSFER });
    const verdict = auditObligation(obligations['at-injection'], commit);
    assert.equal(verdict.ok, false);
    assert.deepEqual(verdict.errors.map((entry) => entry.code), ['DECISION_TOO_EARLY']);
  });

  return results;
}

// ---------------------------------------------------------------------------------------------
// CLI.
// ---------------------------------------------------------------------------------------------

const USAGE = `Phase 4 handover decision-record accounting audit (not process acceptance).

  node scripts/qa/phase4-handover-contract-audit.mjs --self-check [--json]
  node scripts/qa/phase4-handover-contract-audit.mjs --audit-dir <runtime dir>
       [--legacy-socket <exact path>] [--allow-undeclared] [--json]

--audit-dir reads legacy-*.transaction records and any legacy-*.transaction.pending staging file.
--allow-undeclared grades a missing decisionAccounting as a warning (records written by an older peer).
Exit codes: 0 every check passed, 1 at least one violation, 2 usage error.`;

function parseArgs(argv) {
  const options = { mode: null, dir: null, legacySocket: null, requireDeclaration: true, json: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--self-check') options.mode = 'self-check';
    else if (arg === '--audit-dir') {
      options.mode = 'audit-dir';
      index += 1;
      options.dir = argv[index] ?? null;
      if (options.dir === null) throw new Error('--audit-dir needs a directory');
    } else if (arg === '--legacy-socket') {
      index += 1;
      options.legacySocket = argv[index] ?? null;
      if (options.legacySocket === null) throw new Error('--legacy-socket needs a path');
    } else if (arg === '--allow-undeclared') options.requireDeclaration = false;
    else if (arg === '--json') options.json = true;
    else if (arg === '--help' || arg === '-h') options.mode = 'help';
    else throw new Error(`unknown argument ${JSON.stringify(arg)}`);
  }
  if (options.mode === null) options.mode = 'self-check';
  return options;
}

function readDecisionSet(dir, options) {
  const files = {};
  const pending = [];
  for (const name of readdirSync(dir).sort()) {
    if (name.endsWith(PENDING_SUFFIX)) pending.push(name);
    else if (name.endsWith(DECISION_SUFFIX)) files[name] = readFileSync(join(dir, name), 'utf8');
  }
  return auditDecisionSet({ files, pending, legacySocketPath: options.legacySocket, requireDeclaration: options.requireDeclaration });
}

export function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`${error.message}\n\n${USAGE}\n`);
    return 2;
  }
  if (options.mode === 'help') {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }

  const report = options.mode === 'self-check'
    ? { run: 'phase4-handover-contract-audit', mode: 'self-check', checks: runSelfCheck() }
    : { run: 'phase4-handover-contract-audit', mode: 'audit-dir', dir: options.dir, set: readDecisionSet(options.dir, options) };
  report.ok = options.mode === 'self-check' ? report.checks.every((entry) => entry.ok) : report.set.ok;

  if (options.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    return report.ok ? 0 : 1;
  }
  if (options.mode === 'self-check') {
    for (const entry of report.checks) {
      process.stdout.write(`${entry.ok ? 'PASS' : 'FAIL'} ${entry.name}${entry.ok ? '' : `\n     ${entry.detail}`}\n`);
    }
    const failed = report.checks.filter((entry) => !entry.ok).length;
    process.stdout.write(`\n${report.checks.length - failed}/${report.checks.length} checks passed\n`);
  } else {
    for (const entry of report.set.records) {
      const verdict = entry.ok ? 'OK  ' : 'FAIL';
      const state = entry.settled ?? (entry.ok ? 'no-decision' : 'invalid');
      process.stdout.write(`${verdict} ${entry.name} settled=${state}\n`);
      for (const problem of [...entry.errors, ...entry.warnings]) {
        process.stdout.write(`     ${problem.code} ${problem.field} ${problem.detail}\n`);
      }
    }
    if (report.set.empty) process.stdout.write('no decision record and no staging file in the directory\n');
  }
  return report.ok ? 0 : 1;
}

// Importing this file must never run anything: the CLI runs only as the entry point.
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  process.exitCode = main(process.argv.slice(2));
}
