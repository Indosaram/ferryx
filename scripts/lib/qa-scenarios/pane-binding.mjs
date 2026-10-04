#!/usr/bin/env node
// Job 2 (task-9 root cause 2, established by the decisive control in
// `.omo/evidence/local-pane-liveness-completion-replan/task-9/ACCESSIBILITY-EXPERIMENT.md`):
//
// With the UI served, the app boots to its EMPTY state - the UIA tree shows
// `"No open tabs"`, `"Open a terminal or browser tab to get started."`,
// `"New Terminal"` [Button], `"New Browser Tab"` [Button] - and no pane. So the
// scenario's `driver.split` has nothing to split: `Split pane right` "does not
// exist in the tree, because its parent pane does not exist".
//
// ROUTE DECISION - drive the UI (route A), not seed the restore path (route B).
//
// Route A (`"New Terminal"` is a real named button; the click runs the product's
// real `cmd_terminal_spawn` path, which creates a pane and its session):
//   * The identity the scenario needs comes from the product's OWN receipts when
//     they exist. The click's pane presents a frame, and the native lane's
//     presentation producer writes the real `PanePresentationReceipt` (its
//     authoritative seven-field `attachTuple`) into
//     `presentation.receipt.jsonl`. `bindPaneSession` below reads exactly that
//     first, and no id is ever invented from it.
//
// AMENDMENT (task-9 pass-13, the lead's design decision). That producer is not
// always reachable. Its settlement requires `receipt.presented &&
// tuple_is_current`, and `pane_liveness_presentation_receipt(frame_attach_tuple,
// true)` returns `None` - emitting NOTHING AT ALL - when there is no attach
// tuple (`src-tauri/src/native_terminal/surface_host.rs`, read in pass 13). The
// frontend only builds that tuple on the `!session.spawnIntent` branch, so if a
// UI-created pane's native surface does not attach, the receipt stream stays
// empty. And three of the seven tuple fields (`frontendSessionId`,
// `paneIdentity`, `bindingKey`) are frontend-owned identities the daemon has no
// concept of, so a harness-side producer cannot synthesise them - that is the
// "invented id" this module's contract rejects.
//
// The pre-split pane is therefore a SETUP artifact, and the binding needs
// exactly one thing from it honestly: its backend session identity. The plan's
// seven-field + `presented === true` requirement belongs to the pane the SPLIT
// creates, and that assertion is unchanged (`split-scenarios.mjs`). So:
//   1. PREFER the product's own tuple/receipt when it settles (today's path).
//   2. OTHERWISE fall back to a MEASURED daemon inventory delta: the sessions
//      the isolated daemon reports AFTER the click minus the ones it reported
//      BEFORE, excluding the settled fixture sessions. That delta names exactly
//      the session the click created - measured over the product's own control
//      wire (`handshake` + `listSessions` only, in `daemon-inventory.mjs`),
//      never guessed.
//   3. RECORD which source settled it (`paneBinding.settledBy:
//      'presentation-tuple' | 'inventory-delta'`), and keep the typed failure
//      when neither yields a session (`PANE_BINDING_UNBOUND`, with the enriched
//      detail) and when either yields more than one (`PANE_BINDING_AMBIGUOUS`).
//
// Nothing the plan requires is weakened: the split's own seven-field receipt
// assertion, `ptyCreatedCount`, the single-PTY invariant and every other
// assertion are untouched. The binding's evidence shape stays a SUPERSET - the
// tuple path keeps emitting all seven fields, and the delta path always carries
// `backendSessionId` and `settledBy` while omitting the three frontend-owned
// fields rather than inventing them. The only assertion-side consumer in the
// tree is `split-scenarios.mjs`'s `ctx.paneBinding?.backendSessionId`.
//   * No product change and no second launch: the pane exists in the app the
//     scenario is measuring, created through the same IPC path a user's click
//     takes.
//
// Route B (seed `session_state.json` with a layout whose pane points at a fixture
// session id) was REJECTED, with reasons from the code rather than from taste:
//   * The fixture sessions do not exist before the app boots: the product creates
//     them AT boot (`create_gui_fixture_sessions` -> daemon `Spawn`/
//     `create_local_split` in `src-tauri/src/ipc/qa_barrier.rs`) and only then
//     settles `fixture-setup`. So the runner cannot know a fixture session id to
//     seed - route B would need a first boot to learn it and a SECOND launch to
//     restore it.
//   * The restore path is keyed by the app's own workspace identity
//     (`deserializeWorkspaceState(workspaceId, persistedSession, liveBackendIds)`
//     in `ui/src/lib/sessionPersistence.ts`, reached only through
//     `preloadWorkspaceSnapshots`/`useWorkspaceRestore` for a workspace the app
//     has registered). Seeding it would mean re-deriving the product's workspace
//     identity and persisting schema (`WORKSPACE_SESSION_VERSION = 3`) inside the
//     harness - i.e. fabricating the product's own state format, which is exactly
//     the kind of fixture that silently rots when the product changes.
//   * A seeded pane whose backend session is not live is restored with
//     `lifecycle: "exited"` and `backendSessionId: null` (the deserializer's own
//     liveness branch), so the scenario would be splitting a dead pane.
//
// Both routes were weighed against the frozen scenario contract, and route A
// satisfies it without weakening a single assertion:
//   * `split-create` (the product's own receipt) still names the authoritative
//     `backendSessionId` of the pane the split really created, and the
//     seven-field presentation assertion is bound to THAT session (the app's own
//     pane and the split pane both present frames, so the receipt is addressed by
//     session - `awaitReceiptForSession` - instead of by line index).
//   * `ptyCreatedCount === 1` is a PER-SESSION count
//     (`pty_created_counts(session_id)` in `src-tauri/src/terminal/qa_liveness.rs`
//     returns `creations_for(session_id)`), so the extra UI pane cannot inflate
//     the split session's count, and the marker assertion is bound to the session
//     the runner actually typed into (`awaitMarkerReceiptForSession`).
//   * The `fixture-setup` contract is untouched: the split scenarios still declare
//     and validate their `source` fixture (`SCENARIO_FIXTURE_REQUIREMENTS`), which
//     the product still provisions at boot; the pane is additional and is
//     recorded as its own evidence, never substituted for a fixture.

