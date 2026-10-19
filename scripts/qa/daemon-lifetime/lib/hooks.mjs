// Product hooks the harness depends on, probed from the checkout that the QA bundles are built
// from. A scenario whose hook is missing is reported BLOCKED with the exact barrier instead of
// being run against a build that cannot exercise it. Probes read source only; they never build.

import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

/**
 * Each hook: files (relative to the checkout), a pattern that must match in at least one of them,
 * the plan todo that owns it, and the scenarios that need it. platforms limits where it matters.
 */
export const HOOKS = Object.freeze([
  { id: 'listenerStall', owner: 'todo 12 (ipc/qa_barrier.rs QaFixtureKind::DaemonListenerStall)', files: ['src-tauri/src/ipc/qa_barrier.rs'], pattern: /DaemonListenerStall/, scenarios: ['Q04'] },
  { id: 'probeFault', owner: 'todo 12 (OwnerProbe reads FERRYX_QA_PROBE_FAULT under local-split-qa)', files: ['src-tauri/src/daemon/owner_admission.rs'], pattern: /FERRYX_QA_PROBE_FAULT/, scenarios: ['Q05'] },
  { id: 'buildSalt', owner: 'todo 12 (daemon/qa_build_salt.rs: option_env!("FERRYX_QA_BUILD_SALT") #[used] static under local-split-qa)', files: ['src-tauri/src/daemon/qa_build_salt.rs'], pattern: /#\[used\][\s\S]*option_env!\("FERRYX_QA_BUILD_SALT"\)/, scenarios: ['Q07', 'Q08'] },
  { id: 'protocolBump', owner: 'todo 9 (Cargo feature qa-protocol-bump -> DAEMON_PROTOCOL_VERSION 6)', files: ['src-tauri/Cargo.toml'], pattern: /^qa-protocol-bump\s*=/m, scenarios: ['Q08b', 'Q09'] },
  { id: 'disableControl', owner: 'todo 15 (FERRYX_QA_DISABLE_CONTROL)', files: ['src-tauri/src/daemon/control_server.rs'], pattern: /FERRYX_QA_DISABLE_CONTROL/, scenarios: ['Q09'] },
  { id: 'handoverCandidate', owner: 'todo 8 (handover-request.json + start_handover_candidate)', files: ['src-tauri/src/daemon/handover_request.rs', 'src-tauri/src/daemon/supervisor.rs'], pattern: /start_handover_candidate/, scenarios: ['Q08', 'Q08b'], platforms: ['mac', 'linux'] },
  { id: 'authorityTransfer', owner: 'todo 8 (candidate adopts listener and lock fds via request_authority/recv_authority before commit)', files: ['src-tauri/src/daemon/server.rs'], pattern: /(request|recv)_authority\s*\(/, scenarios: ['Q08', 'Q08b'], platforms: ['mac', 'linux'] },
  { id: 'windowsPending', owner: 'todo 9 (IpcErrorCode::DaemonUpdatePending)', files: ['src-tauri/src/ipc/error.rs'], pattern: /DaemonUpdatePending/, scenarios: ['Q08b', 'Q09'], platforms: ['windows'] },
  { id: 'writeAckHoldArm', owner: 'todo 12 (qa_producers.rs WRITE_ACK_HOLD_ARM: <barrier>/write-ack-hold.request.json / .release.json { runId, operationId, sessionId } -> write-ack-hold.receipt.jsonl)', files: ['src-tauri/src/daemon/qa_producers.rs'], pattern: /pub const WRITE_ACK_HOLD_ARM: &str = "write-ack-hold"/, scenarios: ['Q11'] },
  { id: 'macPlistForwardsIsolationEnv', owner: 'todo 4 (QA plist must carry FERRYX_DATA_DIR/RUNTIME_DIR/SESSION_DIR, else the job daemon uses the dev runtime)', files: ['src-tauri/src/daemon/launchd.rs'], pattern: /EnvironmentVariables/, scenarios: ['Q00'], platforms: ['mac'], gatesAll: true },
  { id: 'jobForwardsQaBarrier', owner: 'todos 4-6 (job daemon receives FERRYX_QA_BARRIER_DIR/FERRYX_QA_RUN_ID under local-split-qa)', files: ['src-tauri/src/daemon/launchd.rs', 'src-tauri/src/daemon/supervisor_linux.rs', 'src-tauri/src/daemon/supervisor_windows.rs'], pattern: /FERRYX_QA_BARRIER_DIR/, scenarios: ['Q04', 'Q11'] },
  { id: 'handoverFaultArm', owner: 'todo 8 (qa_producers.rs HANDOVER_FAULT_REQUEST: <barrier>/handover-fault.request.json { runId, operationId, fault } read at PrepareHandover and at candidate boot)', files: ['src-tauri/src/daemon/qa_producers.rs'], pattern: /HANDOVER_FAULT_REQUEST: &str = "handover-fault\.request\.json"/, scenarios: ['Q08'], platforms: ['mac', 'linux'] },
  { id: 'candidateAcceptCounters', owner: 'todo 8 (qa_producers.rs note_authority_stage: per-stage acceptCounts of every inherited listener in handover-authority.receipt.jsonl)', files: ['src-tauri/src/daemon/qa_producers.rs'], pattern: /pub const AUTHORITY_STAGE: &str = "handover-authority"[\s\S]*"acceptCounts": accept_counts/, scenarios: ['Q08', 'Q08b'], platforms: ['mac', 'linux'] },
  { id: 'forceFallback', owner: 'todo 12 (FERRYX_QA_FORCE_FALLBACK=1 under local-split-qa routes connect_or_spawn to the detached fallback)', files: ['src-tauri/src/daemon/owner_admission.rs'], pattern: /FORCE_FALLBACK_ENV: &str = "FERRYX_QA_FORCE_FALLBACK"/, scenarios: ['Q06', 'Q06w'] },
  { id: 'detachGuard', owner: 'todo 6 carry-over (Windows detach guard returns DAEMON_DETACH_UNAVAILABLE inside a no-breakaway job)', files: ['src-tauri/src/daemon/client.rs'], pattern: /code = "DAEMON_DETACH_UNAVAILABLE"/, scenarios: ['Q06w'], platforms: ['windows'] },
]);

/** todo 8's FERRYX_QA_HANDOVER_FAULT values (daemon/qa_producers.rs on feat/daemon-lifetime-w4-t8). */
export const HANDOVER_FAULT_ENV = 'FERRYX_QA_HANDOVER_FAULT';
export const HANDOVER_FAULTS = Object.freeze(['decision-write-failure', 'drop-commit-ok-before-record', 'drop-commit-ok-after-record', 'abort-after-adopt']);

export function probeHooks(checkout, platform, readFile = (f) => readFileSync(f, 'utf8'), exists = existsSync) {
  const result = {};
  for (const hook of HOOKS) {
    if (hook.platforms && !hook.platforms.includes(platform)) {
      result[hook.id] = { present: true, notApplicable: true, owner: hook.owner };
      continue;
    }
    const matched = hook.files.filter((f) => {
      const full = path.join(checkout, f);
      return exists(full) && hook.pattern.test(readFile(full));
    });
    result[hook.id] = { present: matched.length > 0, matchedIn: matched, owner: hook.owner, scenarios: hook.scenarios, gatesAll: Boolean(hook.gatesAll) };
  }
  return result;
}

/** Missing hooks a scenario depends on (empty = runnable). */
export function missingHooksFor(scenarioId, hookResult) {
  return HOOKS.filter((h) => {
    const r = hookResult[h.id];
    if (!r || r.notApplicable || r.present) return false;
    return h.gatesAll || h.scenarios.includes(scenarioId);
  }).map((h) => ({ id: h.id, owner: h.owner }));
}