import { join } from 'node:path';
import { BUDGETS, HarnessError, requireSevenTupleReceipt } from './common-harness.mjs';
import {
  computeInventoryDelta,
  describeInventoryRead,
  readDaemonSessionInventory,
} from './daemon-inventory.mjs';

// The affordance the app itself exposes in its empty state, verbatim from the
// measured UIA tree ("New Terminal" [Button], `ui/src/components/EmptyWorkspaceView.tsx`).
export const PANE_AFFORDANCE_NAMES_WIN32 = Object.freeze(['New Terminal']);
export const PANE_AFFORDANCE_AUTOMATION_IDS_WIN32 = Object.freeze([]);
export const PANE_AFFORDANCE_SELECTOR_DARWIN = Object.freeze({ role: 'button', title: 'New Terminal' });
export const PANE_PRESENTATION_RECEIPT = 'presentation';

// The session a receipt is about: the presentation receipt carries the product's
// own `attachTuple.backendSessionId`; the marker receipt carries `sessionId`
// (`marker_output_payload` in `src-tauri/src/terminal/qa_liveness.rs`).
export function paneReceiptSessionId(receipt) {
  const id = receipt?.attachTuple?.backendSessionId ?? receipt?.sessionId;
  return typeof id === 'string' && id.length > 0 ? id : null;
}

// Every session named by the settled lines of one receipt stream. Used for the
// absence and ambiguity reports: a blocked run must say which sessions really
// settled instead of only that the expected one did not.
export function observedSessionIds(barrierHub, receiptName = PANE_PRESENTATION_RECEIPT) {
  const ids = new Set();
  for (const line of barrierHub.receiptLines(receiptName)) {
    const id = paneReceiptSessionId(line);
    if (id) ids.add(id);
  }
  return [...ids];
}

// The pane step's measured second source: a bounded, READ-ONLY reader of the
// isolated daemon's own session inventory (`daemon-inventory.mjs`). The
// before-read is taken BEFORE the pane click; `readAfter` is a function rather
// than a snapshot because the honest moment for the after-read is the moment the
// delta is actually needed - after the receipt window - when the session the
// click created has had time to register with the daemon. Both reads are
// recorded as their own evidence actions, and every blocking step inside them is
// bounded, so a wedged daemon leaves a typed reason instead of a hang.
export function createPaneInventoryReader({
  isolationRoot = null,
  platform = process.platform,
  runtimeDir = null,
  evidence = null,
  budget = null,
  readInventory = readDaemonSessionInventory,
} = {}) {
  const dir = runtimeDir ?? (isolationRoot === null ? null : join(isolationRoot, 'runtime'));
  if (typeof dir !== 'string' || dir.length === 0) {
    throw new HarnessError('ASSERTION_FAILURE', 'createPaneInventoryReader requires an isolationRoot or an explicit runtimeDir');
  }
  const snapshot = async label => {
    // Charged to the caller's pre-trigger setup budget (the pane step is setup,
    // not the measured attempt), capped by whatever the reader's own default is.
    const boundedTotalMs = budget && typeof budget.consume === 'function'
      ? budget.consume(BUDGETS.daemonInventoryTotalMs, label)
      : undefined;
    const result = await readInventory({
      runtimeDir: dir,
      platform,
      ...(boundedTotalMs === undefined ? {} : { totalMs: boundedTotalMs }),
    });
    // The daemon's bearer token is never part of this record: only its path is.
    evidence?.action?.({
      action: label,
      ok: result.ok === true,
      code: result.code ?? null,
      detail: result.detail ?? null,
      transport: result.transport ?? null,
      endpoint: result.endpoint ?? null,
      sessionCount: result.ok === true ? result.sessions.length : null,
      sessionIds: result.ok === true ? [...result.sessions].sort() : null,
      epoch: result.epoch ?? null,
      elapsedMs: result.elapsedMs ?? null,
    });
    return result;
  };
  return { runtimeDir: dir, platform, snapshot };
}

// Bind the pane the UI step just created to the session the app itself presents.
//
// TWO SOURCES, ONE TYPED DECISION, and which one settled it is recorded:
//
//   1. `presentation-tuple` - the product's own receipt. `fixture` is the settled
//      `fixture-setup` payload: its sessions are daemon sessions the product
//      created before any pane existed, so a presentation naming one of them
//      cannot be the pane this step opened. They are excluded, and a second
//      distinct non-fixture session is reported as ambiguous rather than guessed
//      at.
//   2. `inventory-delta` - a MEASURED daemon inventory delta, used only when the
//      receipt settles no session. The pre-split pane's presentation producer
//      requires the seven-field attachTuple and emits nothing without it, and
//      three of those fields are frontend-owned identities the daemon has no
//      concept of, so the fallback carries the ONE thing the daemon really knows:
//      the backend session id the click created, measured as (after minus before)
//      minus the settled fixture sessions. The same one-session guard applies with
//      the same typed code - zero added sessions is `PANE_BINDING_UNBOUND`, more
//      than one is `PANE_BINDING_AMBIGUOUS` - and it never binds to whichever id
//      sorted first. The delta path omits the three frontend-owned fields rather
//      than inventing them.
export async function bindPaneSession({
  evidence = null,
  barrierHub,
  fixture = null,
  inventory = null,
  receiptName = PANE_PRESENTATION_RECEIPT,
  timeoutMs = BUDGETS.paneBindingReadyMs,
} = {}) {
  if (!barrierHub || typeof barrierHub.awaitReceiptMatching !== 'function') {
    throw new HarnessError('ASSERTION_FAILURE', 'bindPaneSession requires a barrier hub with awaitReceiptMatching');
  }
  const fixtureIds = new Set(
    (fixture?.sessions ?? [])
      .map(session => session?.backendSessionId)
      .filter(id => typeof id === 'string' && id.length > 0),
  );

  // SOURCE 1 (preferred): the product's own presentation receipt.
  let matched = null;
  let receiptGap = null;
  try {
    matched = await barrierHub.awaitReceiptMatching(receiptName, {
      timeoutMs,
      label: 'pane binding',
      match: record => {
        const id = paneReceiptSessionId(record);
        return id !== null && !fixtureIds.has(id);
      },
    });
  } catch (error) {
    if (error?.code !== 'BARRIER_ACK_TIMEOUT') throw error;
    // The block has to say WHICH stream it read and WHAT that stream held.
    // "no pane was created" (an absent or empty stream) and "a pane was
    // created but never presented" (lines that named only fixture sessions)
    // are different defects, and the observed session list alone cannot tell
    // them apart. Fail-closed behaviour is unchanged: the same typed code, the
    // same decision, only more of the measurement in the message.
    const namedSessions = observedSessionIds(barrierHub, receiptName);
    const receiptFile = typeof barrierHub.receiptPath === 'function'
      ? `"${barrierHub.receiptPath(receiptName)}"`
      : 'unknown';
    const receiptLines = typeof barrierHub.receiptLineCount === 'function'
      ? barrierHub.receiptLineCount(receiptName)
      : 'unknown';
    receiptGap = `no ${receiptName} receipt named a session other than the ${fixtureIds.size} fixture session(s) within ${timeoutMs}ms, so the UI pane step created no observable pane (receipt: ${receiptFile}, receipt lines: ${receiptLines}, sessions named by those lines: ${JSON.stringify(namedSessions)}, fixture sessions excluded: ${JSON.stringify([...fixtureIds])}, observed pane sessions: ${JSON.stringify(namedSessions.filter(id => !fixtureIds.has(id)))})`;
  }

  if (matched) {
    const attachTuple = requireSevenTupleReceipt(matched, {}, 'pane binding');
    const observed = observedSessionIds(barrierHub, receiptName).filter(id => !fixtureIds.has(id));
    if (observed.length > 1) {
      throw new HarnessError('PANE_BINDING_AMBIGUOUS', `one UI pane step settled ${observed.length} distinct pane sessions (${JSON.stringify(observed)}); the scenario cannot say which pane it split`);
    }
    const binding = {
      action: 'pane-session-bound',
      settledBy: 'presentation-tuple',
      source: `${receiptName}-receipt`,
      backendSessionId: attachTuple.backendSessionId,
      frontendSessionId: attachTuple.frontendSessionId,
      paneIdentity: attachTuple.paneIdentity,
      bindingKey: attachTuple.bindingKey,
      incarnation: attachTuple.incarnation,
      daemonEpoch: String(attachTuple.daemonEpoch),
      attemptGeneration: attachTuple.attemptGeneration,
      fixtureSessionIds: [...fixtureIds],
      observedPaneSessionIds: observed,
    };
    evidence?.action?.(binding);
    return binding;
  }

  // SOURCE 2 (fallback): a measured daemon inventory delta. The delta is only
  // ever computed from two REAL reads; when a read could not be taken the
  // failure is typed and says which read failed and why - it is never guessed.
  if (!inventory || typeof inventory.readAfter !== 'function') {
    throw new HarnessError('PANE_BINDING_UNBOUND', `${receiptGap}; no daemon inventory reader was wired into the pane step, so no measured session identity was available either`);
  }
  const before = inventory.before ?? null;
  let after = null;
  try {
    after = await inventory.readAfter();
  } catch (error) {
    throw new HarnessError('PANE_BINDING_UNBOUND', `${receiptGap}; the daemon inventory delta could not be measured: the after-read threw ${JSON.stringify(String(error?.message ?? error))} (before-read ${describeInventoryRead(before)})`);
  }
  if (before?.ok !== true || after?.ok !== true) {
    throw new HarnessError('PANE_BINDING_UNBOUND', `${receiptGap}; the daemon inventory delta could not be measured: before-read ${describeInventoryRead(before)}, after-read ${describeInventoryRead(after)}`);
  }
  const delta = computeInventoryDelta({ before, after, fixtureSessionIds: [...fixtureIds] });
  // A read result that is not shaped like a session list is reported as what it
  // is; the failure path must never itself throw.
  const idsOf = read => (Array.isArray(read?.sessions) ? [...read.sessions].sort() : null);
  if (delta.added.length === 0) {
    throw new HarnessError('PANE_BINDING_UNBOUND', `${receiptGap}; the measured daemon inventory delta named no new session either (before-read ${delta.beforeCount} session(s) ${JSON.stringify(idsOf(before))}, after-read ${delta.afterCount} session(s) ${JSON.stringify(idsOf(after))}, added [], removed ${JSON.stringify(delta.removed)}, fixture sessions excluded ${JSON.stringify(delta.fixtureExcluded)})`);
  }
  if (delta.added.length > 1) {
    // The same guard the receipt path applies, with the same typed code: the
    // scenario will not bind to whichever session sorted first.
    throw new HarnessError('PANE_BINDING_AMBIGUOUS', `one UI pane step added ${delta.added.length} distinct pane sessions to the measured daemon inventory (${JSON.stringify(delta.added)}); the scenario cannot say which pane it split (before-read ${delta.beforeCount} session(s), after-read ${delta.afterCount} session(s), removed ${JSON.stringify(delta.removed)}, fixture sessions excluded ${JSON.stringify(delta.fixtureExcluded)})`);
  }
  const binding = {
    action: 'pane-session-bound',
    settledBy: 'inventory-delta',
    source: 'daemon-inventory-delta',
    backendSessionId: delta.added[0],
    fixtureSessionIds: [...fixtureIds],
    observedPaneSessionIds: [...delta.added],
    receiptGap,
    inventoryDelta: {
      transport: after.transport,
      endpoint: after.endpoint,
      epoch: after.epoch,
      daemonVersion: after.daemonVersion ?? null,
      beforeCount: delta.beforeCount,
      afterCount: delta.afterCount,
      added: delta.added,
      removed: delta.removed,
      fixtureExcluded: delta.fixtureExcluded,
      beforeSessionIds: idsOf(before),
      afterSessionIds: idsOf(after),
    },
  };
  evidence?.action?.(binding);
  return binding;
}

// The marker the runner typed into a pane must be reported by THAT pane's
// session. A receipt from another pane is not evidence about the pane under
// test, so an unbound marker is a typed failure rather than a pass that rode
// someone else's output.
export async function awaitMarkerReceiptForSession(barrierHub, sessionId, timeoutMs, label = 'marker output') {
  if (!barrierHub || typeof barrierHub.awaitReceiptForSession !== 'function') {
    throw new HarnessError('ASSERTION_FAILURE', `awaitMarkerReceiptForSession(${label}) requires a barrier hub with awaitReceiptForSession`);
  }
  try {
    return await barrierHub.awaitReceiptForSession('marker-output', sessionId, timeoutMs, label);
  } catch (error) {
    if (error?.code === 'BARRIER_ACK_TIMEOUT') {
      const observed = [...new Set(
        barrierHub.receiptLines('marker-output')
          .map(record => record?.sessionId)
          .filter(id => typeof id === 'string' && id.length > 0),
      )];
      throw new HarnessError('MARKER_SESSION_UNBOUND', `${label}: no marker-output receipt named the pane under test (${sessionId}) within ${timeoutMs}ms; observed marker sessions: ${JSON.stringify(observed)}`);
    }
    throw error;
  }
}
