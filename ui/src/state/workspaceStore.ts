import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { initialGridFields, measureInitialGeometry, type InitialGeometryTarget, type InitialPtyGrid } from "../lib/initialPtyGeometry";

import {
  activityStateToAgentState,
  summarizeActivities,
  type ActivitySummary,
  type TerminalActivity,
  type TerminalActivityState,
} from "../lib/activity";
import { resolveAgentLogo } from "../lib/agentIcon";
import { agentDisplayNameForType, classifyTerminalTitleActivity, formatTabLabelFromTitle, isBareAgentTitle, normalizeTerminalTitle, parseAgentTitle } from "../lib/agentTitle";
import { workspaceName } from "../lib/branchFilter";
import { closeBrowser, createBrowser, createPopupBrowser, navigateBrowser, reloadBrowser, type BrowserReloadOptions, type BrowserSessionCreatedPayload } from "../lib/browserTauri";
import { closeTerminal, describeTerminal, DEFAULT_WORKSPACE_ID, discoverAgentProviderSession, getTerminalCwd, onNativeTerminalAgentState, onNativeTerminalBell, onNativeTerminalFocus, onNativeTerminalTitle, spawnTerminal, toIpcError, waitForTerminalExit, type SpawnTerminalRequest } from "../lib/tauri";
import * as tauriIpc from "../lib/tauri";
import { ensureTerminalEvents, terminalEventBus } from "../lib/terminalEvents";
import { isAbsoluteTerminalCwd } from "../lib/terminalCwd";
import { switchDebug } from "../lib/switchDebug";
import { withTimeout } from "../lib/withTimeout";
import { isPairedWorkspaceId, isRemoteWorkspaceId, resolvePairedStartup } from "../lib/remoteProject";
import { createStandbyBackendSessionId, REMOTE_SESSION_GONE_REASON } from "../lib/sessionLifecycle";
import { findGroupForWorkspace, groupProjects } from "../lib/projectGrouping";
import { hasValidProjectTarget } from "../lib/projectIdentity";
import { getCachedSshHosts } from "../lib/sshHosts";
import { getMigratedItem, PROJECTS_STORAGE_KEY } from "../lib/storageKeys";
import { startSshRecovery } from "../lib/sshRecovery";
import { canCaptureAuthoritativeProviderSession } from "../lib/agentResume";
import { getAgentReconnectAffordance } from "../lib/agentResumeAffordance";
import { getNativeWindowFocused } from "../lib/nativeWindowFocus";
import { isWindowForegroundFocused } from "../lib/notificationCoordinator";
import { listFilePreviewIds, releaseFilePreview, retainFilePreview } from "../lib/filePreviewTabRegistry";
import type { FilePreviewOpenRequest, FilePreviewSource } from "../lib/filePreviewTypes";
import {
  LocalSplitLifecycle,
  localSplitIntent,
  registerLocalSplit,
  unregisterLocalSplit,
  cancelLocalSplit,
  hasLocalSplit,
  type LocalSplitSession,
} from "../lib/localSplitLifecycle";
import { createBrowserPaneContent, isManagedBranchPrefix, isTerminalTab, worktreeIdentity } from "../lib/types";
import type {
  AgentProviderSession,
  AgentBlockedKind,
  AgentOutcome,
  ActiveAgent,
  BrowserState,
  BrowserTab,
  LayoutState,
  PaneContent,
  ReconnectLifecycle,
  RegisteredProject,
  StructuredIpcError,
  TerminalLifecycle,
  TerminalLifecyclePayload,
  ChangesTab,
  FileTab,
  RemoteDesktopTab,
  SessionRecoveryState,
  TerminalSession,
  TerminalTab,
  WorkspaceTab,
  Worktree,
  WorktreeIdentity,
} from "../lib/types";
import { createLayoutState, getFocusedGroupId, getGroupForTab, getTabsForGroup, layoutReducer } from "./layout";
import { collectLeafIds, type PaneDirection, type ResolvedSeam } from "./paneTree";
import { moveTabIntoPaneSplit } from "./tabPaneDrop";
import { notificationCenterStore, type NotificationCenterStore } from "../lib/notificationCenter/notificationCenterStore";
import { hasRecentObservationInteraction } from "../lib/notificationCenter/activityRecording";
import { countAttentionEntries, liveActivityLookup } from "../features/ferryx/attention/attentionModel";
import { formatRecoveryKey, RecoveryScheduler, type AuthorityCorrectionReceipt, type RecoveryClock } from "../lib/sessionRecovery";

const LAST_TAB_EXIT_TIMEOUT_MS = 5_000;
export type LocalRecoveryAdmissionResult =
  | { kind: "pending" }
  | { kind: "ready"; session: TerminalSession }
  | { kind: "blocked"; reason: "identity_conflict" | "SESSION_EXPIRED" | "CONTROL_CONFLICT" |
      "STALE_GENERATION" | "PAIRED_HOST_STALE_GENERATION" | "PAIRED_HOST_UNAUTHORIZED" |
      "UNAUTHORIZED" | "PERMISSION_DENIED" | "PAIRED_HOST_WRONG_MACHINE" |
      "PAIRED_HOST_INVALID_RESPONSE" | "PAIRED_HOST_MIGRATION_PENDING" | "UNSUPPORTED_PROTOCOL" }
  | { kind: "exited" }
  | { kind: "cancelled" };
type LocalRecoveryOwner = { workspaceId: string; sessionId: string; captured: TerminalSession;
  scheduler: RecoveryScheduler; admitting: boolean; requesters: Set<(result: LocalRecoveryAdmissionResult) => void> };
const localRecoveryOwners = new Map<string, LocalRecoveryOwner>();
const localRecoveryReceipts = new Map<string, Map<string, TerminalSession>>();
function hasPendingLocalSplit(session: TerminalSession | undefined | null): boolean {
  const intent = localSplitIntent(session);
  return Boolean(intent && (!intent.ready || intent.cancelRequested));
}
function matchesLocalRecoveryReceipt(current: TerminalSession | undefined, receipt: TerminalSession): boolean {
  return Boolean(current && current.id === receipt.id && current.workspaceId === receipt.workspaceId &&
    current.backendSessionId === receipt.backendSessionId && current.incarnation === receipt.incarnation &&
    current.daemonEpoch === receipt.daemonEpoch && current.recoveryKey === receipt.recoveryKey &&
    current.recoveryToken === receipt.recoveryToken && current.lifecycle === "running" &&
    current.reconnectLifecycle === "idle" && current.remoteConnectionState === receipt.remoteConnectionState &&
    !current.backendUnavailableReason && !current.reconnectError && !hasPendingLocalSplit(current));
}
const localRecoveryAdmissions = new Map<string, (action: WorkspaceAction) => void>();
let persistLocalRecovery: ((workspaceId: string, state: WorkspaceState, current: () => boolean) => Promise<void>) | undefined;
let localRecoveryClock: RecoveryClock | undefined;
let reconcilingLocalRecovery = false;

export function setLocalRecoveryClockForTests(clock?: RecoveryClock): void {
  localRecoveryClock = clock;
}

export function resetLocalRecoveryForTests(): void {
  for (const owner of localRecoveryOwners.values()) owner.scheduler.dispose();
  localRecoveryOwners.clear();
  localRecoveryReceipts.clear();
  localRecoveryAdmissions.clear();
  persistLocalRecovery = undefined;
  localRecoveryClock = undefined;
  reconcilingLocalRecovery = false;
}

if (import.meta.hot) import.meta.hot.dispose(() => resetLocalRecoveryForTests());

export function setLocalRecoveryPersistence(persist: typeof persistLocalRecovery): void {
  persistLocalRecovery = persist;
  reconcileLoadedLocalRecovery();
}

function applyLocalRecovery(workspaceId: string, action: WorkspaceAction): void {
  const admission = localRecoveryAdmissions.get(workspaceId);
  const snapshot = getWorkspaceSnapshot(workspaceId);
  if (!snapshot) return;
  const next = workspaceReducer(snapshot, action);
  setHmrWorkspaceState(workspaceId, next);
  setWorkspaceSnapshot(workspaceId, next);
  admission?.(action);
}

export function wakeLocalSessionRecovery(workspaceId: string, sessionId?: string): void {
  if (sessionId) {
    const snapshot = getWorkspaceSnapshot(workspaceId);
    const session = snapshot?.sessions[sessionId];
    if (!session || session.lifecycle === "exited" || session.backendUnavailableReason === "identity_conflict" ||
      session.reconnectError?.code === "IDENTITY_CONFLICT" || session.remoteConnectionState !== "reconnecting") return;
    if (snapshot && session && !isPairedWorkspaceId(workspaceId) && !isRemoteWorkspaceId(workspaceId) && session.backendSessionId) {
      setWorkspaceSnapshot(workspaceId, { ...snapshot, sessions: { ...snapshot.sessions,
        [sessionId]: { ...session, remoteConnectionState: "reconnecting" },
      } });
    }
  }
  reconcileLoadedLocalRecovery();
  for (const owner of localRecoveryOwners.values()) {
    if (owner.workspaceId === workspaceId && (!sessionId || owner.sessionId === sessionId) &&
      getWorkspaceSnapshot(workspaceId)?.sessions[owner.sessionId]?.backendUnavailableReason !== "identity_conflict") owner.scheduler.wake();
  }
}

export function requestLocalRecoveryAdmission(workspaceId: string, frontendSessionId: string,
  revalidate: boolean, onResult: (result: LocalRecoveryAdmissionResult) => void): () => void {
  const snapshot = getWorkspaceSnapshot(workspaceId);
  const retained = snapshot?.sessions[frontendSessionId];
  if (!retained || isRemoteWorkspaceId(workspaceId) || isPairedWorkspaceId(workspaceId)) {
    onResult({ kind: "cancelled" }); return () => {};
  }
  if (retained.lifecycle === "exited") {
    onResult({ kind: "exited" }); return () => {};
  }
  if (!retained.backendSessionId) {
    onResult({ kind: "cancelled" }); return () => {};
  }
  if (!revalidate && (retained.backendUnavailableReason === "identity_conflict" || retained.reconnectError?.code === "IDENTITY_CONFLICT")) {
    if (retained.remoteConnectionState === "reconnecting") {
      const snap = getWorkspaceSnapshot(workspaceId);
      if (snap?.sessions[frontendSessionId]) {
        reconcilingLocalRecovery = true;
        try {
          setWorkspaceSnapshot(workspaceId, {
            ...snap,
            sessions: {
              ...snap.sessions,
              [frontendSessionId]: { ...snap.sessions[frontendSessionId], remoteConnectionState: undefined },
            },
          });
        } finally {
          reconcilingLocalRecovery = false;
        }
      }
    }
    onResult({ kind: "blocked", reason: "identity_conflict" }); return () => {};
  }
  const receipt = localRecoveryReceipts.get(workspaceId)?.get(frontendSessionId);
  if (!revalidate && receipt && matchesLocalRecoveryReceipt(retained, receipt)) {
    onResult({ kind: "ready", session: retained }); return () => {};
  }
  let live = true;
  let owner: LocalRecoveryOwner | undefined;
  let unsubscribe = () => {};
  const finish = (result: LocalRecoveryAdmissionResult) => {
    if (!live) return;
    live = false; owner?.requesters.delete(finish); unsubscribe(); onResult(result);
  };
  const join = (candidate: LocalRecoveryOwner) => {
    if (!live || candidate.workspaceId !== workspaceId || candidate.sessionId !== frontendSessionId) return;
    owner = candidate; owner.requesters.add(finish);
  };
  unsubscribe = subscribeWorkspaceSnapshots(() => {
    if (!live || owner?.admitting) return;
    const current = getWorkspaceSnapshot(workspaceId)?.sessions[frontendSessionId];
    const expected = owner?.captured ?? retained;
    if (!current || current.backendSessionId !== expected.backendSessionId || current.incarnation !== expected.incarnation ||
      current.daemonEpoch !== expected.daemonEpoch || current.recoveryKey !== expected.recoveryKey ||
      current.recoveryToken !== expected.recoveryToken) finish({ kind: "cancelled" });
    else if (current.lifecycle === "exited") finish({ kind: "exited" });
  });
  onResult({ kind: "pending" });
  const key = JSON.stringify([workspaceId, formatRecoveryKey({ kind: "local", frontendSessionId,
    backendSessionId: retained.backendSessionId, expectedIncarnation: retained.incarnation })]);
  const existing = localRecoveryOwners.get(key);
  if (existing) {
    join(existing);
    if (revalidate && existing.scheduler.getPhase() === "blocked") {
      const capturedOwner = existing;
      const capturedToken = existing.scheduler.getToken();
      const capturedKey = existing.scheduler.getIdentityKey();
      void (async () => {
        try {
          const details = await describeTerminal(retained.backendSessionId!);
          if (!live) return;
          const currentTarget = getWorkspaceSnapshot(workspaceId)?.sessions[frontendSessionId];
          const ownerStillValid =
            localRecoveryOwners.get(key) === capturedOwner &&
            capturedOwner.scheduler.getToken() === capturedToken &&
            capturedOwner.scheduler.getIdentityKey() === capturedKey &&
            capturedOwner.scheduler.getPhase() === "blocked" &&
            Boolean(
              currentTarget &&
              currentTarget.backendSessionId === retained.backendSessionId &&
              currentTarget.incarnation === retained.incarnation &&
              currentTarget.daemonEpoch === retained.daemonEpoch &&
              currentTarget.recoveryKey === retained.recoveryKey &&
              currentTarget.recoveryToken === retained.recoveryToken &&
              currentTarget.lifecycle !== "exited"
            );
          if (!ownerStillValid) {
            finish({ kind: "cancelled" });
            return;
          }
          const authority = details?.recoveryAuthority;
          const resolvedDaemonEpoch = details?.daemonEpoch ?? authority?.daemonEpoch;
          const isAffirmativeOriginal = Boolean(
            details &&
            details.sessionId === retained.backendSessionId &&
            details.running &&
            resolvedDaemonEpoch &&
            retained.incarnation &&
            details.incarnation === retained.incarnation &&
            authority?.status === "running" &&
            authority.incarnation === retained.incarnation &&
            authority.daemonEpoch === resolvedDaemonEpoch
          );
          if (isAffirmativeOriginal) {
            const proof: AuthorityCorrectionReceipt = {
              currentToken: capturedToken,
              currentIdentityKey: capturedKey,
              revalidatedIdentity: {
                kind: "local",
                frontendSessionId,
                backendSessionId: retained.backendSessionId!,
                expectedIncarnation: retained.incarnation,
              },
              clearedConflict: true,
            };
            const unblocked = capturedOwner.scheduler.retryExplicit(proof);
            if (!unblocked) {
              const currentSnapshot = getWorkspaceSnapshot(workspaceId);
              const current = currentSnapshot?.sessions[frontendSessionId];
              if (currentSnapshot && current && current.remoteConnectionState === "reconnecting") {
                reconcilingLocalRecovery = true;
                try {
                  setWorkspaceSnapshot(workspaceId, {
                    ...currentSnapshot,
                    sessions: {
                      ...currentSnapshot.sessions,
                      [frontendSessionId]: { ...current, remoteConnectionState: undefined, backendUnavailableReason: "identity_conflict" },
                    },
                  });
                } finally {
                  reconcilingLocalRecovery = false;
                }
              }
              finish({ kind: "blocked", reason: "identity_conflict" });
            }
          } else {
            const currentSnapshot = getWorkspaceSnapshot(workspaceId);
            const current = currentSnapshot?.sessions[frontendSessionId];
            if (currentSnapshot && current && current.remoteConnectionState === "reconnecting") {
              reconcilingLocalRecovery = true;
              try {
                setWorkspaceSnapshot(workspaceId, {
                  ...currentSnapshot,
                  sessions: {
                    ...currentSnapshot.sessions,
                    [frontendSessionId]: { ...current, remoteConnectionState: undefined, backendUnavailableReason: "identity_conflict" },
                  },
                });
              } finally {
                reconcilingLocalRecovery = false;
              }
            }
            finish({ kind: "blocked", reason: "identity_conflict" });
          }
        } catch {
          if (live) finish({ kind: "blocked", reason: "identity_conflict" });
        }
      })();
    } else {
      existing.scheduler.wake();
    }
  }
  else {
    if (!persistLocalRecovery || hasPendingLocalSplit(retained)) { finish({ kind: "cancelled" }); return () => {}; }
    if (revalidate && (retained.backendUnavailableReason === "identity_conflict" || retained.reconnectError?.code === "IDENTITY_CONFLICT")) {
      void (async () => {
        try {
          const details = await describeTerminal(retained.backendSessionId!);
          if (!live) return;
          const currentSnapshot = getWorkspaceSnapshot(workspaceId);
          const currentTarget = currentSnapshot?.sessions[frontendSessionId];
          if (!currentSnapshot || !currentTarget) {
            finish({ kind: "cancelled" });
            return;
          }
          const targetStillMatches =
            currentTarget.backendSessionId === retained.backendSessionId &&
            currentTarget.incarnation === retained.incarnation &&
            currentTarget.daemonEpoch === retained.daemonEpoch &&
            currentTarget.recoveryKey === retained.recoveryKey &&
            currentTarget.recoveryToken === retained.recoveryToken &&
            currentTarget.lifecycle !== "exited";
          if (!targetStillMatches) {
            finish({ kind: "cancelled" });
            return;
          }
          const authority = details?.recoveryAuthority;
          const resolvedDaemonEpoch = details?.daemonEpoch ?? authority?.daemonEpoch;
          const isAffirmativeOriginal = Boolean(
            details &&
            details.sessionId === retained.backendSessionId &&
            details.running &&
            resolvedDaemonEpoch &&
            retained.incarnation &&
            details.incarnation === retained.incarnation &&
            authority?.status === "running" &&
            authority.incarnation === retained.incarnation &&
            authority.daemonEpoch === resolvedDaemonEpoch
          );
          if (isAffirmativeOriginal) {
            reconcilingLocalRecovery = true;
            try {
              setWorkspaceSnapshot(workspaceId, {
                ...currentSnapshot,
                sessions: {
                  ...currentSnapshot.sessions,
                  [frontendSessionId]: {
                    ...currentTarget,
                    remoteConnectionState: "reconnecting",
                    backendUnavailableReason: null,
                    reconnectError: undefined,
                  },
                },
              });
            } finally {
              reconcilingLocalRecovery = false;
            }
            reconcileLoadedLocalRecovery(join);
          } else {
            reconcilingLocalRecovery = true;
            try {
              if (currentTarget.remoteConnectionState === "reconnecting") {
                setWorkspaceSnapshot(workspaceId, {
                  ...currentSnapshot,
                  sessions: {
                    ...currentSnapshot.sessions,
                    [frontendSessionId]: { ...currentTarget, remoteConnectionState: undefined, backendUnavailableReason: "identity_conflict" },
                  },
                });
              }
            } finally {
              reconcilingLocalRecovery = false;
            }
            finish({ kind: "blocked", reason: "identity_conflict" });
          }
        } catch {
          if (live) {
            const currentSnapshot = getWorkspaceSnapshot(workspaceId);
            const currentTarget = currentSnapshot?.sessions[frontendSessionId];
            if (currentSnapshot && currentTarget && currentTarget.remoteConnectionState === "reconnecting") {
              reconcilingLocalRecovery = true;
              try {
                setWorkspaceSnapshot(workspaceId, {
                  ...currentSnapshot,
                  sessions: {
                    ...currentSnapshot.sessions,
                    [frontendSessionId]: { ...currentTarget, remoteConnectionState: undefined, backendUnavailableReason: "identity_conflict" },
                  },
                });
              } finally {
                reconcilingLocalRecovery = false;
              }
            }
            finish({ kind: "blocked", reason: "identity_conflict" });
          }
        }
      })();
      return () => { if (!live) return; live = false; owner?.requesters.delete(finish); unsubscribe(); };
    }
    reconcilingLocalRecovery = true;
    try {
      const currentSnapshot = getWorkspaceSnapshot(workspaceId) ?? snapshot!;
      const currentTarget = currentSnapshot.sessions[frontendSessionId] ?? retained;
      setWorkspaceSnapshot(workspaceId, { ...currentSnapshot, sessions: { ...currentSnapshot.sessions,
        [frontendSessionId]: { ...currentTarget, remoteConnectionState: "reconnecting" } } });
    } finally { reconcilingLocalRecovery = false; }
    reconcileLoadedLocalRecovery(join);
  }
  return () => { if (!live) return; live = false; owner?.requesters.delete(finish); unsubscribe(); };
}

function reconcileLoadedLocalRecovery(join?: (owner: LocalRecoveryOwner) => void): void {
  if (reconcilingLocalRecovery) return;
  reconcilingLocalRecovery = true;
  try {
    for (const [workspaceId, receipts] of localRecoveryReceipts) {
      const sessions = getWorkspaceSnapshot(workspaceId)?.sessions;
      for (const [sessionId, receipt] of receipts) {
        if (!matchesLocalRecoveryReceipt(sessions?.[sessionId], receipt)) receipts.delete(sessionId);
      }
      if (!receipts.size) localRecoveryReceipts.delete(workspaceId);
    }
    for (const [key, owner] of localRecoveryOwners) {
      const current = getWorkspaceSnapshot(owner.workspaceId)?.sessions[owner.sessionId];
      if (owner.admitting) continue;
      if (!current || current.backendSessionId !== owner.captured.backendSessionId ||
          current.incarnation !== owner.captured.incarnation || current.daemonEpoch !== owner.captured.daemonEpoch ||
          current.recoveryKey !== owner.captured.recoveryKey || current.recoveryToken !== owner.captured.recoveryToken ||
          current.lifecycle === "exited") {
        for (const requester of [...owner.requesters]) requester(current?.lifecycle === "exited" ? { kind: "exited" } : { kind: "cancelled" });
        owner.scheduler.dispose();
        localRecoveryOwners.delete(key);
      }
    }
    if (!persistLocalRecovery) return;
    for (const [workspaceId, state] of listWorkspaceSnapshots()) {
      if (isRemoteWorkspaceId(workspaceId) || isPairedWorkspaceId(workspaceId)) continue;
      for (const session of Object.values(state.sessions)) {
        if (!session.backendSessionId || hasPendingLocalSplit(session) || session.lifecycle === "exited" ||
            session.backendUnavailableReason === "identity_conflict" ||
            session.remoteConnectionState !== "reconnecting") continue;
        const identity = { kind: "local" as const, frontendSessionId: session.id,
          backendSessionId: session.backendSessionId, expectedIncarnation: session.incarnation };
        const key = JSON.stringify([workspaceId, formatRecoveryKey(identity)]);
        if (localRecoveryOwners.has(key)) continue;
        const owner: LocalRecoveryOwner = { workspaceId, sessionId: session.id, captured: session,
          scheduler: null! as RecoveryScheduler, admitting: false, requesters: new Set() };
        // Consecutive daemon answers that the backend does not exist. The canonical daemon answers
        // `session_not_found` only after consulting its own registry and every legacy route
        // (an unreachable predecessor answers `owner_unavailable` instead), so this is positive
        // absence, not a transport outage. Two answers in a row absorb a handover-adoption race.
        let absentStreak = 0;
        owner.scheduler = new RecoveryScheduler({ identity, clock: localRecoveryClock, attemptAction: async (attempt) => {
          const before = getWorkspaceSnapshot(workspaceId)?.sessions[session.id];
          if (!before || before.backendSessionId !== owner.captured.backendSessionId ||
              before.incarnation !== owner.captured.incarnation || before.daemonEpoch !== owner.captured.daemonEpoch ||
              before.recoveryKey !== owner.captured.recoveryKey || before.recoveryToken !== owner.captured.recoveryToken) return { kind: "cancelled" };
          const admissionKey = formatRecoveryKey(identity);
          const admissionToken = attempt.token;
          // Expect our own publication before synchronous snapshot listeners run. An
          // external replacement still differs from this captured admission identity.
          owner.captured = { ...before, recoveryKey: admissionKey, recoveryToken: admissionToken };
          applyLocalRecovery(workspaceId, { type: "LOCAL_RECOVERY_ATTEMPT", sessionId: session.id, key: admissionKey, token: admissionToken });
          const fresh = () => {
            const current = getWorkspaceSnapshot(workspaceId)?.sessions[session.id];
            return !attempt.isCancelled() && localRecoveryOwners.get(key) === owner && Boolean(current &&
              current.backendSessionId === session.backendSessionId && current.incarnation === session.incarnation &&
              current.daemonEpoch === session.daemonEpoch && current.lifecycle !== "exited" &&
              current.recoveryKey === admissionKey && current.recoveryToken === admissionToken);
          };
          if (!fresh()) return { kind: "cancelled" };
          let details: Awaited<ReturnType<typeof describeTerminal>>;
          try {
            details = await describeTerminal(session.backendSessionId!);
          } catch (error) {
            if (!fresh()) return { kind: "cancelled" };
            if (!isConfirmedLocalAbsence(error)) { absentStreak = 0; throw error; }
            absentStreak += 1;
            if (absentStreak < LOCAL_ABSENCE_CONFIRMATIONS) return { kind: "retryable", reason: "owner_unknown", error };
            applyLocalRecovery(workspaceId, { type: "SESSION_CONFIRMED_ABSENT", sessionId: session.id,
              backendSessionId: session.backendSessionId!, key: admissionKey, token: admissionToken });
            return getWorkspaceSnapshot(workspaceId)?.sessions[session.id]?.lifecycle === "exited"
              ? { kind: "exited" } : { kind: "cancelled" };
          }
          absentStreak = 0;
          if (!fresh()) return { kind: "cancelled" };
          if (!details || details.sessionId !== session.backendSessionId || details.isPairedProxy) return { kind: "retryable", reason: "owner_unknown" };
          if (session.incarnation && details.incarnation && session.incarnation !== details.incarnation) return { kind: "blocked", reason: "identity_conflict" };
          const authority = details.recoveryAuthority;
          if (authority?.status === "blocked" || authority?.status === "identity_conflict" || authority?.status === "correction_required") return { kind: "blocked", reason: "identity_conflict", error: authority };
          const resolvedDaemonEpoch = details.daemonEpoch ?? authority?.daemonEpoch;
          if (!session.incarnation || details.incarnation !== session.incarnation || !resolvedDaemonEpoch) return { kind: "retryable", reason: "owner_unknown" };
          if (authority?.status === "affirmative_exit" && authority.incarnation === session.incarnation &&
              authority.daemonEpoch === session.daemonEpoch) {
            applyLocalRecovery(workspaceId, { type: "SESSION_AFFIRMATIVE_EXIT", sessionId: session.id,
              backendSessionId: session.backendSessionId!, incarnation: session.incarnation, daemonEpoch: session.daemonEpoch,
              key: admissionKey, token: admissionToken, exitCode: authority.exitCode });
            return getWorkspaceSnapshot(workspaceId)?.sessions[session.id]?.lifecycle === "exited"
              ? { kind: "exited", exitCode: authority.exitCode } : { kind: "cancelled" };
          }
          if (!details.running || authority?.status !== "running" || authority.incarnation !== session.incarnation ||
              authority.daemonEpoch !== resolvedDaemonEpoch) return { kind: "retryable", reason: "owner_unknown" };
          const snapshot = getWorkspaceSnapshot(workspaceId)!;
          const action: WorkspaceAction = { type: "REBIND_SESSION_BACKEND", sessionId: session.id,
            backendSessionId: session.backendSessionId!, daemonEpoch: resolvedDaemonEpoch,
            incarnation: session.incarnation, recoveryKey: formatRecoveryKey(identity), recoveryToken: attempt.token };
          const next = workspaceReducer(snapshot, action);
          if (!fresh() || !persistLocalRecovery) return { kind: "cancelled" };
          await persistLocalRecovery(workspaceId, next, fresh);
          if (!fresh()) return { kind: "cancelled" };
          owner.admitting = true;
          try { applyLocalRecovery(workspaceId, action); }
          finally { owner.admitting = false; }
          const admitted = getWorkspaceSnapshot(workspaceId)?.sessions[session.id];
          if (admitted?.backendSessionId !== session.backendSessionId || admitted.incarnation !== session.incarnation ||
              admitted.lifecycle === "exited" || admitted.daemonEpoch !== resolvedDaemonEpoch || admitted.recoveryToken !== admissionToken ||
              admitted.recoveryKey !== admissionKey || admitted.remoteConnectionState === "reconnecting") return { kind: "cancelled" };
          owner.captured = admitted;
          return { kind: "ready" };
        }, onOutcomeChange: (outcome) => {
          if (outcome.phase === "ready") {
            const current = getWorkspaceSnapshot(workspaceId)?.sessions[session.id];
            if (!matchesLocalRecoveryReceipt(current, owner.captured)) {
              localRecoveryOwners.delete(key);
              for (const requester of [...owner.requesters]) requester({ kind: "cancelled" });
              return;
            }
            let receipts = localRecoveryReceipts.get(workspaceId);
            if (!receipts) { receipts = new Map(); localRecoveryReceipts.set(workspaceId, receipts); }
            receipts.set(session.id, owner.captured);
            localRecoveryOwners.delete(key);
            for (const requester of [...owner.requesters]) requester({ kind: "ready", session: owner.captured });
            return;
          }
          if (outcome.phase === "exited" || outcome.phase === "cancelled") {
            for (const requester of [...owner.requesters]) requester({ kind: outcome.phase });
            return;
          }
          if (outcome.phase !== "blocked") return;
          const current = getWorkspaceSnapshot(workspaceId)?.sessions[session.id];
          if (current?.recoveryKey !== outcome.identityKey || current.recoveryToken !== outcome.token) return;
          applyLocalRecovery(workspaceId, { type: "SESSION_BACKEND_UNAVAILABLE", sessionId: session.id,
            backendSessionId: session.backendSessionId!, attemptKey: outcome.identityKey, attemptToken: outcome.token,
            outcome: "blocked", reason: outcome.reason });
          for (const requester of [...owner.requesters]) requester({ kind: "blocked", reason: "identity_conflict" });
        } });
        localRecoveryOwners.set(key, owner);
        join?.(owner);
        owner.scheduler.start();
      }
    }
  } finally { reconcilingLocalRecovery = false; }
}

const unsubscribeLocalRecoverySnapshots = subscribeWorkspaceSnapshots(() => reconcileLoadedLocalRecovery());

const LOCAL_ABSENCE_CONFIRMATIONS = 2;

/**
 * The daemon's describe answered that it owns no such session after checking its registry and
 * every legacy route. Transport failures, timeouts and an unreachable predecessor
 * (`owner_unavailable`) stay retryable uncertainty (INV-1).
 */
function isConfirmedLocalAbsence(error: unknown): boolean {
  const ipc = toIpcError(error);
  return ipc.code === "SESSION_NOT_FOUND" && ipc.details?.source === "daemon_describe" &&
    ipc.details?.kind === "session_not_found";
}
if (import.meta.hot) import.meta.hot.dispose(() => {
  unsubscribeLocalRecoverySnapshots();
  resetLocalRecoveryForTests();
});
/** Lower bound between two fallback session-id probes for the same pane and agent. */
const PROVIDER_SESSION_PROBE_INTERVAL_MS = 2_000;

/** Manifest ids whose provider session the daemon can discover from the pane's process tree. */
export const PROVIDER_SESSION_PROBE_AGENTS: readonly string[] = [
  "claude", "codex", "copilot", "cursor", "cursor-agent", "kimi", "omo", "gjc", "antigravity",
  "opencode", "omp", "grok", "droid", "cline", "crush",
];

/**
 * Provider sessions arrive as freshly materialized IPC payloads, so reference equality is
 * meaningless: compare by value or every agent heartbeat churns the session map.
 */
function sameProviderSession(
  a: AgentProviderSession | null | undefined,
  b: AgentProviderSession | null | undefined,
): boolean {
  if (!a || !b) return !a && !b;
  return a.key === b.key && a.id === b.id && (a.transcriptPath ?? null) === (b.transcriptPath ?? null)
    && a.launcher === b.launcher;
}

/**
 * How long after arming an auto-resume attention suppression that suppression stays
 * effective. The resume blip (agent re-landing at its prompt) arrives within seconds of
 * the reconnect; a genuine first completion needs the user to notice, orient, and prompt,
 * so anything later must notify instead of being eaten.
 */
export const ATTENTION_SUPPRESSION_WINDOW_MS = 120_000;

type WorkspaceTerminalActivity = TerminalActivity;

export type WorkspaceState = {
  workspaceId?: string;
  worktrees: Worktree[];
  activeWorktreePath: string | null;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
  worktreeLayouts?: Record<string, LayoutState>;
  unreadTabIds: Record<string, boolean>;
  bellUnreadTabIds?: Record<string, boolean>;
  unreadWorktreePaths: Record<string, boolean>;
  /** Optional for backwards compatibility with persisted/test states created before activity tracking. */
  activityBySessionId?: Record<string, TerminalActivity>;
  attentionEpisodeAtBySession?: Record<string, number>;
  /**
   * Sessions whose NEXT attention transition is app-initiated noise (e.g. auto-resume
   * landing at a prompt). Values are the epoch-ms timestamp when suppression was armed;
   * legacy `true` reads as freshly armed. A resume blip lands within seconds, while a
   * genuine first completion needs a user round-trip — so only attention inside
   * `ATTENTION_SUPPRESSION_WINDOW_MS` of the arm is suppressed, and anything later
   * notifies (and still consumes the flag).
   */
  attentionSuppressions?: Record<string, number | boolean>;
};

export type WorkspaceServices = {
  ensureTerminalEvents: () => Promise<void>;
  spawnTerminal: (request: {
    workspaceId: string;
    worktree: WorktreeIdentity | null;
    cwd?: string | null;
    clientRequestId?: string | null;
    shell?: string | null;
    inheritFromSessionId?: string | null;
    startup?: SpawnTerminalRequest["startup"];
    cols?: number | null;
    rows?: number | null;
  }) => Promise<string>;
  /** Optional detailed spawn carrying the daemon-resolved cwd and the session identity; enables
   *  the single-hop split path and lets a spawned pane record its incarnation immediately. */
  spawnTerminalDetailed?: (request: {
    workspaceId: string;
    worktree: WorktreeIdentity | null;
    cwd?: string | null;
    clientRequestId?: string | null;
    shell?: string | null;
    inheritFromSessionId?: string | null;
    startup?: SpawnTerminalRequest["startup"];
    cols?: number | null;
    rows?: number | null;
  }) => Promise<{ sessionId: string; daemonEpoch?: string | null; session?: { cwd?: string | null; incarnation?: string | null } | null }>;
  /** Optional batch spawn for restore/recovery; falls back to per-session spawns when absent. */
  spawnTerminalsBatch?: (spawns: Array<Parameters<WorkspaceServices["spawnTerminal"]>[0]>) => Promise<Array<{ index: number; sessionId: string | null; error: string | null }>>;
  getTerminalCwd: (sessionId: string) => Promise<string | null>;
  closeTerminal: (sessionId: string) => Promise<void>;
  waitForTerminalExit: (sessionId: string, timeoutMs: number) => Promise<void>;
  splitOperation?: (request: import("../lib/types").SplitOperationRequest) => Promise<import("../lib/types").SplitOperationResponse>;
  splitCreate?: typeof tauriIpc.spawnTerminalDetailed;
  splitAttach?: typeof tauriIpc.attachTerminal;
  /** Measures the initial PTY grid for a pane about to be spawned; null when geometry is unavailable. */
  measureInitialGeometry?: (target: InitialGeometryTarget) => Promise<InitialPtyGrid | null>;
};

export type SplitPaneOptions = {
  position?: "first" | "second";
  content?: PaneContent;
};

export type TabSplitEdge = "left" | "right" | "top" | "bottom";

/** A backend session spawned by the caller before opening its tab. The daemon epoch and
 *  incarnation are required for the pane's durable attach tuple, so callers must pass the
 *  identity reported by the detailed spawn rather than a bare session id. */
export type PreSpawnedBackend = {
  sessionId: string;
  daemonEpoch: string | null;
  incarnation: string | null;
};

export {
  clearHmrWorkspaceState,
  getHmrWorkspaceState,
  setHmrWorkspaceState,
  type HmrWorkspaceStoreData,
} from "./hmrWorkspaceState";

import { getHmrWorkspaceState, setHmrWorkspaceState } from "./hmrWorkspaceState";
import { getWorkspaceSnapshot, listWorkspaceSnapshots, setWorkspaceSnapshot, subscribeWorkspaceSnapshots } from "./workspaceSnapshotCache";
import { getWorkspaceRestoreStatus } from "./workspaceRestore";

export type WorkspaceAction =
  | { type: "LOCAL_RECOVERY_ATTEMPT"; sessionId: string; key: string; token: number }
  | { type: "SET_WORKTREES"; worktrees: Worktree[] }
  | { type: "RESTORE_WORKSPACE"; state: WorkspaceState }
  | { type: "SELECT_WORKTREE"; path: string }
  | { type: "ADD_TAB_WITH_SESSION"; tab: WorkspaceTab; session?: TerminalSession; targetWorktreePath?: string }
  | {
      type: "CLOSE_TAB";
      tabId: string;
      replacement?: { tab: WorkspaceTab; session?: TerminalSession };
    }
  | { type: "UPDATE_BROWSER_TAB"; tabId: string; browserId?: string; updates: Partial<BrowserTab> }
  | { type: "ACTIVATE_TAB"; tabId: string }
  | { type: "UPDATE_FILE_TAB"; tabId: string; line: number | null; col: number | null; path?: string; remote?: FilePreviewOpenRequest["remote"] }
  | { type: "REORDER_TAB"; tabId: string; targetIndex: number }
  | { type: "RENAME_TAB"; tabId: string; label: string }
  | { type: "SET_TAB_PINNED"; tabId: string; pinned: boolean }
  | {
      type: "SPLIT_PANE";
      tabId: string;
      targetLeafId?: string;
      direction: PaneDirection;
      position?: "first" | "second";
      newLeafId: string;
      content?: PaneContent;
      session: TerminalSession;
    }
  | { type: "MOVE_TAB_TO_GROUP"; sourceTabId: string; targetGroupId: string; targetIndex?: number }
  | {
      type: "MOVE_TAB_TO_SPLIT";
      sourceTabId: string;
      targetGroupId: string;
      direction: PaneDirection;
      position?: "first" | "second";
    }
  | {
      type: "MOVE_TAB_TO_PANE_SPLIT";
      sourceTabId: string;
      targetTabId: string;
      targetLeafId: string;
      direction: PaneDirection;
      position?: "first" | "second";
    }
  | {
      type: "DETACH_PANE_TO_TAB";
      sourceTabId: string;
      leafId: string;
      newTab: TerminalTab;
      targetGroupId?: string;
      targetIndex?: number;
    }
  | {
      type: "CLOSE_PANE";
      tabId: string;
      leafId: string;
    }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | {
      type: "SET_PANE_RATIO";
      tabId: string;
      path: string;
      ratio: number;
      isolated?: boolean;
      seam?: ResolvedSeam | null;
    }
  | { type: "SET_TAB_GROUP_RATIO"; path: string; ratio: number }
  | { type: "EQUALIZE_TAB_GROUP_RUN"; path: string }
  | { type: "EQUALIZE_PANE_RUN"; tabId: string; path: string }
  | { type: "SWAP_PANES"; tabId: string; sourceLeafId: string; targetLeafId: string }
  | { type: "SESSION_LIFECYCLE"; backendSessionId: string; lifecycle: TerminalLifecycle; incarnation?: string | null; daemonEpoch?: string | null; exitCode?: number | null }
  | {
      type: "SESSION_BACKEND_UNAVAILABLE";
      sessionId: string;
      backendSessionId: string | null;
      spawnFailed?: boolean;
      bindingKey?: string | null;
      attemptKey?: string | null;
      attemptToken?: string | number | null;
      recoveryKey?: string | null;
      recoveryToken?: number | null;
      outcome?: "retryable" | "blocked";
      recoveryState?: SessionRecoveryState;
      reason: string;
    }
  | {
      type: "SESSION_AFFIRMATIVE_EXIT";
      sessionId: string;
      backendSessionId: string;
      incarnation?: string | null;
      daemonEpoch?: string | null;
      token?: number | null;
      key?: string | null;
      exitCode?: number | null;
      reason?: string | null;
    }
  | { type: "SESSION_CONFIRMED_ABSENT"; sessionId: string; backendSessionId: string; key: string; token: number }
  | { type: "LOCAL_SPLIT_UPDATE"; session: LocalSplitSession }
  | { type: "LOCAL_SPLIT_REMOVE"; sessionId: string }
  | {
      type: "LOCAL_SESSIONS_RECONCILED";
      live: import("./workspaceRestore").LocalLiveSessionMap | null;
    }
  | { type: "SESSION_REMOTE_STATUS"; status: import("../lib/types").SshRecoveryStatus; daemonEpoch?: string | null }
  /** The owning daemon confirmed this exact remote backend gone (paired affirmative exit/expiry). */
  | { type: "REMOTE_SESSION_GONE"; sessionId: string; backendSessionId: string }
  | {
      type: "SET_RECONNECT_LIFECYCLE";
      sessionId: string;
      lifecycle: ReconnectLifecycle;
      error?: StructuredIpcError | null;
      requestId?: string | null;
    }
  | { type: "APPLY_PROVIDER_SESSION"; sessionId: string; providerSession: AgentProviderSession; agentType?: string }
  | {
      type: "REBIND_SESSION_BACKEND";
      sessionId: string;
      backendSessionId: string;
      cwd?: string;
      daemonEpoch?: string | null;
      incarnation?: string | null;
      clearAgent?: boolean;
      pendingAttachment?: boolean;
      recoveryKey?: string | null;
      recoveryToken?: number | null;
    }
  | { type: "SESSION_TITLE_ACTIVITY"; tabId: string; sessionId: string; title: string; observed?: boolean }
  | {
      type: "SESSION_SCREEN_ACTIVITY";
      tabId: string;
      sessionId: string;
      state: "working" | "blocked" | "idle";
      ruleId: string;
      manifestId?: string;
      providerSession?: AgentProviderSession | null;
      detail?: string | null;
      isSnapshot?: boolean;
      outcome?: AgentOutcome | null;
      kind?: AgentBlockedKind | null;
      observed?: boolean;
    }
  | { type: "RESET_AGENT_STATE"; sessionId: string }
  | { type: "MARK_TAB_UNREAD"; tabId: string; observed?: boolean }
  | { type: "CLEAR_TAB_UNREAD"; tabId: string }
  | { type: "MARK_WORKTREE_UNREAD"; worktreePath: string }
  | { type: "CLEAR_WORKTREE_UNREAD"; worktreePath: string }
  | { type: "MARK_SESSION_ACTIVITY_SEEN"; sessionId: string }
  | { type: "SUPPRESS_NEXT_ATTENTION"; sessionId: string }
  | { type: "FOCUS_EXISTING_SESSION"; sessionId: string };

type UseWorkspaceStoreOptions = {
  workspaceId?: string;
  initialWorktrees?: Worktree[];
  services?: WorkspaceServices;
};

// Optional tauri IPC additions (M8/M9) resolved lazily so test doubles that mock
// "../lib/tauri" without them keep working; production always has both.
function optionalTauriFn(name: "spawnTerminalDetailed" | "spawnTerminalsBatch"): unknown {
  try {
    return (tauriIpc as Record<string, unknown>)[name];
  } catch {
    return undefined;
  }
}

const defaultServices: WorkspaceServices = {
  ensureTerminalEvents,
  spawnTerminal,
  get spawnTerminalDetailed() {
    return optionalTauriFn("spawnTerminalDetailed") as WorkspaceServices["spawnTerminalDetailed"];
  },
  get spawnTerminalsBatch() {
    return optionalTauriFn("spawnTerminalsBatch") as WorkspaceServices["spawnTerminalsBatch"];
  },
  getTerminalCwd,
  closeTerminal,
  waitForTerminalExit,
  get splitOperation() {
    return tauriIpc.spawnTerminalSplitOperation;
  },
  get splitCreate() {
    return tauriIpc.spawnTerminalDetailed;
  },
  get splitAttach() {
    return tauriIpc.attachTerminal;
  },
  measureInitialGeometry,
};

export function useWorkspaceStore({
  workspaceId = DEFAULT_WORKSPACE_ID,
  initialWorktrees = [],
  services = defaultServices,
}: UseWorkspaceStoreOptions = {}) {
  const initialRef = useRef<{ state: WorkspaceState; recoveredFromHmr: boolean } | null>(null);
  if (!initialRef.current) {
    initialRef.current = initWorkspaceState({ workspaceId, initialWorktrees });
  }
  const [state, reactDispatch] = useReducer(workspaceReducer, initialRef.current.state);
  const stateRef = useRef(state);
  const spawningSessionIdsRef = useRef(new Set<string>());
  /**
   * Bell subscribers keyed by nothing: the native bell event is global, so App registers one
   * listener here instead of a per-pane prop that only exists for the mounted foreground tab.
   */
  const bellListenersRef = useRef(new Set<(sessionId: string, tabId: string, target?: ActivityNotificationTarget) => void>());
  const activityListenersRef = useRef(new Set<(event: ActivityNotificationEvent) => void>());
  const mountedWorkspaceIdRef = useRef(workspaceId);
  // Snapshots of parked workspaces live outside React state, so a change there has to be announced
  // for the sidebar's cross-workspace activity to recompute.
  const [parkedActivityVersion, setParkedActivityVersion] = useState(0);
  const bumpParkedActivity = useCallback(() => setParkedActivityVersion((value) => value + 1), []);
  // Provenance belongs to the workspace currently mounted; keeping the first
  // one would make restore pick the wrong HMR-vs-disk path after a switch. It is
  // state (not a ref) so the swap below can set it during render without mutating
  // anything a discarded render could leave behind.
  const [provenance, setProvenance] = useState(() => ({
    workspaceId,
    recoveredFromHmr: initialRef.current!.recoveredFromHmr,
  }));

  let renderedState = state;
  let recoveredFromHmr = provenance.recoveredFromHmr;

  if (state.workspaceId !== workspaceId) {
    const previousWorkspaceId = state.workspaceId ?? mountedWorkspaceIdRef.current;
    const swapped = initWorkspaceState({ workspaceId, initialWorktrees });
    switchDebug("workspace.store.swap", {
      fromWorkspaceId: previousWorkspaceId,
      toWorkspaceId: workspaceId,
      outgoingTabCount: state.layout.tabs.length,
      outgoingSessionCount: Object.keys(state.sessions).length,
      incomingStateWorkspaceId: swapped.state.workspaceId ?? null,
      incomingActiveWorktreePath: swapped.state.activeWorktreePath,
      incomingWorktreeCount: swapped.state.worktrees.length,
      incomingTabCount: swapped.state.layout.tabs.length,
      incomingSessionCount: Object.keys(swapped.state.sessions).length,
      recoveredFromHmr: swapped.recoveredFromHmr,
    });
    renderedState = swapped.state;
    recoveredFromHmr = swapped.recoveredFromHmr;
    if (provenance.workspaceId !== workspaceId || provenance.recoveredFromHmr !== swapped.recoveredFromHmr) {
      setProvenance({ workspaceId, recoveredFromHmr: swapped.recoveredFromHmr });
    }
    reactDispatch({ type: "RESTORE_WORKSPACE", state: swapped.state });
  }

  // The reducer mints ids, so stateRef must track React's committed reduction rather than
  // dispatch()'s private one; outside a switch it re-syncs every render as before. Across a
  // workspace switch the refs and the outgoing snapshot move only once the swapped render
  // commits, so a discarded or StrictMode double render cannot repoint them.
  if (mountedWorkspaceIdRef.current === workspaceId && state.workspaceId === workspaceId) {
    stateRef.current = renderedState;
  }
  useLayoutEffect(() => {
    const previousWorkspaceId = mountedWorkspaceIdRef.current;
    if (previousWorkspaceId === workspaceId) return;
    const outgoing = stateRef.current;
    setWorkspaceSnapshot(outgoing.workspaceId ?? previousWorkspaceId, outgoing);
    stateRef.current = state;
    mountedWorkspaceIdRef.current = workspaceId;
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the switch only; dispatch owns later updates
  }, [workspaceId]);

  const emitActivityChanges = useCallback((previous: WorkspaceState, next: WorkspaceState) => {
    for (const target of selectActivityNotificationTargets(next)) {
      const previousState = previous.activityBySessionId?.[target.sessionId]?.state;
      if (previousState === target.state) continue;
      for (const listener of activityListenersRef.current) listener({ ...target, previousState });
    }
  }, []);

  const dispatch = useCallback(
    (action: WorkspaceAction) => {
      const previous = stateRef.current;
      // Same observation rule the inbox recorder applies: a frontmost window with nobody at it
      // is not watching, so the pane must keep its frame exactly when the inbox keeps the row.
      const contextualAction = action.type === "SESSION_TITLE_ACTIVITY" || action.type === "SESSION_SCREEN_ACTIVITY" || action.type === "MARK_TAB_UNREAD"
        ? { ...action, observed: (getNativeWindowFocused() ?? isWindowForegroundFocused()) && hasRecentObservationInteraction() }
        : action;
      const nextState = workspaceReducer(previous, contextualAction);
      // A no-op reduction has nothing to publish; React still reduces the action (id minting).
      const changed = nextState !== previous;
      stateRef.current = nextState;
      const owningWorkspaceId = nextState.workspaceId ?? mountedWorkspaceIdRef.current;
      if (changed) {
        setHmrWorkspaceState(owningWorkspaceId, nextState);
        setWorkspaceSnapshot(owningWorkspaceId, nextState);
      }
      reactDispatch(contextualAction);
      if (changed && (action.type === "SESSION_TITLE_ACTIVITY" || action.type === "SESSION_SCREEN_ACTIVITY")) {
        emitActivityChanges(previous, nextState);
      }

      // Explicit user navigation boundary: acknowledge notifications for the newly active session
      // directly and synchronously, independent of agent classifier activity.
      if (action.type === "ACTIVATE_TAB" || action.type === "FOCUS_PANE" || action.type === "SELECT_WORKTREE") {
        const activeTabId = nextState.layout.activeTabId;
        const activeTab = activeTabId ? nextState.layout.tabs.find((t) => t.id === activeTabId) : undefined;
        if (activeTab && isTerminalTab(activeTab)) {
          const tabLayout = nextState.layout.layoutsByTabId?.[activeTab.id];
          const activeLeafId = tabLayout?.activeLeafId;
          const activeSessionId = activeLeafId ? tabLayout?.sessionIdsByLeafId?.[activeLeafId] : activeTab.sessionId;
          if (activeSessionId) {
            window.dispatchEvent(
              new CustomEvent("ferryx:session-interacted", {
                detail: { sessionId: activeSessionId },
              }),
            );
          }
        }
      }
    },
    [],
  );

  const dispatchToOwner = useCallback((action: WorkspaceAction, owner?: string) => {
    if (!owner || owner === mountedWorkspaceIdRef.current) { dispatch(action); return; }
    const snapshot = getWorkspaceSnapshot(owner);
    if (!snapshot) return;
    const next = workspaceReducer(snapshot, action.type === "MARK_TAB_UNREAD" ? { ...action, observed: false } : action);
    setWorkspaceSnapshot(owner, next);
    setHmrWorkspaceState(owner, next);
    bumpParkedActivity();
  }, [dispatch, bumpParkedActivity]);

  useLayoutEffect(() => {
    localRecoveryAdmissions.set(workspaceId, dispatch);
    setWorkspaceSnapshot(workspaceId, stateRef.current);
    return () => {
      if (localRecoveryAdmissions.get(workspaceId) === dispatch) localRecoveryAdmissions.delete(workspaceId);
    };
  }, [workspaceId, dispatch]);

  useEffect(() => {
    const owningWorkspaceId = renderedState.workspaceId ?? workspaceId;
    setHmrWorkspaceState(owningWorkspaceId, renderedState);
  }, [renderedState, workspaceId]);

  useEffect(() => {
    // Native title and bell events carry the BACKEND session id and are emitted by the daemon
    // stream pump, so they arrive for every attached session -- including background tabs whose
    // panes `TerminalSplitView` has unmounted. That is the whole point: the notification exists
    // for the tab the user is NOT watching.
    const locateSession = (state: WorkspaceState, backendSessionId: string) => {
      const session = Object.values(state.sessions).find(
        (candidate) => candidate.backendSessionId === backendSessionId,
      );
      if (!session) return null;
      const tabId = findTabIdForSession(state, session.id);
      if (!tabId) return null;
      return { sessionId: session.id, tabId };
    };

    const resolveSession = (backendSessionId: string) =>
      locateSession(stateRef.current, backendSessionId);

    /**
     * Applies an activity action to a session owned by a project that is not currently mounted.
     *
     * Only one workspace has a live reducer, so without this an agent working in another project
     * would freeze at whatever state it held when the user switched away -- and the sidebar row
     * that now reads snapshots would spin forever. Returns true when the event was consumed.
     */
    const dispatchToParkedWorkspace = (
      backendSessionId: string,
      build: (resolved: { sessionId: string; tabId: string }) => WorkspaceAction,
    ): boolean => {
      const mountedWorkspaceId = stateRef.current.workspaceId ?? mountedWorkspaceIdRef.current;
      for (const [snapshotWorkspaceId, snapshot] of listWorkspaceSnapshots()) {
        if (snapshotWorkspaceId === mountedWorkspaceId) continue;
        const resolved = locateSession(snapshot, backendSessionId);
        if (!resolved) continue;
        const action = build(resolved);
        const isActivityAction = action.type === "SESSION_TITLE_ACTIVITY" || action.type === "SESSION_SCREEN_ACTIVITY";
        const nextState = workspaceReducer(snapshot, isActivityAction ? { ...action, observed: false } : action);
        if (nextState === snapshot) return true;
        setWorkspaceSnapshot(snapshotWorkspaceId, nextState);
        setHmrWorkspaceState(snapshotWorkspaceId, nextState);
        bumpParkedActivity();
        // Lifecycle-driven settling is bookkeeping about a dead PTY, not an agent reporting a
        // result, so it must stay as silent in a parked project as it is in the mounted one.
        if (isActivityAction) emitActivityChanges(snapshot, nextState);
        return true;
      }
      return false;
    };

    /**
     * A PTY exit is emitted for every attached session, including sessions owned by a project the
     * user switched away from. Without routing it into that project's snapshot its agent would
     * keep claiming live work forever, because only the mounted workspace has a live reducer.
     */
    const unsubscribeLifecycle = terminalEventBus.subscribeLifecycle((payload) => {
      const action: WorkspaceAction = {
        type: "SESSION_LIFECYCLE",
        backendSessionId: payload.sessionId,
        lifecycle: mapBackendLifecycle(payload),
        exitCode: payload.exitCode ?? null,
        incarnation: payload.incarnation ?? null,
        daemonEpoch: payload.daemonEpoch ?? null,
      };
      const session = Object.values(stateRef.current.sessions).find(
        (candidate) => candidate.backendSessionId === payload.sessionId,
      );
      const mountedOwnsSession = Boolean(session);
      const routedToParkedWorkspace = !mountedOwnsSession &&
        dispatchToParkedWorkspace(payload.sessionId, () => action);
      if (!routedToParkedWorkspace) dispatch(action);

      reconcileLoadedLocalRecovery();
    });

    let unlistenTitle: (() => void) | undefined;
    let unlistenBell: (() => void) | undefined;
    let unlistenAgentState: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    const fallbackProbesByAgent = new Map<string, { inFlight: boolean; startedAt: number }>();
    let subscribed = true;

    // Keystroke-rate: it always touches the module-level clocks, and dispatches only when the pane
    // still shows unseen attention, so typing into an already-active pane clears its frame (the
    // inbox acknowledges the same event) without a re-render per keystroke.
    const onSessionInteractedForAttention = (event: Event) => {
      const sessionId = (event as CustomEvent<{ sessionId?: string }>).detail?.sessionId;
      if (!sessionId) return;
      markSessionEngagementForAttention(sessionId);
      const activity = stateRef.current.activityBySessionId?.[sessionId];
      if (activity && !activity.seen && (activity.state === "done" || activity.state === "waiting")) {
        dispatch({ type: "MARK_SESSION_ACTIVITY_SEEN", sessionId });
      }
    };
    window.addEventListener("ferryx:session-interacted", onSessionInteractedForAttention);

    void onNativeTerminalTitle((payload) => {
      const resolved = resolveSession(payload.sessionId);
      if (!resolved) {
        dispatchToParkedWorkspace(payload.sessionId, (parked) => ({
          type: "SESSION_TITLE_ACTIVITY",
          tabId: parked.tabId,
          sessionId: parked.sessionId,
          title: payload.title,
        }));
        return;
      }
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId: resolved.tabId, sessionId: resolved.sessionId, title: payload.title });
    })
      .then((unlisten) => {
        if (subscribed) unlistenTitle = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalBell((payload) => {
      const mounted = stateRef.current;
      for (const snapshot of [mounted, ...listWorkspaceSnapshots()
        .filter(([id]) => id !== mounted.workspaceId).map(([, state]) => state)]) {
        const resolved = locateSession(snapshot, payload.sessionId);
        if (!resolved) continue;
        const target = notificationTarget(snapshot, resolved.sessionId, resolved.tabId);
        bellListenersRef.current.forEach((listener) => listener(resolved.sessionId, resolved.tabId, target));
        return;
      }
    })
      .then((unlisten) => {
        if (subscribed) unlistenBell = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalAgentState((payload) => {
      const resolved = resolveSession(payload.sessionId);
      if (!resolved) {
        dispatchToParkedWorkspace(payload.sessionId, (parked) => ({
          type: "SESSION_SCREEN_ACTIVITY",
          tabId: parked.tabId,
          sessionId: parked.sessionId,
          state: payload.state,
          ruleId: payload.ruleId,
          manifestId: payload.manifestId,
          providerSession: payload.providerSession,
          detail: payload.detail,
          isSnapshot: payload.isSnapshot,
          outcome: payload.outcome,
          kind: payload.kind,
        }));
        return;
      }
      dispatch({
        type: "SESSION_SCREEN_ACTIVITY",
        tabId: resolved.tabId,
        sessionId: resolved.sessionId,
        state: payload.state,
        ruleId: payload.ruleId,
        manifestId: payload.manifestId,
        providerSession: payload.providerSession,
        detail: payload.detail,
        isSnapshot: payload.isSnapshot,
        outcome: payload.outcome,
        kind: payload.kind,
      });
      if (
        !stateRef.current.sessions[resolved.sessionId]?.workspaceId.startsWith("daemon:")
        && !payload.providerSession
        && payload.manifestId
        && PROVIDER_SESSION_PROBE_AGENTS.includes(payload.manifestId)
      ) {
        const discoveryAgent = payload.manifestId === "cursor-agent" ? "cursor" : payload.manifestId;
        // An agent starts new conversations inside the same pane and process (`/new`), so its
        // session id is a live property of the pane rather than a one-shot capture: re-probe on
        // reported transitions instead of latching the first answer. A pane can surface several
        // manifest ids over its lifetime (shared TUI patterns across pi-family agents), so probes
        // are tracked per agent id: a failed probe under one id must not block a more specific id
        // later. This entry only de-duplicates an in-flight probe and rate-limits repeats, because
        // each probe walks the process tree and its open files.
        const probeKey = `${resolved.sessionId}\u0000${discoveryAgent}`;
        const probe = fallbackProbesByAgent.get(probeKey);
        const startedAt = Date.now();
        if (!probe?.inFlight && (!probe || startedAt - probe.startedAt >= PROVIDER_SESSION_PROBE_INTERVAL_MS)) {
          fallbackProbesByAgent.set(probeKey, { inFlight: true, startedAt });
          // The probe can outlive a project switch; its answer belongs to the project that
          // owned the pane when the probe started, not to whichever store is mounted later.
          const probeOwner = stateRef.current.workspaceId ?? mountedWorkspaceIdRef.current;
          void discoverAgentProviderSession(payload.sessionId, discoveryAgent)
            .then((id) => {
              if (!id) return;
              dispatchToOwner({
                type: "APPLY_PROVIDER_SESSION",
                sessionId: resolved.sessionId,
                providerSession: { key: "session_id", id },
                agentType: discoveryAgent,
              }, probeOwner);
            })
            .catch(() => undefined)
            .finally(() => {
              const entry = fallbackProbesByAgent.get(probeKey);
              if (entry) entry.inFlight = false;
              // Settled entries only rate-limit repeats; once past the interval they carry no
              // information, so sweep them instead of keeping one per pane/agent forever.
              const now = Date.now();
              for (const [key, settled] of fallbackProbesByAgent) {
                if (!settled.inFlight && now - settled.startedAt >= PROVIDER_SESSION_PROBE_INTERVAL_MS) {
                  fallbackProbesByAgent.delete(key);
                }
              }
            });
        }
      }
    })
      .then((unlisten) => {
        if (subscribed) unlistenAgentState = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalFocus((backendSessionId) => {
      const resolved = resolveSession(backendSessionId);
      if (!resolved) return;

      // Native-surface clicks can only originate from panes of the currently visible active
      // layout. A hit that only exists in a parked worktree layout means the event sat in flight
      // across a worktree switch: navigating there would yank the user back to the previous
      // worktree, so it is dropped. A leaf-less hit is a single-pane tab whose lone pane is
      // already that tab's active pane, so it is dropped as well.
      const found = locateSessionAcrossLayouts(stateRef.current, resolved.sessionId);
      if (!found?.leafId || found.worktreePath) return;

      dispatch({ type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: resolved.sessionId });

      const isAlreadyActive =
        stateRef.current.layout.activeTabId === found.tabId &&
        stateRef.current.layout.layoutsByTabId[found.tabId]?.activeLeafId === found.leafId;
      if (!isAlreadyActive) {
        dispatch({ type: "FOCUS_PANE", tabId: found.tabId, leafId: found.leafId });
      }
    })
      .then((unlisten) => {
        if (subscribed) unlistenFocus = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    return () => {
      subscribed = false;
      window.removeEventListener("ferryx:session-interacted", onSessionInteractedForAttention);
      unsubscribeLifecycle();
      unlistenTitle?.();
      unlistenBell?.();
      unlistenAgentState?.();
      unlistenFocus?.();
    };
  }, [dispatch, dispatchToOwner]);

  const createSpawnedTab = useCallback(
    async (worktree: Worktree, label?: string, backendOverride?: PreSpawnedBackend, shell?: string) => {
      await services.ensureTerminalEvents();
      // The detailed spawn is the only one that reports the session's incarnation and the daemon
      // epoch, and a pane without an incarnation can never form the durable seven-field attach
      // tuple, so its native surface attach fails until reconciliation happens to fill it in.
      // Use the detailed path wherever the transport provides it (the split path already does).
      let daemonEpoch: string | null = backendOverride?.daemonEpoch ?? null;
      let incarnation: string | null = backendOverride?.incarnation ?? null;
      let backendSessionId = backendOverride?.sessionId;
      if (!backendSessionId) {
        // Measured before the spawn: the new tab will fill the focused group's body.
        const grid = await measureFor(services, {
          kind: "newTab",
          groupId: getFocusedGroupId(stateRef.current.layout),
        });
        if (services.spawnTerminalDetailed) {
          const detailed = await spawnDetailedForLogicalAction(services, {
            workspaceId,
            worktree: worktreeIdentity(worktree),
            cwd: worktree.path,
            shell,
            ...initialGridFields(grid),
          });
          backendSessionId = detailed.sessionId;
          daemonEpoch = detailed.daemonEpoch ?? null;
          incarnation = detailed.session?.incarnation ?? null;
        } else {
          backendSessionId = await spawnTerminalForLogicalAction(services, {
            workspaceId,
            worktree: worktreeIdentity(worktree),
            cwd: worktree.path,
            shell,
            ...initialGridFields(grid),
          });
        }
      }
      const sessionId = createId("session");
      const tabId = createId("tab");
      const session: TerminalSession = {
        id: sessionId,
        cwd: worktree.path,
        worktreePath: worktree.path,
        workspaceId,
        worktree: worktreeIdentity(worktree),
        backendSessionId,
        ...(daemonEpoch !== null ? { daemonEpoch } : {}),
        ...(incarnation !== null ? { incarnation } : {}),
        lifecycle: "working",
        ...(isRemoteWorkspaceId(workspaceId) || isPairedWorkspaceId(workspaceId)
          ? {
              remoteConnectionState: "connected",
              remoteGeneration: 1,
            }
          : {}),
      };
      const tab: TerminalTab = {
        id: tabId,
        label: label ?? nextTabLabel(worktree),
        sessionId,
      };
      return { tab, session };
    },
    [services, workspaceId],
  );

  const openTab = useCallback(
    async (worktree: Worktree, label?: string, backendOverride?: PreSpawnedBackend, shell?: string) => {
      const capturedWorktreePath = worktree.path;
      switchDebug("terminal.open.start", {
        workspaceId,
        worktreePath: capturedWorktreePath,
        tabCount: stateRef.current.layout.tabs.length,
        sessionCount: Object.keys(stateRef.current.sessions).length,
        backendOverride: backendOverride?.sessionId ?? null,
      });
      const binding = await createSpawnedTab(worktree, label, backendOverride, shell);
      // The active project can change while the spawn is in flight; landing this
      // tab now would inject one project's worktree into another's state, and
      // dropping it silently would orphan the backend PTY we just created.
      if (mountedWorkspaceIdRef.current !== workspaceId) {
        if (isRemoteWorkspaceId(workspaceId)) {
          const originSnapshot = getWorkspaceSnapshot(workspaceId);
          if (originSnapshot) {
            if (!originSnapshot.worktrees.some((candidate) => candidate.path === capturedWorktreePath)) {
              dispatchToOwner({ type: "SET_WORKTREES", worktrees: [...originSnapshot.worktrees, worktree] }, workspaceId);
            }
            dispatchToOwner({ type: "ADD_TAB_WITH_SESSION", ...binding, targetWorktreePath: capturedWorktreePath }, workspaceId);
            dispatchToOwner({ type: "SELECT_WORKTREE", path: capturedWorktreePath }, workspaceId);
            return binding.tab.id;
          }
        }
        switchDebug("terminal.open.discarded", {
          requestedWorkspaceId: workspaceId,
          mountedWorkspaceId: mountedWorkspaceIdRef.current,
          worktreePath: capturedWorktreePath,
          backendSessionId: binding.session.backendSessionId,
        });
        if (backendOverride === undefined) {
          await closeBackendSession(binding.session, services);
        }
        return null;
      }
      if (!stateRef.current.worktrees.some((candidate) => candidate.path === capturedWorktreePath)) {
        dispatch({ type: "SET_WORKTREES", worktrees: [...stateRef.current.worktrees, worktree] });
      }
      dispatch({ type: "ADD_TAB_WITH_SESSION", ...binding, targetWorktreePath: capturedWorktreePath });
      dispatch({ type: "SELECT_WORKTREE", path: capturedWorktreePath });
      switchDebug("terminal.open.complete", {
        workspaceId,
        worktreePath: capturedWorktreePath,
        tabId: binding.tab.id,
        sessionId: binding.session.id,
        backendSessionId: binding.session.backendSessionId,
        tabCount: stateRef.current.layout.tabs.length,
      });
      return binding.tab.id;
    },
    [createSpawnedTab, dispatch, dispatchToOwner, services, workspaceId],
  );

  const sshRearmsRef = useRef(new Map<string, number>());
  const sshRecoveryKey = useMemo(
    () => Object.values(renderedState.sessions)
      .filter(session => isRemoteWorkspaceId(session.workspaceId) && session.lifecycle !== "exited")
      .sort((a, b) => a.id.localeCompare(b.id))
      .map(session => {
        const worktreeTag = session.worktree ? `${session.worktree.wsId}:${session.worktree.slug}` : "";
        const hostAuthority = `${session.ownerId ?? ""}:${session.daemonEpoch ?? ""}:${session.workspaceId ?? ""}:${worktreeTag}`;
        return `${session.id}:${session.backendSessionId ?? ""}:${session.incarnation ?? ""}:${hostAuthority}`;
      })
      .join("|"),
    [renderedState.sessions],
  );
  useEffect(() => {
    if (!tauriIpc.isTauriRuntime() || !sshRecoveryKey) return;
    const recovery = startSshRecovery({
      sessions: Object.values(stateRef.current.sessions).filter(
        session => isRemoteWorkspaceId(session.workspaceId) && session.lifecycle !== "exited",
      ),
      dispatch,
      onError: error => console.error("SSH session reconciliation failed:", error),
      rearms: sshRearmsRef.current,
    });
    void recovery.ready.catch(error => console.error("SSH status subscription failed:", error));
    return recovery.stop;
  }, [sshRecoveryKey, dispatch]);

  const splitController = useCallback(
    (session: LocalSplitSession) => {
      const intent = localSplitIntent(session);
      if (!intent) return undefined;
      const readOwner = (): WorkspaceState | undefined => {
        const activeState = stateRef.current;
        if (activeState.sessions[session.id]) return activeState;
        return undefined;
      };
      const updateOwner = (action: WorkspaceAction) => {
        dispatch(action);
      };
      const controller = new LocalSplitLifecycle(
        {
          operation: services.splitOperation ?? tauriIpc.spawnTerminalSplitOperation,
          create: services.splitCreate ?? tauriIpc.spawnTerminalDetailed,
          attach: services.splitAttach ?? tauriIpc.attachTerminal,
          ensureEvents: services.ensureTerminalEvents,
          read: () => readOwner()?.sessions[session.id],
          publish: (next) => updateOwner({ type: "LOCAL_SPLIT_UPDATE", session: next }),
          remove: () => {
            // Ownership ends here: the registry is consulted by the recovery effect below
            // (`hasLocalSplit`) and by `retryLocalSplit`, so leaving a dead id in it makes the
            // session permanently unretryable. `unregisterLocalSplit` had no caller at all.
            unregisterLocalSplit(session.id);
            updateOwner({ type: "LOCAL_SPLIT_REMOVE", sessionId: session.id });
          },
          visible: () => {
            const owner = readOwner();
            const tabId = owner ? findTabIdForSession(owner, session.id) : null;
            return Boolean(owner && tabId && isTabVisible(owner, tabId));
          },
        },
        intent,
      );
      registerLocalSplit(session.id, controller);
      return controller;
    },
    [dispatch, services, workspaceId],
  );

  useEffect(() => {
    for (const session of Object.values(renderedState.sessions)) {
      const intent = localSplitIntent(session);
      if (!intent || hasLocalSplit(session.id)) continue;
      const controller = splitController({ ...session, spawnIntent: intent });
      if (intent.cancelRequested) {
        void controller?.cancel();
      } else if (session.reconnectLifecycle !== "idle") {
        dispatch({
          type: "LOCAL_SPLIT_UPDATE",
          session: {
            ...session,
            spawnIntent: intent,
            reconnectLifecycle: "failed",
            reconnectError: {
              code: "SPAWN_ATTEMPT_TIMEOUT",
              message: "Shell startup requires reconciliation. Retry the original request.",
              details: { delivery: "ambiguous" },
            },
          },
        });
      }
    }
  }, [renderedState.sessions, splitController, dispatch]);

  const ensureSessionBackends = useCallback(
    async (sessionIds: string[], options?: { fallbackToShell?: boolean }) => {
      const targets = sessionIds.filter((sessionId) => {
        const session = stateRef.current.sessions[sessionId];
        if (!session || session.backendSessionId != null || localSplitIntent(session)) return false;
        if (isRemoteWorkspaceId(session.workspaceId)) return false;
        const isAgent = Boolean(session.agentType || session.providerSession || session.agentSessionId);
        const isReconnecting = session.reconnectLifecycle === "validating" || session.reconnectLifecycle === "spawning" || session.reconnectLifecycle === "binding";
        const canReconnect = isAgent && getAgentReconnectAffordance(session, stateRef.current.sessions).canReconnect;
        if (!options?.fallbackToShell && session.lifecycle === "exited" && (canReconnect || isReconnecting)) {
          return false;
        }
        if (spawningSessionIdsRef.current.has(sessionId)) return false;
        return true;
      });
      if (targets.length === 0) return;

      for (const sessionId of targets) {
        spawningSessionIdsRef.current.add(sessionId);
      }

      // Batch spawn (M8): one IPC round trip instead of N per-session invokes when the
      // transport supports it. Per-session fallback keeps mock services working.
      if (services.spawnTerminalsBatch && targets.length > 1) {
        await services.ensureTerminalEvents();
        const requests = await Promise.all(targets.map(async (sessionId) => {
          const session = stateRef.current.sessions[sessionId];
          const startup = resolvePairedStartup(workspaceId);
          const grid = await measureFor(services, { kind: "session", sessionId });
          return {
            workspaceId,
            worktree: session?.worktree ?? null,
            cwd: session?.cwd ?? session?.worktreePath ?? null,
            ...(startup ? { startup } : {}),
            ...initialGridFields(grid),
          };
        }));
        let entries: Array<{ index: number; sessionId: string | null; error: string | null }> = [];
        try {
          entries = await services.spawnTerminalsBatch(requests);
        } catch (batchError) {
          // Batch requests carry no clientRequestId, so the daemon cannot dedupe a retry. When
          // the failure is ambiguous the batch may already have created PTYs; spawning again
          // per session would duplicate them. Only a definite failure falls through.
          if (isAmbiguousRendererTransportError(batchError)) {
            const reason = toIpcError(batchError).message;
            for (const sessionId of targets) {
              spawningSessionIdsRef.current.delete(sessionId);
              dispatchToOwner({ type: "SESSION_BACKEND_UNAVAILABLE", sessionId, backendSessionId: null, reason }, workspaceId);
            }
            throw batchError;
          }
        }
        if (entries.length > 0) {
          try {
            for (const entry of entries) {
              const sessionId = targets[entry.index];
              if (sessionId === undefined) {
                if (entry.sessionId) await closeOrphanBackend(entry.sessionId, services);
                continue;
              }
              spawningSessionIdsRef.current.delete(sessionId);
              if (!entry.sessionId) {
                dispatchToOwner({
                  type: "SESSION_BACKEND_UNAVAILABLE",
                  sessionId,
                  backendSessionId: null,
                  reason: entry.error ?? "Terminal spawn failed",
                }, workspaceId);
                continue;
              }
              const session = stateRef.current.sessions[sessionId];
              // Recovery spawns can outlive a project switch, a pane close, or a newer
              // binding; in each case this PTY has no owner and must not leak.
              if (mountedWorkspaceIdRef.current !== workspaceId || !session || session.backendSessionId != null) {
                await closeOrphanBackend(entry.sessionId, services);
                continue;
              }
              const isAgent = Boolean(session.agentType || session.providerSession || session.agentSessionId);
              dispatch({
                type: "REBIND_SESSION_BACKEND",
                sessionId,
                backendSessionId: entry.sessionId,
                clearAgent: options?.fallbackToShell || isAgent,
              });
            }
          } finally {
            // Targets the batch result did not cover must become retryable again.
            for (const sessionId of targets) spawningSessionIdsRef.current.delete(sessionId);
          }
          return;
        }
      }

      await Promise.all(
        targets.map(async (sessionId) => {
          try {
            const session = stateRef.current.sessions[sessionId];
            if (!session || session.backendSessionId != null) return;
            await services.ensureTerminalEvents();
            const startup = resolvePairedStartup(workspaceId);
            const grid = await measureFor(services, { kind: "session", sessionId });
            try {
              const backendSessionId = await services.spawnTerminal({
                workspaceId,
                worktree: session.worktree,
                cwd: session.cwd ?? session.worktreePath,
                ...(startup ? { startup } : {}),
                ...initialGridFields(grid),
              });
              // Recovery spawns can outlive a project switch, a pane close, or a newer
              // binding; in each case this PTY has no owner and must not leak.
              const current = stateRef.current.sessions[sessionId];
              if (mountedWorkspaceIdRef.current !== workspaceId || !current || current.backendSessionId != null) {
                await closeOrphanBackend(backendSessionId, services);
                return;
              }
              const isAgent = Boolean(current.agentType || current.providerSession || current.agentSessionId);
              dispatch({
                type: "REBIND_SESSION_BACKEND",
                sessionId,
                backendSessionId,
                clearAgent: options?.fallbackToShell || isAgent,
              });
            } catch (spawnError) {
              dispatch({
                type: "SESSION_BACKEND_UNAVAILABLE",
                sessionId,
                backendSessionId: null,
                reason: toIpcError(spawnError).message,
              });
              throw spawnError;
            }
          } finally {
            // Let the caller surface registration/host failures, while allowing retry.
            spawningSessionIdsRef.current.delete(sessionId);
          }
        }),
      );
    },
    [dispatch, dispatchToOwner, services, workspaceId],
  );

  const ensureTabForWorktreeOnce = useCallback(
    async (worktree: Worktree, options?: { allowCreate?: boolean }) => {
      const allowCreate = options?.allowCreate ?? true;
      const restoreStatus = getWorkspaceRestoreStatus(workspaceId);
      const isRestoring = restoreStatus === "loading";
      const effectiveAllowCreate = allowCreate && !isRestoring;
      const snapshot = stateRef.current;
      switchDebug("worktree.ensure.start", {
        workspaceId,
        requestedPath: worktree.path,
        activeWorktreePath: snapshot.activeWorktreePath,
        allowCreate,
        activeTabCount: snapshot.layout.tabs.length,
        activeTabIds: snapshot.layout.tabs.map((tab) => tab.id),
        parkedTabCount: snapshot.worktreeLayouts?.[worktree.path]?.tabs.length ?? 0,
        sessionCount: Object.keys(snapshot.sessions).length,
      });
      if (snapshot.activeWorktreePath === worktree.path) {
        const hasValidActiveTab = Boolean(
          snapshot.layout.activeTabId &&
            snapshot.layout.tabs.some((tab) => tab.id === snapshot.layout.activeTabId),
        );
        const activeTab = snapshot.layout.tabs.find(
          (tab) => isTerminalTab(tab) && sessionWorktreePath(snapshot.sessions[tab.sessionId]) === worktree.path,
        ) ?? snapshot.layout.tabs[0];
        if (activeTab) {
          if (isTerminalTab(activeTab)) {
            const activeSession = snapshot.sessions[activeTab.sessionId];
            if (activeSession && activeSession.backendSessionId === null && isPairedWorkspaceId(activeSession.workspaceId)) {
              void ensureSessionBackends([activeSession.id], { fallbackToShell: true });
            }
          }
          if (hasValidActiveTab) {
            switchDebug("worktree.ensure.skipped", {
              workspaceId,
              worktreePath: worktree.path,
              activeTabId: snapshot.layout.activeTabId,
            });
            return snapshot.layout.activeTabId;
          }
          switchDebug("worktree.ensure.active-tab", {
            workspaceId,
            worktreePath: worktree.path,
            tabId: activeTab.id,
          });
          dispatch({ type: "ACTIVATE_TAB", tabId: activeTab.id });
          return activeTab.id;
        }
        if (!effectiveAllowCreate) {
          switchDebug("worktree.ensure.empty-no-create", {
            workspaceId,
            worktreePath: worktree.path,
          });
          return null;
        }
        switchDebug("worktree.ensure.create-active", {
          workspaceId,
          worktreePath: worktree.path,
        });
        return openTab(worktree);
      }

      const selectWorktreeImmediately = () => {
        if (!stateRef.current.worktrees.some((candidate) => candidate.path === worktree.path)) {
          dispatch({ type: "SET_WORKTREES", worktrees: [...stateRef.current.worktrees, worktree] });
        }
        dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
      };

      const parkedLayout = snapshot.worktreeLayouts?.[worktree.path];
      if (parkedLayout && parkedLayout.tabs.length > 0) {
        switchDebug("worktree.ensure.restore-parked", {
          workspaceId,
          worktreePath: worktree.path,
          parkedTabCount: parkedLayout.tabs.length,
          parkedActiveTabId: parkedLayout.activeTabId,
        });
        selectWorktreeImmediately();
        const activeTabId = parkedLayout.activeTabId ?? parkedLayout.tabs[0]?.id;
        if (activeTabId) {
          dispatch({ type: "ACTIVATE_TAB", tabId: activeTabId });
          const activeTab = parkedLayout.tabs.find((t) => t.id === activeTabId);
          if (activeTab && isTerminalTab(activeTab)) {
            const activeSession = snapshot.sessions[activeTab.sessionId];
            if (activeSession && activeSession.backendSessionId === null && isPairedWorkspaceId(activeSession.workspaceId)) {
              void ensureSessionBackends([activeSession.id], { fallbackToShell: true });
            }
          }
          return activeTabId;
        }
        return parkedLayout.tabs[0].id;
      }

      const existingInCurrent = snapshot.layout.tabs.find(
        (tab) => isTerminalTab(tab) && sessionWorktreePath(snapshot.sessions[tab.sessionId]) === worktree.path,
      );
      if (existingInCurrent) {
        switchDebug("worktree.ensure.existing-current", {
          workspaceId,
          worktreePath: worktree.path,
          tabId: existingInCurrent.id,
        });
        selectWorktreeImmediately();
        dispatch({ type: "ACTIVATE_TAB", tabId: existingInCurrent.id });
        if (isTerminalTab(existingInCurrent)) {
          const existingSession = snapshot.sessions[existingInCurrent.sessionId];
          if (existingSession && existingSession.backendSessionId === null && isPairedWorkspaceId(existingSession.workspaceId)) {
            void ensureSessionBackends([existingSession.id], { fallbackToShell: true });
          }
        }
        return existingInCurrent.id;
      }

      if (!effectiveAllowCreate) {
        switchDebug("worktree.ensure.empty-no-create", {
          workspaceId,
          worktreePath: worktree.path,
        });
        return null;
      }

      if (!isRestoring && snapshot.activeWorktreePath !== worktree.path) {
        selectWorktreeImmediately();
      }

      switchDebug("worktree.ensure.create-new", {
        workspaceId,
        worktreePath: worktree.path,
      });
      return openTab(worktree);
    },
    [dispatch, openTab, workspaceId],
  );

  // A click and a refresh can ensure the same worktree before the first spawn lands its tab;
  // share the in-flight result instead of spawning a second backend session and tab.
  const ensureInFlightRef = useRef(new Map<string, Promise<string | null>>());
  const ensureTabForWorktree = useCallback(
    (worktree: Worktree, options?: { allowCreate?: boolean }): Promise<string | null> => {
      const key = `${workspaceId}\u0000${worktree.path}\u0000${options?.allowCreate ?? true}`;
      const inFlight = ensureInFlightRef.current;
      const existing = inFlight.get(key);
      if (existing) return existing;
      const pending = ensureTabForWorktreeOnce(worktree, options);
      inFlight.set(key, pending);
      const release = () => {
        if (inFlight.get(key) === pending) inFlight.delete(key);
      };
      void pending.then(release, release);
      return pending;
    },
    [ensureTabForWorktreeOnce, workspaceId],
  );

  const splitPane = useCallback(
    async (
      tabId: string,
      targetLeafId: string,
      direction: PaneDirection,
      options: SplitPaneOptions = {},
    ) => {
      const snapshot = stateRef.current;
      const targetTab = snapshot.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!targetTab || !isTerminalTab(targetTab)) {
        switchDebug("split.pane.refused.non-terminal-tab", {
          reason: targetTab ? `tab-kind=${targetTab.kind}` : `tab-not-found:${tabId}`,
        });
        return;
      }
      const targetLayout = snapshot.layout.layoutsByTabId?.[targetTab.id];
      if (!targetLayout || !collectLeafIds(targetLayout.root).includes(targetLeafId)) {
        switchDebug("split.pane.refused.target-leaf-not-in-layout", {
          reason: `targetLeafId=${targetLeafId} layoutPresent=${targetLayout !== undefined} layoutLeaves=${targetLayout ? collectLeafIds(targetLayout.root).join("|") : "-"}`,
        });
        return;
      }

      const sourceLocalSessionId = targetLayout.sessionIdsByLeafId[targetLeafId] ?? targetTab.sessionId;
      const sourceSession = snapshot.sessions[sourceLocalSessionId];
      if (!sourceSession) {
        switchDebug("split.pane.refused.source-session-missing", {
          reason: `sourceLocalSessionId=${sourceLocalSessionId} tabSessionId=${targetTab.sessionId}`,
        });
        return;
      }

      const localSessionId = createId("session");
      const newLeafId = createId("leaf");
      const reliableLocal =
        !isRemoteWorkspaceId(sourceSession.workspaceId || workspaceId) &&
        !isPairedWorkspaceId(sourceSession.workspaceId || workspaceId);
      const session: LocalSplitSession = {
        id: localSessionId,
        cwd: sourceSession.cwd,
        worktreePath: sessionWorktreePath(sourceSession),
        workspaceId: sourceSession.workspaceId || workspaceId,
        worktree: sourceSession.worktree,
        backendSessionId: null,
        lifecycle: "working",
        reconnectLifecycle: "spawning",
        ...(reliableLocal ? { reconnectRequestId: crypto.randomUUID() } : {}),
      };
      if (reliableLocal && session.reconnectRequestId) {
        session.spawnIntent = {
          requestId: session.reconnectRequestId,
          prepared: null,
          cancelRequested: false,
          generation: 0,
          createSent: false,
        };
      }

      // Commit synchronously so the new leaf's pane is laid out before its PTY size is measured.
      flushSync(() => {
        dispatch({
          type: "SPLIT_PANE",
          tabId: targetTab.id,
          targetLeafId,
          direction,
          position: options.position,
          newLeafId,
          content: options.content,
          session,
        });
      });
      switchDebug("split.pane.dispatched", {
        reason: `tabId=${targetTab.id} targetLeafId=${targetLeafId} direction=${direction} newLeafId=${newLeafId}`,
      });

      if (options.content && options.content.kind !== "terminal") {
        return;
      }

      const grid = await measureFor(services, { kind: "session", sessionId: localSessionId });

      if (session.spawnIntent) {
        const controller = splitController(session);
        await controller?.run({
          workspaceId: session.workspaceId,
          worktree: session.worktree,
          inheritFromSessionId: sourceSession.backendSessionId,
          ...initialGridFields(grid),
        });
        return;
      }

      let backendSessionId: string | null = null;
      let daemonEpoch: string | null = null;
      let incarnation: string | null = null;
      let inheritedCwd = sourceSession.cwd;
      // Pin the owner before the first await: a project switch mid-spawn must still land
      // the binding (or the failure) on the workspace that owns this pane.
      const isRemote = isRemoteWorkspaceId(workspaceId) || isRemoteWorkspaceId(sourceSession.workspaceId);
      const owningWorkspaceId = isRemote ? (sourceSession.workspaceId || workspaceId) : mountedWorkspaceIdRef.current;
      try {
        await services.ensureTerminalEvents();
        if (services.spawnTerminalDetailed) {
          // CWD inheritance (M9): one IPC round trip — the daemon resolves the source
          // pane's live working directory server-side and returns it in the response.
          const isPaired = isPairedWorkspaceId(sourceSession.workspaceId || workspaceId);
          const result = await spawnDetailedForLogicalAction(services, {
            workspaceId: sourceSession.workspaceId || workspaceId,
            worktree: sourceSession.worktree,
            cwd: isPaired ? sourceSession.cwd : null,
            inheritFromSessionId: sourceSession.backendSessionId,
            ...initialGridFields(grid),
          });
          backendSessionId = result.sessionId;
          daemonEpoch = result.daemonEpoch ?? null;
          incarnation = result.session?.incarnation ?? null;
          inheritedCwd = result.session?.cwd ?? inheritedCwd;
        } else {
          // Mock/test services without detailed spawn: resolve live cwd explicitly.
          let liveCwd = sourceSession.cwd;
          if (sourceSession.backendSessionId) {
            try {
              liveCwd = (await services.getTerminalCwd(sourceSession.backendSessionId)) ?? sourceSession.cwd;
            } catch {
              liveCwd = sourceSession.cwd;
            }
          }
          inheritedCwd = liveCwd;
          backendSessionId = await spawnTerminalForLogicalAction(services, {
            workspaceId: sourceSession.workspaceId || workspaceId,
            worktree: sourceSession.worktree,
            cwd: liveCwd,
            ...initialGridFields(grid),
          });
        }

        const ownerState = owningWorkspaceId === mountedWorkspaceIdRef.current
          ? stateRef.current
          : getWorkspaceSnapshot(owningWorkspaceId);
        const currentSession = ownerState?.sessions[localSessionId];
        if (currentSession && currentSession.backendSessionId === null) {
          dispatchToOwner({
            type: "REBIND_SESSION_BACKEND",
            sessionId: localSessionId,
            backendSessionId,
            cwd: inheritedCwd,
            ...(daemonEpoch !== null ? { daemonEpoch } : {}),
            ...(incarnation !== null ? { incarnation } : {}),
          }, owningWorkspaceId);
        } else {
          terminalEventBus.clearSession(backendSessionId);
          await services.closeTerminal(backendSessionId).catch(() => undefined);
        }
      } catch (error) {
        if (backendSessionId) {
          terminalEventBus.clearSession(backendSessionId);
          await services.closeTerminal(backendSessionId).catch(() => undefined);
        }
        dispatchToOwner({
          type: "SESSION_BACKEND_UNAVAILABLE",
          sessionId: localSessionId,
          backendSessionId: null,
          spawnFailed: true,
          reason: toIpcError(error).message,
        }, owningWorkspaceId);
        throw error;
      }
    },
    [dispatchToOwner, services, workspaceId, splitController],
  );

  const moveTabToGroup = useCallback(
    (sourceTabId: string, targetGroupId: string, targetIndex?: number) => {
      dispatch({ type: "MOVE_TAB_TO_GROUP", sourceTabId, targetGroupId, targetIndex });
    },
    [dispatch],
  );

  const moveTabToSplit = useCallback(
    (
      sourceTabId: string,
      targetGroupId: string,
      direction: PaneDirection,
      position: "first" | "second" = "second",
      targetPane?: { tabId: string; leafId: string },
    ) => {
      if (targetPane) {
        dispatch({
          type: "MOVE_TAB_TO_PANE_SPLIT",
          sourceTabId,
          targetTabId: targetPane.tabId,
          targetLeafId: targetPane.leafId,
          direction,
          position,
        });
      } else {
        dispatch({ type: "MOVE_TAB_TO_SPLIT", sourceTabId, targetGroupId, direction, position });
      }
    },
    [dispatch],
  );

  const detachPaneToTab = useCallback(
    (sourceTabId: string, leafId: string, targetGroupId?: string, targetIndex?: number) => {
      const snapshot = stateRef.current;
      const sourceTab = snapshot.layout.tabs.find((tab) => tab.id === sourceTabId);
      const sourceLayout = snapshot.layout.layoutsByTabId[sourceTabId];
      if (!sourceTab || !isTerminalTab(sourceTab) || !sourceLayout || collectLeafIds(sourceLayout.root).length <= 1) return null;
      const sessionId = sourceLayout.sessionIdsByLeafId[leafId];
      if (!sessionId || !snapshot.sessions[sessionId]) return null;
      const newTabId = createId("tab");
      const newTab: TerminalTab = {
        id: newTabId,
        label: sourceTab.label,
        sessionId,
      };
      dispatch({ type: "DETACH_PANE_TO_TAB", sourceTabId, leafId, newTab, targetGroupId, targetIndex });
      return stateRef.current.layout.tabs.some((tab) => tab.id === newTabId) ? newTabId : null;
    },
    [dispatch],
  );

  const closeTab = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const closingTab = snapshot.layout.tabs.find((tab) => tab.id === tabId);
      if (!closingTab || closingTab.pinned) return;

      if (closingTab.kind === "browser") {
        // The tab closes regardless of the native teardown: a rejected close must not leave the
        // user with a tab that can never be closed. The webview is torn down only once no layout
        // references its id any more.
        dispatch({ type: "CLOSE_TAB", tabId });
        if (!isBrowserIdReferenced(stateRef.current, closingTab.browserId)) {
          await closeBrowser(closingTab.browserId).catch((error: unknown) => {
            console.warn("Failed to close browser webview:", closingTab.browserId, error);
          });
        }
        return;
      }

      if (closingTab.kind === "file") {
        dispatch({ type: "CLOSE_TAB", tabId });
        releaseOrphanPreviews(stateRef.current);
        return;
      }

      if (closingTab.kind === "changes") {
        dispatch({ type: "CLOSE_TAB", tabId });
        return;
      }

      // The pane disconnects its viewer session on unmount; no backend PTY to dispose.
      if (closingTab.kind === "remoteDesktop") {
        dispatch({ type: "CLOSE_TAB", tabId });
        return;
      }

      // A browser can live as pane content inside a terminal-kind tab (e.g. after a
      // browser tab was dragged into this tab's split); collect its ids so the native
      // child webviews are torn down with the tab instead of leaking.
      const closingBrowserIds = new Set<string>();
      for (const content of Object.values(snapshot.layout.layoutsByTabId?.[tabId]?.contentsByLeafId ?? {})) {
        if (content.kind !== "browser") continue;
        const browserId = content.browser?.browserId ?? content.browserId ?? "";
        if (browserId) closingBrowserIds.add(browserId);
      }

      const disposableSessions = getDisposableSessionsForTab(snapshot, tabId);
      if (snapshot.layout.tabs.length === 1 && !disposableSessions.some((session) => localSplitIntent(session))) {
        // Only a healthy local PTY holds a writer the next tab must wait for. A connecting,
        // reconnecting, lost or exited session has none, and its close may reject or never
        // settle, so it must not hold the tab open; even the healthy wait is bounded.
        const healthy = disposableSessions.filter(isHealthyLocalPty);
        const results = await Promise.allSettled(healthy.map((session) => withTimeout(
          closeBackendSessionAndWait(session, services),
          LAST_TAB_EXIT_TIMEOUT_MS,
          "closeTab.lastTabExit",
        )));
        warnRejectedCloses(results, healthy);
        dispatch({ type: "CLOSE_TAB", tabId });
        const rest = disposableSessions.filter((session) => !healthy.includes(session));
        warnRejectedCloses(
          await Promise.allSettled(rest.map((session) => closeBackendSession(session, services))),
          rest,
        );
      } else {
        dispatch({ type: "CLOSE_TAB", tabId });
        await Promise.allSettled(disposableSessions.map((session) => closeBackendSession(session, services)));
      }

      // Only after CLOSE_TAB removes the layout references, tear down any collected
      // browser whose id is no longer referenced anywhere.
      await Promise.allSettled(
        [...closingBrowserIds]
          .filter((browserId) => !isBrowserIdReferenced(stateRef.current, browserId))
          .map((browserId) => closeBrowser(browserId)),
      );
      releaseOrphanPreviews(stateRef.current);
    },
    [dispatch, services],
  );

  const closePane = useCallback(
    async (tabId: string, leafId: string) => {
      const snapshot = stateRef.current;
      const tabLayout = snapshot.layout.layoutsByTabId?.[tabId];
      if (!tabLayout) return;
      if (tabLayout.root.type === "leaf") {
        if (tabLayout.root.leafId === leafId) await closeTab(tabId);
        return;
      }

      const closingSessionId = tabLayout.sessionIdsByLeafId[leafId];
      const closingSession = closingSessionId ? snapshot.sessions[closingSessionId] : undefined;
      const closingContent = tabLayout.contentsByLeafId?.[leafId];
      const closingBrowserId = closingContent?.kind === "browser"
        ? (closingContent.browser?.browserId ?? closingContent.browserId ?? "")
        : "";
      dispatch({ type: "CLOSE_PANE", tabId, leafId });
      releaseOrphanPreviews(stateRef.current);
      if (closingSessionId && !isSessionReferenced(stateRef.current, closingSessionId)) {
        await closeBackendSession(closingSession, services);
      }
      // Browser pane leaves carry no session; their native child webview must be torn
      // down here once the last layout reference to the browser id is gone.
      if (closingBrowserId && !isBrowserIdReferenced(stateRef.current, closingBrowserId)) {
        await closeBrowser(closingBrowserId);
      }
    },
    [closeTab, dispatch, services],
  );

  const closeTabs = useCallback(
    async (tabIds: string[]) => {
      for (const tabId of tabIds) await closeTab(tabId);
    },
    [closeTab],
  );

  const closeOtherTabs = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      await closeTabs(tabs.filter((tab) => tab.id !== tabId && !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const closeTabsToRight = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      const index = tabs.findIndex((tab) => tab.id === tabId);
      if (index < 0) return;
      await closeTabs(tabs.slice(index + 1).filter((tab) => !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const closeTabsToLeft = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      const index = tabs.findIndex((tab) => tab.id === tabId);
      if (index < 0) return;
      await closeTabs(tabs.slice(0, index).filter((tab) => !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const syncWorktrees = useCallback(
    async (worktrees: Worktree[], options?: { confirmedRemoval?: boolean }) => {
      const snapshot = stateRef.current;
      // An empty listing next to live sessions is far more often a transient listing failure
      // than every worktree vanishing at once; applying it would close every PTY. Callers
      // that know the removal is real pass confirmedRemoval.
      if (worktrees.length === 0 && Object.keys(snapshot.sessions).length > 0 && !options?.confirmedRemoval) {
        switchDebug("worktree.sync.refused-empty", {
          workspaceId,
          sessionCount: Object.keys(snapshot.sessions).length,
        });
        return;
      }
      const validWorktreePaths = new Set(worktrees.map((worktree) => worktree.path));
      const staleSessions = Object.values(snapshot.sessions).filter((session) => !validWorktreePaths.has(sessionWorktreePath(session)));
      dispatch({ type: "SET_WORKTREES", worktrees });
      // Removing a worktree drops its parked layout, file tabs included.
      releaseOrphanPreviews(stateRef.current);
      await Promise.allSettled(staleSessions.map((session) => closeBackendSession(session, services)));
    },
    [dispatch, services, workspaceId],
  );

  const activateTab = useCallback((tabId: string) => dispatch({ type: "ACTIVATE_TAB", tabId }), [dispatch]);
  const reorderTab = useCallback(
    (tabId: string, targetIndex: number) => dispatch({ type: "REORDER_TAB", tabId, targetIndex }),
    [dispatch],
  );
  const renameTab = useCallback(
    (tabId: string, label: string) => dispatch({ type: "RENAME_TAB", tabId, label }),
    [dispatch],
  );
  const setTabPinned = useCallback(
    (tabId: string, pinned: boolean) => dispatch({ type: "SET_TAB_PINNED", tabId, pinned }),
    [dispatch],
  );
  const focusPane = useCallback((tabId: string, leafId: string) => dispatch({ type: "FOCUS_PANE", tabId, leafId }), [dispatch]);
  const setPaneRatio = useCallback(
    (
      tabId: string,
      path: string,
      ratio: number,
      options?: { isolated?: boolean; seam?: ResolvedSeam | null },
    ) =>
      dispatch({
        type: "SET_PANE_RATIO",
        tabId,
        path,
        ratio,
        isolated: options?.isolated,
        seam: options?.seam,
      }),
    [dispatch],
  );
  const setTabGroupRatio = useCallback(
    (path: string, ratio: number) => dispatch({ type: "SET_TAB_GROUP_RATIO", path, ratio }),
    [dispatch],
  );
  const equalizeTabGroupRun = useCallback(
    (path: string) => dispatch({ type: "EQUALIZE_TAB_GROUP_RUN", path }),
    [dispatch],
  );
  const equalizePaneRun = useCallback(
    (tabId: string, path: string) => dispatch({ type: "EQUALIZE_PANE_RUN", tabId, path }),
    [dispatch],
  );
  const swapPanes = useCallback(
    (tabId: string, sourceLeafId: string, targetLeafId: string) => dispatch({ type: "SWAP_PANES", tabId, sourceLeafId, targetLeafId }),
    [dispatch],
  );

  const restoreWorkspace = useCallback(
    (restoredState: WorkspaceState) => dispatch({ type: "RESTORE_WORKSPACE", state: restoredState }),
    [dispatch],
  );

  const updateSessionTitleActivity = useCallback(
    (tabId: string, title: string, explicitSessionId?: string) => {
      const snapshot = stateRef.current;
      const tab = getAllTabs(snapshot).find((candidate) => candidate.id === tabId);
      if (!tab || !isTerminalTab(tab)) return;
      const sessionId = explicitSessionId ?? tab.sessionId;
      if (!snapshot.sessions[sessionId]) return;
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId, title });
    },
    [dispatch],
  );

  const subscribeTerminalBell = useCallback((listener: (sessionId: string, tabId: string, target?: ActivityNotificationTarget) => void) => {
    const listeners = bellListenersRef.current;
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);

  const subscribeActivityNotification = useCallback((listener: (event: ActivityNotificationEvent) => void) => {
    activityListenersRef.current.add(listener);
    return () => { activityListenersRef.current.delete(listener); };
  }, []);

  const createBrowserTab = useCallback(
    async (url = "http://localhost:3000", label?: string, options?: { worktreePath?: string; profileId?: string; browserId?: string; opener?: { browserId: string; handle: string } }) => {
      const capturedWorktreePath = options?.worktreePath ?? stateRef.current.activeWorktreePath ?? undefined;
      const targetWorktree = capturedWorktreePath
        ? stateRef.current.worktrees.find((wt) => wt.path === capturedWorktreePath)
        : getActiveWorktree(stateRef.current);
      const createRequest: Parameters<typeof createBrowser>[0] = {
        browserId: options?.browserId,
        workspaceId,
        worktreePath: targetWorktree?.path,
        url,
        profile: options?.profileId,
        // Created hidden on purpose. The backend has no pane geometry yet, so a visible child
        // webview would be shown at the creation default rect (0,0 800x600) and cover app
        // chrome plus any native terminal underneath until BrowserPane reports real bounds.
        // BrowserPane reveals it once the bounds update is acknowledged.
        visible: false,
      };
      // A popup tab is created linked to the tab that called window.open, so its OAuth
      // callback can be routed back through the host.
      const browserState = options?.opener
        ? await createPopupBrowser(createRequest, options.opener)
        : await createBrowser(createRequest);
      // Landing this tab after a project switch would attach one project's
      // browser to another's layout, and dropping it silently would orphan the
      // webview we just created.
      if (mountedWorkspaceIdRef.current !== workspaceId) {
        // The tab is discarded here, so the webview close has no user-facing surface to fail
        // into; it is still recorded rather than vanishing as a silent no-op.
        await closeBrowser(browserState.browserId).catch((error: unknown) => {
          switchDebug("browser.create.discarded-close.error", {
            browserId: browserState.browserId,
            error: String(error),
          });
        });
        return null;
      }

      const tabId = createId("tab");
      const browserTab: BrowserTab = {
        kind: "browser",
        id: tabId,
        label: label ?? "Browser",
        browserId: browserState.browserId,
        url: browserState.url,
        title: browserState.title,
        loading: browserState.loading,
        canGoBack: browserState.canGoBack,
        canGoForward: browserState.canGoForward,
        zoomFactor: browserState.zoomFactor,
        loadError: browserState.loadError ?? null,
        profileId: browserState.profileId,
        worktreePath: targetWorktree?.path,
      };

      dispatch({ type: "ADD_TAB_WITH_SESSION", tab: browserTab, targetWorktreePath: capturedWorktreePath });
      return tabId;
    },
    [dispatch, workspaceId],
  );

  const openFilePreviewTab = useCallback(
    (source: FilePreviewSource, request: FilePreviewOpenRequest): string => {
      const snapshot = stateRef.current;
      const sourceWorktreePath = sessionWorktreePath(snapshot.sessions[source.sessionId]);
      const targetWorktreePath = sourceWorktreePath || snapshot.activeWorktreePath;
      // An absolute path names one file whichever session (or the CLI, with no session) asked;
      // only a relative path depends on the requesting session's cwd.
      const absolute = /^(?:\/|\\\\|[a-zA-Z]:[\\/])/.test(request.path);
      const sameFile = (tab: WorkspaceTab) =>
        tab.kind === "file" && (request.remote
          ? tab.remote?.workspaceId === request.remote.workspaceId && tab.remote.path === request.remote.path
          : !tab.remote && tab.path === request.path && (absolute || tab.backendSessionId === request.backendSessionId));
      const existing = (targetWorktreePath === snapshot.activeWorktreePath ? snapshot.layout.tabs : []).find(sameFile);
      if (existing && existing.kind === "file") {
        retainFilePreview(existing.previewId, source, request);
        dispatch({
          type: "UPDATE_FILE_TAB",
          tabId: existing.id,
          path: request.path,
          remote: request.remote,
          line: request.line,
          col: request.col,
        });
        return existing.id;
      }

      for (const [wtPath, parkedLayout] of Object.entries(snapshot.worktreeLayouts ?? {})) {
        if (sourceWorktreePath && wtPath !== sourceWorktreePath) continue;
        const parkedTab = parkedLayout.tabs.find(sameFile);
        if (parkedTab && parkedTab.kind === "file") {
          dispatch({ type: "SELECT_WORKTREE", path: wtPath });
          retainFilePreview(parkedTab.previewId, source, request);
          dispatch({
            type: "UPDATE_FILE_TAB",
            tabId: parkedTab.id,
            path: request.path,
            remote: request.remote,
            line: request.line,
            col: request.col,
          });
          return parkedTab.id;
        }
      }

      const tabId = createId("tab");
      const label = request.path.split(/[/\\]/).pop() || request.path;
      const tab: FileTab = {
        kind: "file",
        id: tabId,
        label,
        path: request.path,
        backendSessionId: request.backendSessionId,
        line: request.line,
        col: request.col,
        workspaceId: source.workspaceId,
        previewId: tabId,
        ...(request.remote ? { remote: request.remote } : {}),
      };
      retainFilePreview(tabId, source, request);
      dispatch({ type: "ADD_TAB_WITH_SESSION", tab, targetWorktreePath: targetWorktreePath ?? undefined });
      // The stored label stays the basename; TabBar disambiguates duplicates at render time.
      return tabId;
    },
    [dispatch],
  );

  /**
   * Opens (or focuses) the Changes tab of a checkout in that worktree's layout. The main
   * checkout and unmanaged worktrees have no identity; the backend resolves them by path.
   */
  const openChangesTab = useCallback(
    (worktree: Worktree, ownerWorkspaceId: string): string | null => {
      const identity = worktreeIdentity(worktree);
      const snapshot = stateRef.current;
      const isChangesFor = (tab: WorkspaceTab) =>
        tab.kind === "changes" && tab.worktreePath === worktree.path && tab.workspaceId === ownerWorkspaceId;
      if (snapshot.activeWorktreePath !== worktree.path && snapshot.worktrees.some((candidate) => candidate.path === worktree.path)) {
        dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
      }
      const current = stateRef.current;
      const existing = (current.activeWorktreePath === worktree.path ? current.layout.tabs : current.worktreeLayouts?.[worktree.path]?.tabs ?? [])
        .find(isChangesFor);
      if (existing) {
        dispatch({ type: "ACTIVATE_TAB", tabId: existing.id });
        return existing.id;
      }
      const tabId = createId("tab");
      const tab: ChangesTab = {
        kind: "changes",
        id: tabId,
        label: "Changes",
        workspaceId: ownerWorkspaceId,
        worktreePath: worktree.path,
        worktree: identity,
      };
      dispatch({ type: "ADD_TAB_WITH_SESSION", tab, targetWorktreePath: worktree.path });
      return tabId;
    },
    [dispatch],
  );

  /** Opens (or focuses) the remote desktop viewer of a paired host in the active layout. */
  const openRemoteDesktopTab = useCallback(
    (hostId: string, label: string): string => {
      const existing = stateRef.current.layout.tabs.find(
        (tab) => tab.kind === "remoteDesktop" && tab.hostId === hostId,
      );
      if (existing) {
        dispatch({ type: "ACTIVATE_TAB", tabId: existing.id });
        return existing.id;
      }
      const tabId = createId("tab");
      const tab: RemoteDesktopTab = { kind: "remoteDesktop", id: tabId, label, hostId, sessionId: "" };
      dispatch({ type: "ADD_TAB_WITH_SESSION", tab });
      return tabId;
    },
    [dispatch],
  );

  const adoptBrowserSession = useCallback(
    (
      browserState: BrowserState | BrowserSessionCreatedPayload["browser"],
      targetWorkspaceId?: string | null,
    ): string | null => {
      const allTabs = [
        ...stateRef.current.layout.tabs,
        ...Object.values(stateRef.current.worktreeLayouts ?? {}).flatMap((layout) => layout.tabs),
      ];
      if (
        allTabs.some((tab) => tab.kind === "browser" && tab.browserId === browserState.browserId) ||
        isBrowserIdReferenced(stateRef.current, browserState.browserId)
      ) {
        return null;
      }

      const resolvedWorkspaceId = targetWorkspaceId ?? browserState.workspaceId ?? undefined;
      const targetWorktreePathInput = browserState.worktreePath ?? undefined;

      const matchedByWorkspaceId = resolvedWorkspaceId
        ? stateRef.current.worktrees.find(
            (wt) => wt.workspaceId === resolvedWorkspaceId || worktreeIdentity(wt)?.wsId === resolvedWorkspaceId,
          )
        : undefined;

      const matchedByWorktreePath = targetWorktreePathInput
        ? stateRef.current.worktrees.find((wt) => wt.path === targetWorktreePathInput)
        : undefined;

      const activeWorktree = getActiveWorktree(stateRef.current);

      const targetWorktree = matchedByWorkspaceId ?? matchedByWorktreePath ?? activeWorktree;
      const targetWorktreePath = targetWorktree?.path;

      const tabId = createId("tab");
      const browserTab: BrowserTab = {
        kind: "browser",
        id: tabId,
        label: browserState.title || "Browser",
        browserId: browserState.browserId,
        url: browserState.url,
        title: browserState.title,
        loading: browserState.loading,
        canGoBack: browserState.canGoBack,
        canGoForward: browserState.canGoForward,
        zoomFactor: browserState.zoomFactor,
        loadError: browserState.loadError ?? null,
        profileId: browserState.profileId,
        worktreePath: targetWorktree?.path,
      };

      dispatch({ type: "ADD_TAB_WITH_SESSION", tab: browserTab, targetWorktreePath });
      return tabId;
    },
    [dispatch],
  );

  const duplicateBrowserTab = useCallback(
    async (tabId: string, profileId?: string) => {
      const source = stateRef.current.layout.tabs.find((tab) => tab.id === tabId);
      if (!source || source.kind !== "browser") return null;
      return createBrowserTab(source.url, source.label, {
        worktreePath: source.worktreePath,
        profileId: profileId ?? source.profileId,
      });
    },
    [createBrowserTab],
  );

  const navigateBrowserTabAction = useCallback(
    async (tabId: string, url: string, paneBrowserId?: string) => {
      const browserId = getBrowserIdForTab(stateRef.current.layout, tabId, paneBrowserId);
      if (!browserId) return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, browserId, updates: { url, loading: true } });
      try {
        await navigateBrowser(browserId, url);
      } finally {
        dispatch({ type: "UPDATE_BROWSER_TAB", tabId, browserId, updates: { loading: false } });
      }
    },
    [dispatch],
  );

  const reloadBrowserTabAction = useCallback(
    async (tabId: string, paneBrowserId?: string, options?: BrowserReloadOptions) => {
      const browserId = getBrowserIdForTab(stateRef.current.layout, tabId, paneBrowserId);
      if (!browserId) return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, browserId, updates: { loading: true } });
      try {
        await reloadBrowser(browserId, options);
      } finally {
        dispatch({ type: "UPDATE_BROWSER_TAB", tabId, browserId, updates: { loading: false } });
      }
    },
    [dispatch],
  );

  const openWorkspacePortInBrowser = useCallback(
    async (port: number) => {
      const url = `http://localhost:${port}`;
      return createBrowserTab(url, `Port ${port}`);
    },
    [createBrowserTab],
  );

  const agents = useMemo(
    () => selectAgents(renderedState),
    [renderedState.layout, renderedState.worktreeLayouts, renderedState.sessions, renderedState.worktrees, renderedState.activityBySessionId],
  );
  const tabActivity = useMemo(
    () => selectTabActivitySummaries(renderedState),
    [renderedState.layout, renderedState.activityBySessionId, renderedState.unreadTabIds],
  );
  // Rows for other projects come from their snapshots and are overlaid by the live workspace, so a
  // worktree the user switched away from keeps reporting its agent instead of going blank.
  const worktreeActivity = useMemo(
    () => ({
      ...selectWorktreeActivitySummariesAcrossWorkspaces(renderedState.workspaceId ?? workspaceId),
      ...selectWorktreeActivitySummaries(renderedState),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- parkedActivityVersion is the change
    // signal for the snapshot cache, which useMemo cannot observe directly.
    [renderedState.layout, renderedState.worktreeLayouts, renderedState.sessions, renderedState.worktrees, renderedState.activityBySessionId, renderedState.unreadTabIds, renderedState.unreadWorktreePaths, renderedState.workspaceId, workspaceId, parkedActivityVersion],
  );
  const activityNotificationTargets = useMemo(
    () => selectActivityNotificationTargets(renderedState),
    [renderedState.layout, renderedState.worktreeLayouts, renderedState.worktrees, renderedState.activityBySessionId, renderedState.sessions],
  );
  const [notificationVersion, setNotificationVersion] = useState(0);
  useEffect(() => {
    return notificationCenterStore.subscribe(() => {
      setNotificationVersion((v) => v + 1);
    });
  }, []);
  const unreadBadgeCount = useMemo(
    () => selectGlobalUnreadBadgeCount(renderedState, workspaceId),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- parkedActivityVersion and notificationVersion
    // are external change signals that useMemo cannot observe directly from renderedState.
    [renderedState, workspaceId, parkedActivityVersion, notificationVersion],
  );

  return {
    state: renderedState,
    recoveredFromHmr,
    agents,
    tabActivity,
    worktreeActivity,
    unreadBadgeCount,
    activityNotificationTargets,
    openTab,
    createBrowserTab,
    openFilePreviewTab,
    openChangesTab,
    openRemoteDesktopTab,
    adoptBrowserSession,
    duplicateBrowserTab,
    navigateBrowserTab: navigateBrowserTabAction,
    reloadBrowserTab: reloadBrowserTabAction,
    ensureTabForWorktree,
    openWorkspacePortInBrowser,
    ensureSessionBackends,
    dispatchWorkspaceAction: dispatch,
    closeTab,
    closeOtherTabs,
    closeTabsToRight,
    closeTabsToLeft,
    splitPane,
    moveTabToGroup,
    moveTabToSplit,
    detachPaneToTab,
    closePane,
    activateTab,
    reorderTab,
    renameTab,
    setTabPinned,
    focusPane,
    setPaneRatio,
    equalizePaneRun,
    setTabGroupRatio,
    equalizeTabGroupRun,
    swapPanes,
    syncWorktrees,
    restoreWorkspace,
    resetAgentState: async (sessionId: string) => {
      const session = stateRef.current.sessions[sessionId];
      const backendId = session?.backendSessionId || sessionId;
      await tauriIpc.resetAgentState(backendId);
      dispatch({ type: "RESET_AGENT_STATE", sessionId });
    },
    resetWorktreeAgentState: async (worktreePath: string) => {
      const sessions = Object.values(stateRef.current.sessions).filter(
        (s) => sessionWorktreePath(s) === worktreePath,
      );
      const results = await Promise.allSettled(sessions.map(async (session) => {
        const backendId = session.backendSessionId || session.id;
        await tauriIpc.resetAgentState(backendId);
        dispatch({ type: "RESET_AGENT_STATE", sessionId: session.id });
      }));
      const failures = results.filter((result) => result.status === "rejected");
      if (failures.length > 0) {
        throw Object.assign(new Error(`Agent state reset failed for ${failures.length} of ${sessions.length} sessions`), {
          results: results.map((result, index) => ({ sessionId: sessions[index].id, ...result })),
        });
      }
    },
    markBackendSessionUnavailable: (
      sessionId: string,
      backendSessionId: string,
      reason: string,
      bindingKey?: string | null,
      attemptKey?: string | null,
      attemptToken?: number | null,
      outcome?: "retryable" | "blocked",
    ) => {
      // A local loss is fenced by the session's current recovery owner context. Apply it once
      // through dispatch so the hook state, its ref and the snapshot cache stay identical; a
      // snapshot-only write is overwritten by the hook's own (rejected, still running) state.
      const current = stateRef.current.sessions[sessionId];
      const local = current?.backendSessionId === backendSessionId &&
        !isPairedWorkspaceId(workspaceId) && !isRemoteWorkspaceId(workspaceId);
      dispatch({
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId,
        backendSessionId,
        bindingKey,
        attemptKey: attemptKey ?? (local ? current.recoveryKey : undefined),
        attemptToken: attemptToken ?? (local ? current.recoveryToken : undefined),
        outcome,
        reason,
      });
      if (local) wakeLocalSessionRecovery(workspaceId, sessionId);
    },
    markSessionAffirmativeExit: (
      sessionId: string,
      backendSessionId: string,
      incarnation: string,
      daemonEpoch?: string | null,
      token?: number | null,
      exitCode?: number | null,
      reason?: string | null,
    ) => {
      dispatch({
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId,
        backendSessionId,
        incarnation,
        daemonEpoch,
        token,
        exitCode,
        reason,
      });
    },
    updateSessionTitleActivity,
    subscribeTerminalBell,
    subscribeActivityNotification,
    parkedActivityVersion,
    markTabUnread: (tabId: string, owner?: string) => dispatchToOwner({ type: "MARK_TAB_UNREAD", tabId }, owner),
    clearTabUnread: (tabId: string) => dispatch({ type: "CLEAR_TAB_UNREAD", tabId }),
    markWorktreeUnread: (worktreePath: string, owner?: string) => dispatchToOwner({ type: "MARK_WORKTREE_UNREAD", worktreePath }, owner),
    clearWorktreeUnread: (worktreePath: string) => dispatch({ type: "CLEAR_WORKTREE_UNREAD", worktreePath }),
    /** Acknowledges one session's pane attention in whichever workspace owns it (mounted or parked). */
    markSessionActivitySeen: (sessionId: string, owner?: string) =>
      dispatchToOwner({ type: "MARK_SESSION_ACTIVITY_SEEN", sessionId }, owner),
  };
}

/**
 * Authoritative describe-to-exit consumer:
 * Queries routed DescribeSession for a terminal session and, only if affirmative matching exit proof
 * is established with matching incarnation, dispatches SESSION_AFFIRMATIVE_EXIT.
 * If the session is running or absent/uncertain, preserves retained ID and activity.
 */
export async function reconcileSessionFromDescribe(
  sessionId: string,
  backendSessionId: string,
  expectedIncarnation: string | null | undefined,
  dispatch: (action: WorkspaceAction) => void,
  expectedEpoch?: string | null,
  expectedToken?: number | null,
  expectedKey?: string | null,
  getCurrentSession?: () => TerminalSession | undefined,
): Promise<"exited" | "active" | "uncertain" | "conflict"> {
  // Pre-check current session state before query when accessor is provided
  if (getCurrentSession) {
    const pre = getCurrentSession();
    if (!pre) {
      return "uncertain";
    }
    if (
      pre.backendSessionId !== backendSessionId ||
      (expectedIncarnation && pre.incarnation !== expectedIncarnation) ||
      (expectedEpoch && pre.daemonEpoch !== expectedEpoch) ||
      (expectedToken !== undefined && expectedToken !== null && pre.recoveryToken !== expectedToken) ||
      (expectedKey && (!pre.recoveryKey || pre.recoveryKey !== expectedKey))
    ) {
      return "conflict";
    }
  }
  try {
    const details = await describeTerminal(backendSessionId);
    if (!details) {
      return "uncertain";
    }

    // Target response validation:
    if (details.sessionId !== backendSessionId) {
      return "uncertain";
    }

    // Paired proxy actor absence is connection loss / uncertainty (INV-4), NEVER active PTY and NEVER process death
    if (details.isPairedProxy || details.recoveryAuthority?.isPairedProxy) {
      if (details.running && details.recoveryAuthority?.status === "proxy_actor_active") {
        return "active";
      }
      return "uncertain";
    }

    if (
      expectedIncarnation &&
      details.incarnation &&
      details.incarnation !== expectedIncarnation
    ) {
      return "conflict";
    }

    const authority = details.recoveryAuthority;
    const isAffirmative = Boolean(
      authority &&
      typeof authority === "object" &&
      authority.status === "affirmative_exit"
    );

    if (isAffirmative) {
      // Affirmative exit requires proven matching incarnation context
      if (!expectedIncarnation || !details.incarnation || details.incarnation !== expectedIncarnation) {
        return "uncertain";
      }

      // Affirmative exit requires proven matching owner epoch context
      const exitEpoch = authority?.daemonEpoch ?? details.daemonEpoch ?? null;
      if (!expectedEpoch || !exitEpoch || exitEpoch !== expectedEpoch) {
        return "uncertain";
      }

      // Affirmative exit requires proven matching numeric token context
      if (expectedToken === undefined || expectedToken === null) {
        return "uncertain";
      }

      // Late rebind / replacement check post-await against current session state:
      if (getCurrentSession) {
        const current = getCurrentSession();
        if (!current) return "uncertain";
        if (
          current.backendSessionId !== backendSessionId ||
          current.incarnation !== expectedIncarnation ||
          (expectedEpoch && current.daemonEpoch !== expectedEpoch) ||
          (expectedToken !== undefined && expectedToken !== null && current.recoveryToken !== expectedToken) ||
          (expectedKey && (!current.recoveryKey || current.recoveryKey !== expectedKey))
        ) {
          return "conflict";
        }
      }

      dispatch({
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId,
        backendSessionId,
        incarnation: details.incarnation,
        daemonEpoch: exitEpoch,
        token: expectedToken,
        key: expectedKey ?? null,
        exitCode: authority?.exitCode ?? null,
        reason: authority?.exitReason ?? null,
      });

      // Post-await admission verification (missing session or un-admitted transition fails closed):
      if (getCurrentSession) {
        const post = getCurrentSession();
        if (!post || post.lifecycle !== "exited") {
          return "uncertain";
        }
      }
      return "exited";
    }

    // When details.running is false without affirmative matching proof:
    if (!details.running) {
      return "uncertain";
    }

    // If running, verify identity proof if expected:
    if (expectedIncarnation && !details.incarnation) {
      return "uncertain";
    }

    return "active";
  } catch {
    return "uncertain";
  }
}

export function selectAgents(state: WorkspaceState): ActiveAgent[] {
  const allTabs = getAllTabs(state);
  return allTabs.flatMap((tab) => {
    if (!isTerminalTab(tab)) return [];
    const session = state.sessions[tab.sessionId];
    if (!session) return [];
    const worktreePath = sessionWorktreePath(session);
    const worktree = state.worktrees.find((candidate) => candidate.path === worktreePath);
    const activity = state.activityBySessionId?.[session.id];
    const parsed = activity ? parseAgentTitle(activity.title) : null;
    return [
      {
        id: session.backendSessionId ?? session.id,
        name: parsed?.isAgent ? parsed.name : tab.label,
        task: parsed?.isAgent
          ? parsed.task
          : worktree?.branch?.replace(/^refs\/heads\//, "") ?? worktreePath,
        state: activity
          ? activityStateToAgentState(activity.state)
          // No activity entry means nothing has ever observed this pane doing agent work: a live
          // PTY alone is a shell prompt as often as it is a running agent, and claiming "working"
          // here spins the worktree row forever. Report the pane as merely alive instead.
          : session.lifecycle === "running" || session.lifecycle === "working"
            ? "starting"
            : session.lifecycle,
        worktree: session.worktree,
        worktreePath,
        sessionId: session.backendSessionId ?? session.id,
      },
    ];
  });
}

export function selectTabActivitySummaries(state: WorkspaceState): Record<string, ActivitySummary> {
  const result: Record<string, ActivitySummary> = {};
  const activityBySessionId = state.activityBySessionId ?? {};
  const allTabs = getAllTabs(state);

  for (const tab of allTabs) {
    if (!isTerminalTab(tab)) continue;
    const sessionIds = getTabSessionIds(state, tab.id);
    const activities = [...sessionIds]
      .map((sessionId) => activityBySessionId[sessionId])
      .filter((activity): activity is TerminalActivity => Boolean(activity));
    result[tab.id] = summarizeActivities(activities, Boolean(state.unreadTabIds[tab.id]));
  }

  return result;
}

export function selectWorktreeActivitySummaries(state: WorkspaceState): Record<string, ActivitySummary> {
  const result: Record<string, ActivitySummary> = {};
  const activityBySessionId = state.activityBySessionId ?? {};
  const sessionIdsByWorktree = new Map<string, Set<string>>();
  const unreadTabsByWorktree = new Set<string>();
  const allTabs = getAllTabs(state);

  for (const tab of allTabs) {
    if (!isTerminalTab(tab)) continue;
    const sessionIds = getTabSessionIds(state, tab.id);
    const worktreePaths = new Set<string>();
    for (const sessionId of sessionIds) {
      const session = state.sessions[sessionId];
      if (!session) continue;
      const worktreePath = sessionWorktreePath(session);
      worktreePaths.add(worktreePath);
      const bucket = sessionIdsByWorktree.get(worktreePath) ?? new Set<string>();
      bucket.add(sessionId);
      sessionIdsByWorktree.set(worktreePath, bucket);
    }
    if (state.unreadTabIds[tab.id]) {
      for (const path of worktreePaths) unreadTabsByWorktree.add(path);
    }
  }

  for (const worktree of state.worktrees) {
    const sessionIds = sessionIdsByWorktree.get(worktree.path) ?? new Set<string>();
    const activities = [...sessionIds]
      .map((sessionId) => activityBySessionId[sessionId])
      .filter((activity): activity is TerminalActivity => Boolean(activity));
    result[worktree.path] = summarizeActivities(
      activities,
      Boolean(state.unreadWorktreePaths[worktree.path] || unreadTabsByWorktree.has(worktree.path)),
    );
  }

  return result;
}

/**
 * Worktree activity for every project EXCEPT the mounted one.
 *
 * The store is per project: `useWorkspaceStore({ workspaceId })` mounts one workspace at a time, so
 * an agent running in another project has no live store to feed the sidebar. Its rows would sit
 * blank while the agent works, then light up again the moment the user switched back. The snapshot
 * cache already holds each workspace's last state, which is what the sidebar reads here.
 *
 * The mounted workspace is excluded on purpose: its snapshot lags the live reducer state by a
 * render, and the caller merges this map UNDER the live one.
 */
export function selectWorktreeActivitySummariesAcrossWorkspaces(
  mountedWorkspaceId: string,
): Record<string, ActivitySummary> {
  const merged: Record<string, ActivitySummary> = {};

  for (const [workspaceId, snapshot] of listWorkspaceSnapshots()) {
    if (workspaceId === mountedWorkspaceId) continue;
    for (const [path, summary] of Object.entries(selectWorktreeActivitySummaries(snapshot))) {
      merged[path] = summary;
    }
  }

  return merged;
}

/**
 * Global Dock badge count, aligned with the pane-attention signal:
 * each workspace contributes the larger of (unseen attention sessions — the
 * same `done`/`waiting && !seen` state that lights pane highlight frames) and
 * (bell-only unread tabs), summed across the live workspace and every parked
 * workspace snapshot, so an agent finishing anywhere still lights the badge.
 */
export function selectGlobalUnreadBadgeCount(
  currentState?: WorkspaceState,
  currentWorkspaceId?: string,
  store: NotificationCenterStore = notificationCenterStore,
): number {
  const storeEntries = store.getSnapshot().entries;
  if (store !== notificationCenterStore || storeEntries.length > 0) {
    const mounted = currentState
      ? { workspaceId: currentState.workspaceId ?? currentWorkspaceId, activityBySessionId: currentState.activityBySessionId }
      : null;
    return countAttentionEntries(storeEntries, liveActivityLookup(mounted, listWorkspaceSnapshots()));
  }

  if (!currentState) return 0;

  const countAttention = (state: WorkspaceState): number =>
    Object.values(state.activityBySessionId ?? {}).filter(
      (activity) =>
        activity &&
        !activity.seen &&
        (activity.state === "done" || activity.state === "waiting"),
    ).length;
  const countUnreadTabs = (state: WorkspaceState): number =>
    Object.values(state.unreadTabIds ?? {}).filter(Boolean).length;
  const badgeFor = (state: WorkspaceState): number =>
    Math.max(countAttention(state), countUnreadTabs(state));

  let count = badgeFor(currentState);
  const targetWsId = currentState.workspaceId ?? currentWorkspaceId;
  for (const [workspaceId, snapshot] of listWorkspaceSnapshots()) {
    if (workspaceId === targetWsId) continue;
    count += badgeFor(snapshot);
  }
  return count;
}

export type ActivityNotificationTarget = {
  workspaceId?: string;
  workspaceLabel?: string;
  notificationSuppressed?: boolean;
  sessionId: string;
  tabId: string;
  worktreePath: string;
  worktreeLabel: string;
  agentLabel: string | undefined;
  terminalTitle: string;
  state: TerminalActivityState;
};

export type ActivityNotificationEvent = ActivityNotificationTarget & { previousState?: TerminalActivityState };

/** Match ProjectHeader labels without tying parked-workspace events to the active project. */
export function selectNotificationWorkspaceLabel(state: WorkspaceState): string | undefined {
  const workspaceId = state.workspaceId;
  if (!workspaceId) return undefined;
  let projects: RegisteredProject[];
  try {
    const stored: unknown = JSON.parse(getMigratedItem(PROJECTS_STORAGE_KEY) ?? "[]");
    if (!Array.isArray(stored)) return workspaceId;
    projects = stored.filter((project): project is RegisteredProject =>
      project && typeof project.workspaceId === "string" && typeof project.repoRoot === "string" &&
      hasValidProjectTarget(project),
    );
  } catch (error) {
    console.error("Failed to resolve notification workspace label", error);
    return workspaceId;
  }
  const project = projects.find((candidate) => candidate.workspaceId === workspaceId);
  const remote = project?.target?.kind === "ssh" ? project.target : null;
  if (!project || !remote) return workspaceId;
  const folderName = project.repoRoot.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).at(-1) ?? "/";
  const hostLabel = getCachedSshHosts()?.find((host) => host.id === remote.hostId)?.label ?? project.hostLabel ?? remote.hostId;
  const standalone = findGroupForWorkspace(groupProjects(projects), workspaceId)?.memberProjects.length === 1;
  return standalone && hostLabel ? `${folderName} (${hostLabel})` : folderName;
}

function notificationTarget(state: WorkspaceState, sessionId: string, tabId: string): ActivityNotificationTarget {
  const activity = state.activityBySessionId?.[sessionId];
  const worktreePath = sessionWorktreePath(state.sessions[sessionId]);
  const worktree = state.worktrees.find((candidate) => candidate.path === worktreePath);
  const parsed = parseAgentTitle(activity?.title ?? "");
  return {
    workspaceId: state.workspaceId, sessionId, tabId, worktreePath,
    workspaceLabel: selectNotificationWorkspaceLabel(state),
    worktreeLabel: worktree ? workspaceName(worktree) : "",
    agentLabel: parsed?.isAgent ? parsed.name : agentDisplayNameForType(activity?.isAgent ? activity.agentType : undefined),
    terminalTitle: activity?.title ?? "", state: activity?.state ?? "done",
    notificationSuppressed: activity?.notificationSuppressed,
  };
}

export function selectActivityNotificationTargets(state: WorkspaceState): ActivityNotificationTarget[] {
  const targets: ActivityNotificationTarget[] = [];
  const workspaceLabel = selectNotificationWorkspaceLabel(state);
  const activityBySessionId = state.activityBySessionId ?? {};

  for (const [sessionId, activity] of Object.entries(activityBySessionId)) {
    const tabId = findTabIdForSession(state, sessionId);
    if (!tabId) continue;
    const session = state.sessions[sessionId];
    const worktreePath = sessionWorktreePath(session);
    const worktree = state.worktrees.find((candidate) => candidate.path === worktreePath);
    const worktreeLabel = worktree ? workspaceName(worktree) : "";
    const parsed = parseAgentTitle(activity.title);
    // An extension-reported state carries agentType but no agent name in the title, so fall back
    // to the classified type rather than sending a nameless notification.
    const agentLabel = parsed?.isAgent
      ? parsed.name
      : agentDisplayNameForType(activity.isAgent ? activity.agentType : undefined);

    targets.push({
      workspaceId: state.workspaceId,
      workspaceLabel,
      notificationSuppressed: activity.notificationSuppressed,
      sessionId,
      tabId,
      worktreePath,
      worktreeLabel,
      agentLabel,
      terminalTitle: activity.title,
      state: activity.state,
    });
  }

  return targets;
}

function initWorkspaceState({
  workspaceId,
  initialWorktrees,
}: {
  workspaceId: string;
  initialWorktrees: Worktree[];
}): { state: WorkspaceState; recoveredFromHmr: boolean } {
  const hmrState = getHmrWorkspaceState(workspaceId);
  if (hmrState) {
    switchDebug("workspace.state.init", {
      workspaceId,
      source: "hmr",
      worktreeCount: hmrState.worktrees.length,
      tabCount: hmrState.layout.tabs.length,
      sessionCount: Object.keys(hmrState.sessions).length,
    });
    return { state: { ...hmrState, workspaceId }, recoveredFromHmr: true };
  }
  const snapshot = getWorkspaceSnapshot(workspaceId);
  if (snapshot) {
    switchDebug("workspace.state.init", {
      workspaceId,
      source: "snapshot",
      worktreeCount: snapshot.worktrees.length,
      tabCount: snapshot.layout.tabs.length,
      sessionCount: Object.keys(snapshot.sessions).length,
    });
    return { state: { ...snapshot, workspaceId }, recoveredFromHmr: false };
  }
  switchDebug("workspace.state.init", {
    workspaceId,
    source: "empty",
    initialWorktreeCount: initialWorktrees.length,
  });
  return { state: createInitialState(initialWorktrees, workspaceId), recoveredFromHmr: false };
}

function createInitialState(worktrees: Worktree[], workspaceId?: string): WorkspaceState {
  return {
    workspaceId,
    worktrees,
    activeWorktreePath: worktrees[0]?.path ?? null,
    sessions: {},
    layout: createLayoutState(),
    worktreeLayouts: {},
    unreadTabIds: {},
    bellUnreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

export function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  if (action.type === "LOCAL_RECOVERY_ATTEMPT") {
    const session = state.sessions[action.sessionId];
    if (!session) return state;
    return { ...state, sessions: { ...state.sessions, [action.sessionId]: {
      ...session, recoveryKey: action.key, recoveryToken: action.token,
    } } };
  }
  if (action.type === "LOCAL_SPLIT_UPDATE") {
    const current = state.sessions[action.session.id];
    if (!current) return state;
    const currentIntent = localSplitIntent(current);
    const incomingIntent = localSplitIntent(action.session);
    if (currentIntent && (!incomingIntent || incomingIntent.requestId !== currentIntent.requestId ||
      incomingIntent.generation < currentIntent.generation)) return state;
    return { ...state, sessions: { ...state.sessions, [action.session.id]: action.session } };
  }
  if (action.type === "LOCAL_SPLIT_REMOVE") {
    const sessions = { ...state.sessions };
    delete sessions[action.sessionId];
    const located = locateSessionAcrossLayouts(state, action.sessionId);
    if (!located || !located.leafId) {
      return { ...state, sessions };
    }
    const { worktreePath, tabId, leafId } = located;
    const targetLayout = worktreePath === null ? state.layout : state.worktreeLayouts?.[worktreePath];
    if (!targetLayout) {
      return { ...state, sessions };
    }
    const tabLayout = targetLayout.layoutsByTabId?.[tabId];
    if (!tabLayout || tabLayout.root.type === "leaf") {
      return { ...state, sessions };
    }
    let updatedLayout = layoutReducer(targetLayout, { type: "CLOSE_PANE", tabId, leafId });
    if (updatedLayout === targetLayout) {
      return { ...state, sessions };
    }
    const tab = targetLayout.tabs.find((candidate) => candidate.id === tabId);
    if (tab && isTerminalTab(tab) && action.sessionId === tab.sessionId) {
      const remainingLayout = updatedLayout.layoutsByTabId[tabId];
      const replacementSessionId = Object.values(remainingLayout?.sessionIdsByLeafId ?? {}).find(Boolean);
      if (replacementSessionId && replacementSessionId !== tab.sessionId) {
        updatedLayout = {
          ...updatedLayout,
          tabs: updatedLayout.tabs.map((candidate) =>
            candidate.id === tabId && isTerminalTab(candidate)
              ? { ...candidate, sessionId: replacementSessionId }
              : candidate,
          ),
        };
      }
    }
    if (worktreePath === null) {
      return { ...state, sessions, layout: updatedLayout };
    } else {
      return {
        ...state,
        sessions,
        worktreeLayouts: {
          ...state.worktreeLayouts,
          [worktreePath]: updatedLayout,
        },
      };
    }
  }
  switch (action.type) {
    case "RESTORE_WORKSPACE":
      return {
        ...action.state,
        sessions: { ...action.state.sessions, ...Object.fromEntries(
          Object.entries(state.sessions).filter(([id, current]) => {
            const incoming = action.state.sessions[id];
            return incoming && current.backendSessionId && (
              incoming.backendSessionId !== current.backendSessionId ||
              incoming.incarnation !== current.incarnation ||
              (localSplitIntent(incoming)?.generation ?? 0) < (localSplitIntent(current)?.generation ?? 0)
            );
          }),
        ) },
        workspaceId: action.state.workspaceId ?? state.workspaceId,
        worktreeLayouts: action.state.worktreeLayouts ?? {},
        unreadTabIds: action.state.unreadTabIds ?? {},
        unreadWorktreePaths: action.state.unreadWorktreePaths ?? {},
        activityBySessionId: action.state.activityBySessionId ?? {},
      };
    case "SET_WORKTREES": {
      const validWorktreePaths = new Set(action.worktrees.map((worktree) => worktree.path));
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).filter(([, session]) => validWorktreePaths.has(sessionWorktreePath(session))),
      );
      const activityBySessionId = Object.fromEntries(
        Object.entries(state.activityBySessionId ?? {}).filter(([sessionId]) => Boolean(sessions[sessionId])),
      );
      let layout = state.layout;
      for (const tab of state.layout.tabs) {
        const hasRetainedSession = [...getTabSessionIdsForLayout(state.layout, tab.id)].some((sessionId) => Boolean(sessions[sessionId]));
        if (isTerminalTab(tab) && !hasRetainedSession) {
          layout = layoutReducer(layout, { type: "CLOSE_TAB", tabId: tab.id });
        }
      }
      const worktreeLayouts: Record<string, LayoutState> = {};
      for (const [path, parkedLayout] of Object.entries(state.worktreeLayouts ?? {})) {
        if (!validWorktreePaths.has(path)) continue;
        let cleanedLayout = parkedLayout;
        for (const tab of parkedLayout.tabs) {
          const hasRetainedSession = [...getTabSessionIdsForLayout(parkedLayout, tab.id)].some((sessionId) => Boolean(sessions[sessionId]));
          if (isTerminalTab(tab) && !hasRetainedSession) {
            cleanedLayout = layoutReducer(cleanedLayout, { type: "CLOSE_TAB", tabId: tab.id });
          }
        }
        worktreeLayouts[path] = cleanedLayout;
      }
      let activeWorktreePath = state.activeWorktreePath;
      if (!action.worktrees.some((worktree) => worktree.path === activeWorktreePath)) {
        activeWorktreePath = action.worktrees[0]?.path ?? null;
        if (activeWorktreePath && worktreeLayouts[activeWorktreePath]) {
          layout = worktreeLayouts[activeWorktreePath];
          delete worktreeLayouts[activeWorktreePath];
        }
      }
      const allTabs = [...layout.tabs, ...Object.values(worktreeLayouts).flatMap((l) => l.tabs)];
      const unreadTabIds = Object.fromEntries(
        Object.entries(state.unreadTabIds).filter(([tabId]) => allTabs.some((tab) => tab.id === tabId)),
      );
      const unreadWorktreePaths = Object.fromEntries(
        Object.entries(state.unreadWorktreePaths).filter(([path]) => validWorktreePaths.has(path)),
      );
      return {
        ...state,
        worktrees: action.worktrees,
        activeWorktreePath,
        sessions,
        layout,
        worktreeLayouts,
        activityBySessionId,
        unreadTabIds,
        unreadWorktreePaths,
      };
    }
    case "UPDATE_BROWSER_TAB": {
      const updateLayout = (layout: LayoutState): LayoutState => {
        const browserId = getBrowserIdForTab(layout, action.tabId, action.browserId);
        if (!browserId) return layout;
        const tabs = layout.tabs.map((tab) =>
          tab.id === action.tabId && tab.kind === "browser" && tab.browserId === browserId
            ? { ...tab, ...action.updates }
            : tab,
        );
        const tabLayout = layout.layoutsByTabId[action.tabId];
        if (!tabLayout?.contentsByLeafId) return { ...layout, tabs };
        const contentsByLeafId = { ...tabLayout.contentsByLeafId };
        for (const [leafId, content] of Object.entries(contentsByLeafId)) {
          if (content.kind === "browser" && (content.browser?.browserId ?? content.browserId) === browserId) {
            const { kind: _kind, browser, ...legacyBrowser } = content;
            contentsByLeafId[leafId] = createBrowserPaneContent({
              ...legacyBrowser,
              ...browser,
              browserId,
              url: browser?.url ?? content.url ?? "about:blank",
              ...action.updates,
            });
          }
        }
        return {
          ...layout,
          tabs,
          layoutsByTabId: {
            ...layout.layoutsByTabId,
            [action.tabId]: { ...tabLayout, contentsByLeafId },
          },
        };
      };
      const layout = updateLayout(state.layout);
      if (layout !== state.layout) return { ...state, layout };
      const worktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
      for (const [wtPath, parkedLayout] of Object.entries(worktreeLayouts)) {
        worktreeLayouts[wtPath] = updateLayout(parkedLayout);
      }
      return { ...state, worktreeLayouts };
    }
    case "SELECT_WORKTREE": {
      if (!state.worktrees.some((worktree) => worktree.path === action.path)) return state;
      if (action.path === state.activeWorktreePath) {
        return state.layout.activeTabId
          ? workspaceReducer(state, { type: "ACTIVATE_TAB", tabId: state.layout.activeTabId })
          : state;
      }

      const nextWorktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
      if (state.activeWorktreePath) {
        nextWorktreeLayouts[state.activeWorktreePath] = state.layout;
      }
      const nextLayout = nextWorktreeLayouts[action.path] ?? createLayoutState();
      delete nextWorktreeLayouts[action.path];

      const selected = {
        ...state,
        activeWorktreePath: action.path,
        layout: nextLayout,
        worktreeLayouts: nextWorktreeLayouts,
      };
      return nextLayout.activeTabId
        ? workspaceReducer(selected, { type: "ACTIVATE_TAB", tabId: nextLayout.activeTabId })
        : selected;
    }
    case "MARK_TAB_UNREAD": {
      if (action.observed !== false && isTabVisible(state, action.tabId)) return state;
      const worktreePath = getTabWorktreePath(state, action.tabId);
      return {
        ...state,
        unreadTabIds: { ...state.unreadTabIds, [action.tabId]: true },
        bellUnreadTabIds: { ...state.bellUnreadTabIds, [action.tabId]: true },
        unreadWorktreePaths: worktreePath
          ? { ...state.unreadWorktreePaths, [worktreePath]: true }
          : state.unreadWorktreePaths,
      };
    }
    case "CLEAR_TAB_UNREAD": {
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      const bellUnreadTabIds = { ...state.bellUnreadTabIds };
      delete bellUnreadTabIds[action.tabId];
      return clearWorktreeUnreadWhenRead({ ...state, unreadTabIds, bellUnreadTabIds }, action.tabId);
    }
    case "MARK_WORKTREE_UNREAD":
      return {
        ...state,
        unreadWorktreePaths: { ...state.unreadWorktreePaths, [action.worktreePath]: true },
      };
    case "CLEAR_WORKTREE_UNREAD": {
      const unreadWorktreePaths = { ...state.unreadWorktreePaths };
      delete unreadWorktreePaths[action.worktreePath];
      return { ...state, unreadWorktreePaths };
    }
    case "ADD_TAB_WITH_SESSION": {
      const sessions = action.session ? { ...state.sessions, [action.session.id]: action.session } : state.sessions;
      const sessionId = action.session?.id;
      const targetWorktreePath =
        action.targetWorktreePath ??
        action.session?.worktreePath ??
        (action.tab.kind === "browser" ? action.tab.worktreePath : undefined);

      if (targetWorktreePath && state.activeWorktreePath && targetWorktreePath !== state.activeWorktreePath) {
        const targetLayout = state.worktreeLayouts?.[targetWorktreePath] ?? createLayoutState();
        const updatedLayout = layoutReducer(targetLayout, { type: "ADD_TAB", tab: action.tab, sessionId });
        return {
          ...state,
          sessions,
          worktreeLayouts: {
            ...(state.worktreeLayouts ?? {}),
            [targetWorktreePath]: updatedLayout,
          },
        };
      }

      return {
        ...state,
        sessions,
        layout: layoutReducer(state.layout, { type: "ADD_TAB", tab: action.tab, sessionId }),
      };
    }
    case "REORDER_TAB":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "RENAME_TAB":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "SET_TAB_PINNED":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "SPLIT_PANE": {
      const requestedTabLayout = state.layout.layoutsByTabId?.[action.tabId];
      const requestedLeafIds = requestedTabLayout ? collectLeafIds(requestedTabLayout.root) : null;
      // `targetLeafId` is optional on this action; normalize so the discriminator below can name
      // the requested leaf without widening `includes` to `string | undefined`.
      const requestedTargetLeafId = action.targetLeafId ?? "";
      const layout = layoutReducer(state.layout, {
        type: "SPLIT_PANE",
        tabId: action.tabId,
        targetLeafId: action.targetLeafId,
        direction: action.direction,
        position: action.position,
        newLeafId: action.newLeafId,
        content: action.content,
        sessionId: action.session.id,
      });
      if (layout === state.layout) {
        // The layout refused the split, so the session is deliberately not inserted below and the
        // caller's lifecycle reads `undefined` and returns. That refusal used to be invisible:
        // `splitPane` logs `split.pane.dispatched` unconditionally, so a no-op here is
        // indistinguishable from a split that succeeded and whose backend call went missing.
        switchDebug("split.pane.layout-noop", {
          reason: !requestedTabLayout
            ? `tab-not-in-layout:${action.tabId}`
            : !requestedLeafIds?.includes(requestedTargetLeafId)
              ? `target-leaf-not-in-layout:${requestedTargetLeafId}`
              : requestedLeafIds?.includes(action.newLeafId)
                ? `new-leaf-already-present:${action.newLeafId}`
                : "layout-unchanged",
          tabId: action.tabId,
          targetLeafId: action.targetLeafId,
          newLeafId: action.newLeafId,
          existingLeafIds: requestedLeafIds ? requestedLeafIds.join("|") : "-",
        });
        return state;
      }
      return {
        ...state,
        sessions: { ...state.sessions, [action.session.id]: action.session },
        layout,
      };
    }
    case "MOVE_TAB_TO_GROUP": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.sourceTabId];
      return { ...state, layout, unreadTabIds };
    }
    case "MOVE_TAB_TO_SPLIT": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.sourceTabId];
      return { ...state, layout, unreadTabIds };
    }
    case "MOVE_TAB_TO_PANE_SPLIT": {
      const layout = moveTabIntoPaneSplit(
        state.layout,
        action.sourceTabId,
        action.targetTabId,
        action.targetLeafId,
        action.direction,
        action.position,
      );
      if (layout === state.layout) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.sourceTabId];
      return { ...state, layout, unreadTabIds };
    }
    case "DETACH_PANE_TO_TAB": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      return { ...state, layout };
    }
    case "CLOSE_PANE": {
      const tab = state.layout.tabs.find((candidate) => candidate.id === action.tabId);
      const tabLayout = state.layout.layoutsByTabId?.[action.tabId];
      if (!tab || !isTerminalTab(tab) || !tabLayout) return state;
      if (tabLayout.root.type === "leaf" && tabLayout.root.leafId === action.leafId) {
        return workspaceReducer(state, { type: "CLOSE_TAB", tabId: action.tabId });
      }
      const closingSessionId = tabLayout.sessionIdsByLeafId[action.leafId];
      let layout = layoutReducer(state.layout, {
        type: "CLOSE_PANE",
        tabId: action.tabId,
        leafId: action.leafId,
      });
      if (layout === state.layout) return state;

      if (closingSessionId === tab.sessionId) {
        const remainingLayout = layout.layoutsByTabId[action.tabId];
        const replacementSessionId = Object.values(remainingLayout?.sessionIdsByLeafId ?? {}).find(Boolean);
        if (replacementSessionId && replacementSessionId !== tab.sessionId) {
          layout = {
            ...layout,
            tabs: layout.tabs.map((candidate) =>
              candidate.id === action.tabId && isTerminalTab(candidate)
                ? { ...candidate, sessionId: replacementSessionId }
                : candidate,
            ),
          };
        }
      }

      const candidateState: WorkspaceState = { ...state, layout };
      if (!closingSessionId || isSessionReferenced(candidateState, closingSessionId)) return candidateState;
      const sessions = { ...state.sessions };
      if (!localSplitIntent(sessions[closingSessionId])) delete sessions[closingSessionId];
      const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
      delete activityBySessionId[closingSessionId];
      return { ...candidateState, sessions, activityBySessionId };
    }
    case "CLOSE_TAB": {
      const closingSessionIds = getTabSessionIds(state, action.tabId);
      const sessions = { ...state.sessions };
      const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
      for (const sessionId of closingSessionIds) {
        if (isSessionReferencedOutsideTab(state, sessionId, action.tabId)) continue;
        if (!localSplitIntent(sessions[sessionId])) delete sessions[sessionId];
        delete activityBySessionId[sessionId];
      }
      if (action.replacement?.session) sessions[action.replacement.session.id] = action.replacement.session;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];

      const inActiveLayout = state.layout.tabs.some((t) => t.id === action.tabId);
      let layout = state.layout;
      let worktreeLayouts = state.worktreeLayouts;

      if (inActiveLayout) {
        layout = layoutReducer(state.layout, {
          type: "CLOSE_TAB",
          tabId: action.tabId,
          replacementTab: action.replacement?.tab,
        });
      } else if (worktreeLayouts) {
        const nextWorktreeLayouts = { ...worktreeLayouts };
        for (const [wtPath, parkedLayout] of Object.entries(nextWorktreeLayouts)) {
          if (parkedLayout.tabs.some((t) => t.id === action.tabId)) {
            nextWorktreeLayouts[wtPath] = layoutReducer(parkedLayout, {
              type: "CLOSE_TAB",
              tabId: action.tabId,
              replacementTab: action.replacement?.tab,
            });
          }
        }
        worktreeLayouts = nextWorktreeLayouts;
      }

      const nextState = {
        ...state,
        sessions,
        activityBySessionId,
        unreadTabIds,
        layout,
        worktreeLayouts,
      };
      return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
    }
    case "UPDATE_FILE_TAB":
      return {
        ...state,
        layout: layoutReducer(state.layout, action),
      };
    case "ACTIVATE_TAB": {
      if (state.layout.tabs.some((tab) => tab.id === action.tabId)) {
        const unreadTabIds = { ...state.unreadTabIds };
        delete unreadTabIds[action.tabId];
        const nextState = acknowledgeTabCompletions(
          {
            ...state,
            unreadTabIds,
            layout: layoutReducer(state.layout, { type: "ACTIVATE_TAB", tabId: action.tabId }),
          },
          action.tabId,
        );
        return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
      }
      for (const [wtPath, parkedLayout] of Object.entries(state.worktreeLayouts ?? {})) {
        if (parkedLayout.tabs.some((tab) => tab.id === action.tabId)) {
          const nextWorktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
          if (state.activeWorktreePath) {
            nextWorktreeLayouts[state.activeWorktreePath] = state.layout;
          }
          delete nextWorktreeLayouts[wtPath];
          const nextLayout = layoutReducer(parkedLayout, { type: "ACTIVATE_TAB", tabId: action.tabId });
          const unreadTabIds = { ...state.unreadTabIds };
          delete unreadTabIds[action.tabId];
          const unreadWorktreePaths = { ...state.unreadWorktreePaths };
          delete unreadWorktreePaths[wtPath];
          const nextState = acknowledgeTabCompletions(
            {
              ...state,
              activeWorktreePath: wtPath,
              layout: nextLayout,
              worktreeLayouts: nextWorktreeLayouts,
              unreadTabIds,
              unreadWorktreePaths,
            },
            action.tabId,
          );
          return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
        }
      }
      return state;
    }
    case "FOCUS_PANE": {
      const layout = layoutReducer(state.layout, { type: "FOCUS_PANE", tabId: action.tabId, leafId: action.leafId });
      const focused = acknowledgeTabCompletions({ ...state, layout }, action.tabId);
      return workspaceReducer(focused, { type: "CLEAR_TAB_UNREAD", tabId: action.tabId });
    }
    case "FOCUS_EXISTING_SESSION": {
      // Navigate to a live session's exact pane using the current inventory. A stale or closed
      // session is rejected with no navigation and no fallback selection or terminal spawn.
      if (!state.sessions[action.sessionId]) return state;
      const found = locateSessionAcrossLayouts(state, action.sessionId);
      if (!found) return state;

      // Select the owning worktree first, parking the current active layout instead of discarding
      // it. Every live session in either layout is preserved.
      let working = state;
      if (found.worktreePath && found.worktreePath !== state.activeWorktreePath) {
        const nextWorktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
        if (state.activeWorktreePath) nextWorktreeLayouts[state.activeWorktreePath] = state.layout;
        const nextLayout = nextWorktreeLayouts[found.worktreePath];
        delete nextWorktreeLayouts[found.worktreePath];
        working = { ...state, activeWorktreePath: found.worktreePath, layout: nextLayout, worktreeLayouts: nextWorktreeLayouts };
      }

      // A single layout-level FOCUS_PANE activates the owning tab group, the tab, and the target
      // leaf together. Setting the active leaf BEFORE acknowledging is what limits acknowledgment
      // to the intended session: a split tab's sibling panes keep their unseen attention.
      let layout = found.leafId
        ? layoutReducer(working.layout, { type: "FOCUS_PANE", tabId: found.tabId, leafId: found.leafId })
        : layoutReducer(working.layout, { type: "ACTIVATE_TAB", tabId: found.tabId });

      // FOCUS_PANE leaves expandedLeafId untouched, so a zoomed sibling would keep the target
      // hidden. Move the expansion onto the target (existing TOGGLE_PANE_EXPANDED semantics) so
      // it is actually visible; a tab with nothing expanded is left alone.
      if (found.leafId) {
        const tabLayout = layout.layoutsByTabId?.[found.tabId];
        if (tabLayout && tabLayout.expandedLeafId && tabLayout.expandedLeafId !== found.leafId) {
          layout = layoutReducer(layout, { type: "TOGGLE_PANE_EXPANDED", tabId: found.tabId, leafId: found.leafId });
        }
      }

      const unreadTabIds = { ...working.unreadTabIds };
      delete unreadTabIds[found.tabId];
      const acknowledged = acknowledgeTabCompletions({ ...working, layout, unreadTabIds }, found.tabId);
      return clearWorktreeUnreadWhenRead(acknowledged, found.tabId, working);
    }
    case "SET_PANE_RATIO": {
      const layout = layoutReducer(state.layout, {
        type: "SET_PANE_RATIO",
        tabId: action.tabId,
        path: action.path,
        ratio: action.ratio,
        isolated: action.isolated,
        seam: action.seam,
      });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SET_TAB_GROUP_RATIO": {
      const layout = layoutReducer(state.layout, { type: "SET_TAB_GROUP_RATIO", path: action.path, ratio: action.ratio });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "EQUALIZE_TAB_GROUP_RUN": {
      const layout = layoutReducer(state.layout, action);
      return layout === state.layout ? state : { ...state, layout };
    }
    case "EQUALIZE_PANE_RUN": {
      const layout = layoutReducer(state.layout, action);
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SWAP_PANES": {
      const layout = layoutReducer(state.layout, { type: "SWAP_PANES", tabId: action.tabId, sourceLeafId: action.sourceLeafId, targetLeafId: action.targetLeafId });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SUPPRESS_NEXT_ATTENTION": {
      return {
        ...state,
        attentionSuppressions: {
          ...(state.attentionSuppressions ?? {}),
          [action.sessionId]: Date.now(),
        },
      };
    }
    case "SESSION_REMOTE_STATUS": {
      const sessions = { ...state.sessions };
      const lostSessionIds: string[] = [];
      for (const [id, session] of Object.entries(sessions)) {
        if (session.backendSessionId !== action.status.sessionId || !isRemoteWorkspaceId(session.workspaceId)) continue;
        // Only an expired target failure confirms process loss. Retry exhaustion also emits an
        // expired state, but its transport failure leaves process ownership uncertain.
        // `missing` only says the daemon we queried does not know this session, which is transient
        // (a draining predecessor daemon may still own it, or restore has not completed), so it must
        // not be read as process death. `disconnected`/`reconnecting` is only the control channel
        // dropping: in all three cases the remote agent keeps working and its activity must survive.
        const lost = action.status.state === "expired" && action.status.failure?.kind === "expired";
        if (lost) {
          // Process death is not pane death: like a local pane whose PTY is gone, the pane keeps
          // its identity and agent on a standby backend and is resumed on the same remote host.
          lostSessionIds.push(id);
          sessions[id] = remoteSessionGone(session, action.status.failure);
          continue;
        }
        const epochChanged = action.daemonEpoch != null && action.daemonEpoch !== session.daemonEpoch;
        sessions[id] = {
          ...session,
          remoteConnectionState: action.status.state,
          remoteGeneration: action.status.generation,
          remoteFailure: action.status.failure,
          remoteReplayGap: action.status.replayGap,
          lifecycle: lost ? "exited" : action.status.state === "connected" ? "running" : session.lifecycle,
          ...(action.daemonEpoch != null ? { daemonEpoch: action.daemonEpoch } : {}),
          ...(epochChanged ? { lastOutputSequence: null } : {}),
        };
      }
      return settleActivityForDeadSessions({ ...state, sessions }, lostSessionIds);
    }
    case "REMOTE_SESSION_GONE": {
      const session = state.sessions[action.sessionId];
      if (!session || session.backendSessionId !== action.backendSessionId ||
          (!isPairedWorkspaceId(session.workspaceId) && !isRemoteWorkspaceId(session.workspaceId))) return state;
      return settleActivityForDeadSessions({ ...state, sessions: {
        ...state.sessions, [session.id]: remoteSessionGone(session, session.remoteFailure ?? null),
      } }, [session.id]);
    }
    case "LOCAL_SESSIONS_RECONCILED": {
      let changed = false;
      const deadSessionIds: string[] = [];
      const sessions = { ...state.sessions };
      for (const [id, session] of Object.entries(sessions)) {
        if (session.remoteConnectionState !== "reconnecting") continue;
        if (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)) continue;
        changed = true;
        if (action.live === null) {
          sessions[id] = { ...session, remoteConnectionState: "reconnecting" };
          continue;
        }
        const liveInfo = session.backendSessionId ? action.live.get(session.backendSessionId) : undefined;
        if (liveInfo && liveInfo.running !== false) {
          const epochChanged = liveInfo.daemonEpoch != null && liveInfo.daemonEpoch !== session.daemonEpoch;
          const liveIncarnation = liveInfo.incarnation ?? null;
          const sessionIncarnation = session.incarnation ?? null;
          const sameIncarnationProven = sessionIncarnation && liveIncarnation && sessionIncarnation === liveIncarnation;
          if (!sameIncarnationProven) {
            sessions[id] = {
              ...session,
              remoteConnectionState: sessionIncarnation && liveIncarnation ? "disconnected" : "reconnecting",
              reconnectLifecycle: sessionIncarnation && liveIncarnation ? "failed" : "idle",
            };
          } else {
            sessions[id] = {
              ...session,
              remoteConnectionState: undefined,
              lifecycle: "running",
              ...(liveInfo.daemonEpoch != null ? { daemonEpoch: liveInfo.daemonEpoch } : {}),
              ...(epochChanged ? { lastOutputSequence: null } : {}),
              ...(liveIncarnation ? { incarnation: liveIncarnation } : {}),
            };
          }
        } else if (
          liveInfo?.recoveryAuthority &&
          liveInfo.recoveryAuthority.status === "affirmative_exit" &&
          liveInfo.incarnation &&
          session.incarnation &&
          liveInfo.incarnation === session.incarnation
          && liveInfo.recoveryAuthority.incarnation === session.incarnation
          && Boolean(session.daemonEpoch && liveInfo.recoveryAuthority.daemonEpoch === session.daemonEpoch && liveInfo.daemonEpoch === session.daemonEpoch)
        ) {
          deadSessionIds.push(id);
          sessions[id] = {
            ...session,
            lifecycle: "exited",
            backendSessionId: null,
            remoteConnectionState: undefined,
            reconnectLifecycle: "idle",
            exitCode: liveInfo.recoveryAuthority.exitCode ?? null,
            backendUnavailableReason: liveInfo.recoveryAuthority.exitReason ?? null,
          };
        } else {
          sessions[id] = {
            ...session,
            remoteConnectionState: "reconnecting",
            reconnectLifecycle: "idle",
          };
        }
      }
      if (!changed) return state;
      return settleActivityForDeadSessions({ ...state, sessions }, deadSessionIds);
    }
    case "SESSION_LIFECYCLE": {
      const matchedSessionIds: string[] = [];
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).map(([id, session]) => {
          if (session.backendSessionId !== action.backendSessionId) return [id, session];
          // For terminal exit/failed lifecycle events:
          if (action.lifecycle === "exited" || action.lifecycle === "failed") {
            // Proven matching current incarnation context is required to establish exit.
            // An unfenced lifecycle event (missing incarnation or mismatched incarnation)
            // must NOT kill the pane; it requests reconcile safely instead.
            const hasProvenIncarnationContext = Boolean(
              action.incarnation &&
              session.incarnation &&
              action.incarnation === session.incarnation
            );
            const matchesEpoch = Boolean(
              action.daemonEpoch && session.daemonEpoch && action.daemonEpoch === session.daemonEpoch
            );

            if (!hasProvenIncarnationContext || !matchesEpoch) {
              // Unfenced lifecycle event triggers reconcile safely, preserving backendSessionId & activity
              return [id, { ...session, remoteConnectionState: "reconnecting" }];
            }
          }
          if (isRemoteWorkspaceId(session.workspaceId) && action.lifecycle === "exited") {
            return [id, { ...session, remoteConnectionState: "reconnecting", remoteGeneration: null }];
          }
          matchedSessionIds.push(id);
          const dead = action.lifecycle === "exited" || action.lifecycle === "failed";
          return [
            id,
            {
              ...session,
              lifecycle: action.lifecycle,
              exitCode: action.lifecycle === "exited" ? action.exitCode ?? null : null,
              backendSessionId: dead ? null : session.backendSessionId,
              reconnectLifecycle: dead ? "idle" : session.reconnectLifecycle,
              ...(dead ? clearedTransportState(session) : {}),
            },
          ];
        }),
      ) as Record<string, TerminalSession>;
      let nextState: WorkspaceState = { ...state, sessions };
      // `failed` is as terminal as `exited`: the PTY is gone either way, so an in-flight work
      // claim can never be retired by a later event and must settle here.
      if (action.lifecycle === "exited" || action.lifecycle === "failed") {
        nextState = settleActivityForDeadSessions(nextState, matchedSessionIds);
      }
      return nextState;
    }
    case "SESSION_AFFIRMATIVE_EXIT": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      if (session.backendSessionId !== action.backendSessionId) return state;
      // Strict affirmative proof requirement: missing or mismatched incarnation rejected
      if (!action.incarnation || !session.incarnation || action.incarnation !== session.incarnation) {
        return state;
      }
      // Complete captured authority context required: missing epoch or missing token must not become positive exit
      if (!action.daemonEpoch || !session.daemonEpoch || action.daemonEpoch !== session.daemonEpoch) {
        return state;
      }
      if (
        action.token === undefined ||
        action.token === null ||
        session.recoveryToken === undefined ||
        session.recoveryToken === null ||
        action.token !== session.recoveryToken
      ) {
        return state;
      }
      if (!action.key || !session.recoveryKey || action.key !== session.recoveryKey) {
        return state;
      }
      const updatedSession: TerminalSession = {
        ...session,
        lifecycle: "exited",
        exitCode: action.exitCode ?? null,
        backendSessionId: null,
        reconnectLifecycle: "idle",
        ...clearedTransportState(session),
        backendUnavailableReason: action.reason ?? null,
        recoveryState: { phase: "exited", reason: action.reason ?? null },
      };
      let nextState: WorkspaceState = {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: updatedSession,
        },
      };
      nextState = settleActivityForDeadSessions(nextState, [action.sessionId]);
      return nextState;
    }
    case "SESSION_CONFIRMED_ABSENT": {
      const session = state.sessions[action.sessionId];
      if (!session || session.backendSessionId !== action.backendSessionId ||
          session.recoveryKey !== action.key || session.recoveryToken !== action.token) return state;
      // Same settled shape as an affirmative exit: the pane offers a new shell, and an agent pane
      // keeps its agent and provider session so it can resume the conversation.
      return settleActivityForDeadSessions({ ...state, sessions: { ...state.sessions, [session.id]: {
        ...session,
        lifecycle: "exited",
        backendSessionId: null,
        reconnectLifecycle: "idle",
        ...clearedTransportState(session),
        backendUnavailableReason: "absent",
        recoveryState: { phase: "exited", reason: "absent" },
      } } }, [session.id]);
    }
    case "SESSION_BACKEND_UNAVAILABLE": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      // Explicit spawn rejection has no retained process whose status is uncertain.
      if (action.spawnFailed && action.backendSessionId === null && session.backendSessionId === null) {
        return settleActivityForDeadSessions({ ...state, sessions: { ...state.sessions, [session.id]: {
          ...session, lifecycle: "exited", reconnectLifecycle: "idle",
          remoteConnectionState: "disconnected", backendUnavailableReason: action.reason ?? null,
          recoveryState: { phase: "exited", reason: action.reason ?? null },
        } } }, [session.id]);
      }
      if (action.backendSessionId !== null) {
        if (session.backendSessionId !== action.backendSessionId) return state;
      } else {
        if (session.backendSessionId !== null) return state;
      }
      if (action.bindingKey) {
        const expected = [
          session.backendSessionId ?? "",
          session.daemonEpoch ?? "",
          session.remoteGeneration ?? "",
          session.incarnation ?? "",
        ].join(":");
        if (action.bindingKey !== expected) {
          return state;
        }
      }

      const sessionHasOwnerContext = (session.recoveryKey !== undefined && session.recoveryKey !== null) ||
                                     (session.recoveryToken !== undefined && session.recoveryToken !== null);
      const actionHasAttemptEvidence = (action.attemptKey !== undefined && action.attemptKey !== null) ||
                                       (action.attemptToken !== undefined && action.attemptToken !== null);

      if (sessionHasOwnerContext || actionHasAttemptEvidence) {
        // Strict verification against current recovery owner key and token (no coercion, no fail-open):
        if (
          !action.attemptKey ||
          !session.recoveryKey ||
          action.attemptKey !== session.recoveryKey ||
          action.attemptToken === undefined ||
          action.attemptToken === null ||
          session.recoveryToken === undefined ||
          session.recoveryToken === null ||
          action.attemptToken !== session.recoveryToken
        ) {
          return state; // Stale, mismatched, or absent key/token rejected (INV-7)
        }

        if (localSplitIntent(session)) {
          return {
            ...state,
            sessions: {
              ...state.sessions,
              [session.id]: {
                ...session,
                reconnectLifecycle: "failed",
                recoveryState: { phase: "blocked", reason: action.reason },
                reconnectError: {
                  code: "INTERNAL_ERROR",
                  message: action.reason,
                  details: { delivery: "confirmed", stage: "presentation" },
                },
              },
            },
          };
        }

        if (isRemoteWorkspaceId(session.workspaceId)) {
          return {
            ...state,
            sessions: {
              ...state.sessions,
              [action.sessionId]: {
                ...session,
                remoteConnectionState: "disconnected",
                remoteGeneration: null,
                remoteFailure: {
                  kind: "network",
                  message: action.reason ?? "Failed to connect terminal",
                },
                reconnectLifecycle: action.outcome === "blocked" ? "failed" : (session.reconnectLifecycle ?? "idle"),
                backendUnavailableReason: action.reason ?? null,
                recoveryState: action.recoveryState ?? { phase: action.outcome === "blocked" ? "blocked" : "recovering", reason: action.reason },
              },
            },
          };
        }

        const updatedSession: TerminalSession = {
          ...session,
          remoteConnectionState: "reconnecting",
          reconnectLifecycle: action.outcome === "blocked" ? "failed" : (session.reconnectLifecycle ?? "idle"),
          backendUnavailableReason: action.reason ?? null,
          recoveryState: action.recoveryState ?? { phase: action.outcome === "blocked" ? "blocked" : "recovering", reason: action.reason },
        };

        return {
          ...state,
          sessions: {
            ...state.sessions,
            [action.sessionId]: updatedSession,
          },
        };
      } else {
        // Legacy path (neither session nor action has recoveryKey/recoveryToken):
        if (localSplitIntent(session)) {
          return {
            ...state,
            sessions: {
              ...state.sessions,
              [session.id]: {
                ...session,
                reconnectLifecycle: "failed",
                recoveryState: { phase: "blocked", reason: action.reason },
                reconnectError: {
                  code: "INTERNAL_ERROR",
                  message: action.reason,
                  details: { delivery: "confirmed", stage: "presentation" },
                },
              },
            },
          };
        }

        if (isRemoteWorkspaceId(session.workspaceId)) {
          return {
            ...state,
            sessions: {
              ...state.sessions,
              [action.sessionId]: {
                ...session,
                remoteConnectionState: "disconnected",
                remoteGeneration: null,
                remoteFailure: {
                  kind: "network",
                  message: action.reason ?? "Failed to connect terminal",
                },
                reconnectLifecycle: session.reconnectLifecycle ?? "idle",
                backendUnavailableReason: action.reason ?? null,
                recoveryState: { phase: "recovering", reason: action.reason },
              },
            },
          };
        }

        return {
          ...state,
          sessions: {
            ...state.sessions,
            [action.sessionId]: {
              ...session,
              remoteConnectionState: "reconnecting",
              backendUnavailableReason: action.reason ?? null,
              recoveryState: { phase: "recovering", reason: action.reason },
            },
          },
        };
      }
    }
    case "SET_RECONNECT_LIFECYCLE": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      return {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            reconnectLifecycle: action.lifecycle,
            reconnectError: action.error ?? null,
            reconnectRequestId: action.requestId ?? session.reconnectRequestId ?? null,
          },
        },
      };
    }
    case "APPLY_PROVIDER_SESSION": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      // The freshest answer wins. Keeping the id already on the session is what pinned a pane to
      // the first conversation it ever ran: `/new` rotates the agent's session id in place, and a
      // pane restored from disk always arrives with the previous id already set.
      const nextAgentType = session.agentType ?? (action.agentType || null);
      if (
        sameProviderSession(session.providerSession, action.providerSession)
        && nextAgentType === session.agentType
      ) {
        return state;
      }
      return {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            providerSession: action.providerSession,
            ...(nextAgentType ? { agentType: nextAgentType } : {}),
          },
        },
      };
    }
    case "REBIND_SESSION_BACKEND": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      // A changed backend id means the old remote process was replaced: drop agent
      // identity and activity. An unchanged id is a same-process reattach (the daemon
      // restarted around the same PTY); the pane keeps its agent and generation.
      const isSshBackendReplaced = isRemoteWorkspaceId(session.workspaceId) && session.backendSessionId !== action.backendSessionId;
      const isPairedBackendReplaced = isPairedWorkspaceId(session.workspaceId) && session.backendSessionId !== action.backendSessionId;
      const shouldClearAgent =
        action.clearAgent === true ||
        (action.clearAgent === undefined && (isSshBackendReplaced || isPairedBackendReplaced));
      const activityBySessionId = { ...state.activityBySessionId };
      if (shouldClearAgent) delete activityBySessionId[action.sessionId];
      return {
        ...state,
        ...(shouldClearAgent ? { activityBySessionId } : {}),
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            backendSessionId: action.backendSessionId,
            processState: "running",
            exitCode: null,
            cwd: action.cwd && isAbsoluteTerminalCwd(action.cwd) ? action.cwd : session.cwd,
            daemonEpoch: action.daemonEpoch ?? null,
            ...(action.incarnation != null ? { incarnation: action.incarnation } : {}),
            recoveryKey: action.recoveryKey ?? session.recoveryKey ?? formatRecoveryKey({
              kind: isRemoteWorkspaceId(session.workspaceId) ? "ssh" : "local",
              frontendSessionId: action.sessionId,
              backendSessionId: action.backendSessionId,
              expectedIncarnation: action.incarnation ?? session.incarnation ?? null,
            }),
            ...(action.recoveryToken != null
              ? { recoveryToken: action.recoveryToken }
              : session.recoveryToken != null
                ? { recoveryToken: session.recoveryToken + 1 }
                : {}),
            lastOutputSequence: null,
            lifecycle: action.pendingAttachment ? session.lifecycle : "running",
            reconnectLifecycle: action.pendingAttachment ? "binding" : "idle",
            reconnectError: null,
            backendUnavailableReason: null,
            recoveryState: null,
            reconnectRequestId: action.pendingAttachment ? session.reconnectRequestId : null,
            ...(shouldClearAgent
              ? {
                  agentType: null,
                  agentSessionId: null,
                  providerSession: null,
                }
              : {}),
            ...(isSshBackendReplaced || isPairedBackendReplaced
              ? {
                  remoteConnectionState: "connected",
                  remoteGeneration: 1,
                  remoteFailure: null,
                  remoteReplayGap: null,
                }
              : {
                  ...(session.remoteConnectionState !== undefined ? { remoteConnectionState: undefined } : {}),
                  ...(session.remoteFailure != null ? { remoteFailure: null } : {}),
                  ...(session.remoteReplayGap != null ? { remoteReplayGap: null } : {}),
                }),
          },
        },
      };
    }
    case "SESSION_SCREEN_ACTIVITY": {
      const previous = state.activityBySessionId?.[action.sessionId];
      let mappedState: TerminalActivityState;
      if (action.state === "working") {
        if (isSessionBackendDead(state, action.sessionId)) return state;
        mappedState = "working";
      } else if (action.state === "blocked") {
        if (isSessionBackendDead(state, action.sessionId)) return state;
        mappedState = "waiting";
      } else if (action.state === "idle") {
        if (!action.isSnapshot && (!previous || (previous.state !== "working" && previous.state !== "waiting"))) {
          return state;
        }
        mappedState = "done";
      } else {
        return state;
      }

      const authoritativeAgentType = state.sessions[action.sessionId]?.agentType ?? null;
      const normalizedManifestId = action.manifestId?.trim().toLowerCase();
      const isSupportedManifest = Boolean(normalizedManifestId && resolveAgentLogo(normalizedManifestId));
      const prevActivity = previous as WorkspaceTerminalActivity | undefined;
      const agentType = authoritativeAgentType
        ? authoritativeAgentType
        : isSupportedManifest
          ? normalizedManifestId
          : prevActivity?.agentType;
      const isAgent = authoritativeAgentType ? true : isSupportedManifest ? true : (prevActivity?.isAgent ?? false);
      const agentSource = authoritativeAgentType
        ? prevActivity?.agentSource
        : isSupportedManifest
          ? "screen"
          : prevActivity?.agentSource;

      const activity: WorkspaceTerminalActivity = {
        state: mappedState,
        title: previous?.title ?? "",
        isAgent,
        agentType,
        source: "screen",
        agentSource,
        ...(action.detail ? { detail: action.detail } : {}),
        ...(mappedState === "done" && action.outcome ? { outcome: action.outcome } : {}),
        ...(mappedState === "waiting" && action.kind ? { kind: action.kind } : {}),
        ...(action.isSnapshot && mappedState !== "working"
          ? { seen: true, notificationSuppressed: true }
          : {}),
      };
      let nextState = applySessionActivity(state, action.tabId, action.sessionId, activity, action.observed);
      const replacesUnseenAttention =
        previous?.seen !== true && (previous?.state === "done" || previous?.state === "waiting");
      if (action.isSnapshot && mappedState !== "working" && replacesUnseenAttention) {
        const tabSessionIds = getTabSessionIds(nextState, action.tabId);
        const hasOtherUnseenSessionInTab = [...tabSessionIds].some(
          (sId) => sId !== action.sessionId && nextState.activityBySessionId?.[sId]?.seen !== true,
        );
        const hasBellUnread = Boolean(nextState.bellUnreadTabIds?.[action.tabId]);
        if (!hasOtherUnseenSessionInTab && !hasBellUnread) {
          const unreadTabIds = { ...nextState.unreadTabIds };
          delete unreadTabIds[action.tabId];
          nextState = clearWorktreeUnreadWhenRead({ ...nextState, unreadTabIds }, action.tabId);
        }
      }
      const session = nextState.sessions[action.sessionId];
      if (!session) return nextState;

      const updatedProviderSession = action.providerSession ?? session.providerSession;
      const isAuthoritative = Boolean(agentType && canCaptureAuthoritativeProviderSession(agentType));
      const hasProviderSession = Boolean(updatedProviderSession);
      const shouldAdoptAgentType = Boolean(session.agentType || !isAuthoritative || hasProviderSession);
      const updatedAgentType = shouldAdoptAgentType ? (session.agentType ?? (agentType || null)) : session.agentType;

      if (
        updatedAgentType === session.agentType
        && sameProviderSession(session.providerSession, updatedProviderSession)
      ) {
        return nextState;
      }

      return {
        ...nextState,
        sessions: {
          ...nextState.sessions,
          [action.sessionId]: {
            ...session,
            ...(updatedAgentType ? { agentType: updatedAgentType } : {}),
            ...(updatedProviderSession ? { providerSession: updatedProviderSession } : {}),
          },
        },
      };
    }
    case "MARK_SESSION_ACTIVITY_SEEN": {
      // Acknowledging is user engagement: production acks ride pane clicks/focus that also fire
      // `ferryx:session-interacted`, so record the clock here too — the completion after fresh
      // post-ack work is user-attended, not an automation turn. Map write is idempotent.
      lastSessionEngagementAtBySession.set(action.sessionId, Date.now());
      const activity = state.activityBySessionId?.[action.sessionId];
      if (
        !activity ||
        (activity.state !== "done" && activity.state !== "waiting") ||
        activity.seen
      ) return state;
      return {
        ...state,
        activityBySessionId: {
          ...state.activityBySessionId,
          [action.sessionId]: { ...activity, seen: true },
        },
      };
    }
    case "RESET_AGENT_STATE": {
      let nextState = state;
      const attentionEpisodeAtBySession = { ...state.attentionEpisodeAtBySession };
      delete attentionEpisodeAtBySession[action.sessionId];
      nextState = { ...nextState, attentionEpisodeAtBySession };
      lastSessionEngagementAtBySession.delete(action.sessionId);
      if (state.activityBySessionId?.[action.sessionId]) {
        const activityBySessionId = { ...(nextState.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        nextState = { ...nextState, activityBySessionId };
      }
      const tabId = findTabIdForSession(nextState, action.sessionId);
      if (tabId && nextState.unreadTabIds[tabId]) {
        const tabSessionIds = getTabSessionIds(nextState, tabId);
        const hasOtherUnseenSessionInTab = [...tabSessionIds].some(
          (sId) => sId !== action.sessionId && nextState.activityBySessionId?.[sId]?.seen !== true,
        );
        const hasBellUnread = Boolean(nextState.bellUnreadTabIds?.[tabId]);
        if (!hasOtherUnseenSessionInTab && !hasBellUnread) {
          const unreadTabIds = { ...nextState.unreadTabIds };
          delete unreadTabIds[tabId];
          nextState = clearWorktreeUnreadWhenRead({ ...nextState, unreadTabIds }, tabId);
        }
      }
      return nextState;
    }
    case "SESSION_TITLE_ACTIVITY": {
      const previous = state.activityBySessionId?.[action.sessionId];
      const prevActivity = previous as WorkspaceTerminalActivity | undefined;
      const isScreenSource = previous?.source === "screen";
      const isScreenAgent = isScreenSource && prevActivity?.agentSource === "screen";

      if (isBareAgentTitle(action.title)) {
        if (isScreenSource) {
          const parsed = parseAgentTitle(action.title);
          const normalizedTitle = normalizeTerminalTitle(action.title);
          const isAgent = parsed?.isAgent ? true : (isScreenAgent ? (previous.isAgent ?? false) : false);
          const agentType = parsed?.isAgent ? parsed.agentType : (isScreenAgent ? previous.agentType : undefined);
          const agentSource = parsed?.isAgent ? "title" : (isScreenAgent ? "screen" : undefined);
          const activity: WorkspaceTerminalActivity = {
            state: previous.state,
            title: formatTabLabelFromTitle(action.title, normalizedTitle),
            isAgent,
            agentType,
            source: "screen",
            agentSource,
          };
          return applySessionActivity(state, action.tabId, action.sessionId, activity, action.observed);
        }
        if (!previous) return state;
        const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        return { ...state, activityBySessionId };
      }

      const parsed = parseAgentTitle(action.title);
      const classified = classifyTerminalTitleActivity(action.title);

      if (!classified && !previous) {
        return state;
      }

      // A shell prompt repainting the title (cwd, `zsh`, …) is not evidence that the agent
      // stopped: agents leave the title alone while a turn runs. Only drop the entry once the
      // run has actually settled, so an in-flight state survives the repaint the same way a
      // screen-derived one already does. The stale brand is still dropped below.
      const inFlight = previous?.state === "working" || previous?.state === "waiting";

      if (!classified && !parsed?.isAgent && !isScreenSource && !inFlight) {
        if (!previous) return state;
        const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        return { ...state, activityBySessionId };
      }

      const normalizedTitle = normalizeTerminalTitle(action.title);
      const isAgent = parsed?.isAgent
        ? true
        : isScreenAgent && inFlight
          ? (previous?.isAgent ?? false)
          : false;
      const agentType = parsed?.isAgent
        ? parsed.agentType
        : isScreenAgent && inFlight
          ? previous?.agentType
          : undefined;
      const agentSource = parsed?.isAgent
        ? "title"
        : isScreenAgent && inFlight
          ? "screen"
          : undefined;

      const activity: WorkspaceTerminalActivity = {
        state: isScreenSource ? previous.state : (classified ?? previous!.state),
        title: formatTabLabelFromTitle(action.title, normalizedTitle),
        isAgent,
        agentType,
        source: isScreenSource ? "screen" : (previous?.source ?? "title"),
        agentSource,
      };
      // A title is decoded from terminal output, which can still arrive after the PTY is gone.
      // Such a late frame must never raise a dead pane back to working/waiting; the label still
      // applies, so only the in-flight claim is downgraded to the settled state.
      const claimsInFlight = activity.state === "working" || activity.state === "waiting";
      const guarded: WorkspaceTerminalActivity =
        claimsInFlight && isSessionBackendDead(state, action.sessionId)
          ? { ...activity, state: "done" }
          : activity;
      return applySessionActivity(state, action.tabId, action.sessionId, guarded, action.observed);
    }
  }
}

/**
 * Marks every completion the given tab owns as seen.
 *
 * Opening a tab is how the user reads its completion, so the green dot must go out — the same way
 * activating a tab already clears its unread flag. The entries are kept so the tab retains its agent
 * brand icon; only the attention signal is consumed.
 */
function acknowledgeTabCompletions(state: WorkspaceState, tabId: string): WorkspaceState {
  const activityBySessionId = state.activityBySessionId ?? {};
  let changed = false;
  const next = { ...activityBySessionId };

  // In a split tab only the focused leaf's session is actually read by the user;
  // background panes keep their unseen completion so their attention frame survives.
  const tabLayout = state.layout.layoutsByTabId?.[tabId];
  const activeLeafId = tabLayout?.activeLeafId;
  const activeSessionId = activeLeafId ? tabLayout.sessionIdsByLeafId[activeLeafId] : null;

  for (const sessionId of getTabSessionIds(state, tabId)) {
    const activity = activityBySessionId[sessionId];
    if (!activity || (activity.state !== "done" && activity.state !== "waiting") || activity.seen) continue;
    if (tabLayout && tabLayout.root.type === "split" && activeSessionId && sessionId !== activeSessionId) {
      continue;
    }
    next[sessionId] = { ...activity, seen: true };
    // Reading the completion is engagement: the completion after fresh post-ack work is
    // user-attended, not an automation turn re-driving itself.
    lastSessionEngagementAtBySession.set(sessionId, Date.now());
    changed = true;
  }

  return changed ? { ...state, activityBySessionId: next } : state;
}

function isTabVisible(state: WorkspaceState, tabId: string): boolean {
  if (state.layout.activeTabId === tabId) return true;
  if (state.layout.tabGroups) {
    for (const group of Object.values(state.layout.tabGroups)) {
      if (group.activeTabId === tabId) return true;
    }
  }
  return false;
}

/**
 * Whether the user is genuinely looking at THIS session right now: its tab is
 * visible and, in a split tab, its leaf is the active one. A background split
 * pane must never be treated as observed just because its tab is frontmost.
 */
function isSessionActivelyObserved(state: WorkspaceState, tabId: string, sessionId: string): boolean {
  if (!isTabVisible(state, tabId)) return false;
  const tabLayout = state.layout.layoutsByTabId?.[tabId];
  if (tabLayout && tabLayout.root.type === "split") {
    const leafId = Object.entries(tabLayout.sessionIdsByLeafId).find(([_, sId]) => sId === sessionId)?.[0];
    return Boolean(leafId && leafId === tabLayout.activeLeafId);
  }
  return true;
}

/**
 * Settles any in-flight work claim of sessions whose backend process is known to be gone.
 *
 * A `working`/`waiting` entry is a claim that something is running right now. Once the PTY that
 * produced it is dead, no further screen or title event can ever arrive to retire it, so leaving
 * the claim in place makes tabs, worktree rows and the sidebar spin forever.
 */
function settleActivityForDeadSessions(state: WorkspaceState, sessionIds: Iterable<string>): WorkspaceState {
  let nextState = state;
  for (const sessionId of sessionIds) {
    const current = nextState.activityBySessionId?.[sessionId];
    if (!current || current.state === "done") continue;
    const tabId = findTabIdForSession(nextState, sessionId);
    if (!tabId) continue;
    nextState = applySessionActivity(nextState, tabId, sessionId, { ...current, state: "done" });
  }
  return nextState;
}

/** The local standby shape for a remote pane whose backend is confirmed gone. */
function remoteSessionGone(session: TerminalSession, failure: TerminalSession["remoteFailure"]): TerminalSession {
  return {
    ...session,
    backendSessionId: createStandbyBackendSessionId(session.id),
    processState: "standby",
    lifecycle: "exited",
    exitCode: null,
    reconnectLifecycle: "idle",
    reconnectError: null,
    reconnectRequestId: null,
    remoteConnectionState: undefined,
    remoteGeneration: null,
    remoteFailure: failure ?? null,
    remoteReplayGap: null,
    lastOutputSequence: null,
    recoveryState: null,
    backendUnavailableReason: REMOTE_SESSION_GONE_REASON,
  };
}

/**
 * Transport fields a session must drop once its PTY is confirmed gone.
 *
 * `remoteConnectionState` describes a live binding that may come back (`disconnected` invites a
 * reattach); once the backend id is cleared there is nothing left to reattach to, so keeping it
 * leaves an exited pane advertising a recoverable outage. `running` likewise cannot outlive the
 * process; hibernated and suspended are deliberate user states and stay as they are.
 */
function clearedTransportState(session: TerminalSession): Partial<TerminalSession> {
  return {
    remoteConnectionState: undefined,
    remoteGeneration: null,
    remoteFailure: null,
    remoteReplayGap: null,
    ...(session.processState === "running" ? { processState: "standby" as const } : {}),
  };
}

/**
 * A session whose backend is gone cannot be producing live agent output.
 *
 * Screen-state reports are decoded from terminal output, which the daemon stream pump can deliver
 * slightly after the exit event. Accepting such a late `working`/`blocked` would light a spinner
 * for a dead PTY that nothing will ever turn off.
 */
function isSessionBackendDead(state: WorkspaceState, sessionId: string): boolean {
  const session = state.sessions[sessionId];
  if (!session) return false;
  return session.backendSessionId === null || session.lifecycle === "exited" || session.lifecycle === "failed";
}

/**
 * Engagement clocks, keyed by frontend session id.
 *
 * `ferryx:session-interacted` (input, paste, pane focus, navigation) records the last moment the
 * user was part of a session's loop; every fresh attention episode records when it began. A
 * `done` that starts without any engagement after the previous episode is an automation turn
 * re-driving itself, so it is stored quietly instead of re-arming attention. Engagement stays
 * module-level to avoid keystroke-rate dispatches; episode clocks belong to reducer state so
 * replaying an action cannot change whether its completion is suppressed.
 */
const lastSessionEngagementAtBySession = new Map<string, number>();

/** Record that the user just engaged with a session (input, paste, pane focus, navigation). */
export function markSessionEngagementForAttention(sessionId: string): void {
  lastSessionEngagementAtBySession.set(sessionId, Date.now());
}

/** Test hook: clears the engagement clocks. */
export function resetAttentionEngagementClocksForTests(): void {
  lastSessionEngagementAtBySession.clear();
}

function applySessionActivity(
  state: WorkspaceState,
  tabId: string,
  sessionId: string,
  activity: TerminalActivity,
  observed = true,
): WorkspaceState {
  const previous = state.activityBySessionId?.[sessionId];
  if (
    previous &&
    previous.state === activity.state &&
    previous.title === activity.title &&
    previous.isAgent === activity.isAgent &&
    previous.agentType === activity.agentType &&
    previous.source === activity.source &&
    previous.agentSource === activity.agentSource &&
    previous.seen === activity.seen &&
    previous.notificationSuppressed === activity.notificationSuppressed &&
    previous.outcome === activity.outcome &&
    previous.kind === activity.kind
  ) {
    return state;
  }

  const isAttentionState = activity.state === "done" || activity.state === "waiting";
  const wasAttentionState = previous?.state === "done" || previous?.state === "waiting";
  // Acknowledge carry-over only applies within the same attention state (e.g. metadata/title
  // refresh while already seen). Transitions between distinct attention states (done -> waiting,
  // waiting -> done) require fresh attention notice.
  const isSameAttentionState = isAttentionState && wasAttentionState && previous?.state === activity.state;
  // An app-initiated auto-resume fabricates a working->idle->done blip when the agent merely
  // lands back at its prompt; that first attention is noise, not a request for the user.
  // The blip lands within seconds of the reconnect, while a genuine first completion needs
  // a user round-trip — so a stale suppression (e.g. the resume raced a bare idle before any
  // real work) must not eat real completions minutes later.
  const suppressionArmedAt = state.attentionSuppressions?.[sessionId];
  const rawSuppressionAgeMs =
    suppressionArmedAt === true
      ? 0
      : typeof suppressionArmedAt === "number"
        ? Date.now() - suppressionArmedAt
        : null;
  // A future-dated arm (clock adjustment) must never extend suppression: treat it as expired.
  const suppressionAgeMs = rawSuppressionAgeMs !== null && rawSuppressionAgeMs < 0 ? Number.POSITIVE_INFINITY : rawSuppressionAgeMs;
  const suppression =
    suppressionAgeMs !== null &&
    suppressionAgeMs <= ATTENTION_SUPPRESSION_WINDOW_MS &&
    isAttentionState;
  const hasSuppressionFlag = suppressionAgeMs !== null;
  // An automation loop re-drives its own turns: a fresh `done` that lands without any user
  // engagement (input, focus, navigation) since the previous attention episode means the user
  // was never part of this round-trip, so it is stored quietly (seen + suppressed) instead of
  // re-arming the pane frame, tab dots and the notification center. `waiting` is never quieted
  // — an agent asking for input is a real request regardless of who drove the run — and the
  // armed resume-blip suppression keeps its own, narrower contract alongside this one.
  const previousAttentionAt = state.attentionEpisodeAtBySession?.[sessionId];
  const lastEngagementAt = lastSessionEngagementAtBySession.get(sessionId);
  const freshAttentionEpisode = isAttentionState && (!wasAttentionState || previous?.state !== activity.state);
  const automationQuietDone =
    freshAttentionEpisode &&
    activity.state === "done" &&
    !suppression &&
    previousAttentionAt !== undefined &&
    (lastEngagementAt === undefined || lastEngagementAt < previousAttentionAt);
  const acknowledged =
    isAttentionState &&
    (activity.seen === true ||
      (isSameAttentionState && previous?.seen === true) ||
      suppression ||
      automationQuietDone ||
      (observed && isSessionActivelyObserved(state, tabId, sessionId)));
  const stored: TerminalActivity = {
    ...activity,
    ...(isAttentionState ? { seen: acknowledged } : {}),
    ...(isAttentionState
      ? {
          notificationSuppressed:
            activity.notificationSuppressed === true ||
            suppression ||
            automationQuietDone ||
            (isSameAttentionState && previous?.notificationSuppressed === true),
        }
      : {}),
  };

  // The flag is consumed by the first attention transition either way: an effective
  // suppression eats the resume blip, an expired one lets the genuine completion through.
  let nextState: WorkspaceState = {
    ...state,
    ...(freshAttentionEpisode && activity.notificationSuppressed !== true
      ? { attentionEpisodeAtBySession: { ...state.attentionEpisodeAtBySession, [sessionId]: Date.now() } }
      : {}),
    activityBySessionId: { ...(state.activityBySessionId ?? {}), [sessionId]: stored },
    ...(hasSuppressionFlag && isAttentionState
      ? {
          attentionSuppressions: Object.fromEntries(
            Object.entries(state.attentionSuppressions ?? {}).filter(([id]) => id !== sessionId),
          ),
        }
      : {}),
  };

  if (freshAttentionEpisode && !stored.notificationSuppressed && (!observed || !isTabVisible(state, tabId))) {
    const worktreePath = sessionWorktreePath(state.sessions[sessionId]) || getTabWorktreePath(state, tabId);
    nextState = {
      ...nextState,
      unreadTabIds: { ...state.unreadTabIds, [tabId]: true },
      unreadWorktreePaths: worktreePath
        ? { ...state.unreadWorktreePaths, [worktreePath]: true }
        : state.unreadWorktreePaths,
    };
  }

  return nextState;
}

function getAllTabs(state: WorkspaceState): WorkspaceTab[] {
  return [...state.layout.tabs, ...Object.values(state.worktreeLayouts ?? {}).flatMap((l) => l.tabs)];
}

function getTabSessionIdsForLayout(layout: LayoutState, tabId: string): Set<string> {
  const tab = layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab || !isTerminalTab(tab)) return new Set<string>();
  const tabLayout = layout.layoutsByTabId?.[tabId];
  const sessionIds = new Set<string>(Object.values(tabLayout?.sessionIdsByLeafId ?? {}));
  sessionIds.add(tab.sessionId);
  sessionIds.delete("");
  return sessionIds;
}

export function getTabSessionIds(state: WorkspaceState, tabId: string): Set<string> {
  const inActive = getTabSessionIdsForLayout(state.layout, tabId);
  if (inActive.size > 0 || state.layout.tabs.some((t) => t.id === tabId)) return inActive;
  for (const layout of Object.values(state.worktreeLayouts ?? {})) {
    const inParked = getTabSessionIdsForLayout(layout, tabId);
    if (inParked.size > 0 || layout.tabs.some((t) => t.id === tabId)) return inParked;
  }
  return new Set<string>();
}

function isSessionReferencedOutsideTab(state: WorkspaceState, sessionId: string, tabId: string): boolean {
  return getAllTabs(state).some((tab) => tab.id !== tabId && getTabSessionIds(state, tab.id).has(sessionId));
}

function isSessionReferenced(state: WorkspaceState, sessionId: string): boolean {
  return getAllTabs(state).some((tab) => getTabSessionIds(state, tab.id).has(sessionId));
}

function getBrowserIdForTab(layout: LayoutState, tabId: string, paneBrowserId?: string): string | undefined {
  const tab = layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab) return undefined;
  if (paneBrowserId === undefined) return tab.kind === "browser" ? tab.browserId : undefined;
  if (tab.kind === "browser" && tab.browserId === paneBrowserId) return paneBrowserId;
  const contents = layout.layoutsByTabId[tabId]?.contentsByLeafId ?? {};
  return Object.values(contents).some(
    (content) => content.kind === "browser" && (content.browser?.browserId ?? content.browserId) === paneBrowserId,
  ) ? paneBrowserId : undefined;
}

function isBrowserIdReferenced(state: WorkspaceState, browserId: string): boolean {
  if (!browserId) return false;
  const layouts = [state.layout, ...Object.values(state.worktreeLayouts ?? {})];
  return layouts.some((layout) => {
    if (layout.tabs.some((tab) => tab.kind === "browser" && tab.browserId === browserId)) return true;
    return Object.values(layout.layoutsByTabId ?? {}).some((tabLayout) =>
      Object.values(tabLayout.contentsByLeafId ?? {}).some(
        (content) =>
          content.kind === "browser" &&
          (content.browser?.browserId ?? content.browserId ?? "") === browserId,
      ),
    );
  });
}

function getDisposableSessionsForTab(state: WorkspaceState, tabId: string): TerminalSession[] {
  const result: TerminalSession[] = [];
  const seenBackends = new Set<string>();
  for (const sessionId of getTabSessionIds(state, tabId)) {
    if (isSessionReferencedOutsideTab(state, sessionId, tabId)) continue;
    const session = state.sessions[sessionId];
    if (!session) continue;
    const key = session.backendSessionId ?? session.id;
    if (seenBackends.has(key)) continue;
    seenBackends.add(key);
    result.push(session);
  }
  return result;
}

function sessionWorktreePath(session: TerminalSession | undefined): string {
  return session?.worktreePath ?? session?.cwd ?? "";
}

function getTabWorktreePath(state: WorkspaceState, tabId: string): string | null {
  for (const sessionId of getTabSessionIds(state, tabId)) {
    const path = sessionWorktreePath(state.sessions[sessionId]);
    if (path) return path;
  }
  for (const tab of getAllTabs(state)) {
    if (tab.id === tabId && tab.kind === "browser") {
      return tab.worktreePath ?? null;
    }
  }
  return null;
}

function findTabIdForSession(state: WorkspaceState, sessionId: string): string | null {
  for (const tab of getAllTabs(state)) {
    if (!isTerminalTab(tab)) continue;
    if (getTabSessionIds(state, tab.id).has(sessionId)) return tab.id;
  }
  return null;
}

/**
 * Resolve the exact tab + leaf that renders a session, requiring membership in the current pane
 * tree root: a stale `sessionIdsByLeafId` entry for a removed leaf, or a primary `tab.sessionId`
 * that the root no longer contains, does NOT match. Only a legacy tab with no pane layout falls
 * back to its primary session id. Returns the owning worktree path (or null for the active
 * layout) alongside the tab/leaf.
 */
function locateSessionLeaf(
  layout: LayoutState,
  sessionId: string,
): { tabId: string; leafId: string | null } | null {
  for (const tab of layout.tabs) {
    if (!isTerminalTab(tab)) continue;
    const tabLayout = layout.layoutsByTabId?.[tab.id];
    if (tabLayout) {
      for (const leafId of collectLeafIds(tabLayout.root)) {
        if (tabLayout.sessionIdsByLeafId[leafId] === sessionId) return { tabId: tab.id, leafId };
      }
      // A tab that has a pane layout but no matching live leaf is not a match, even if its
      // legacy primary sessionId still points at the removed session.
      continue;
    }
    if (tab.sessionId === sessionId) return { tabId: tab.id, leafId: null };
  }
  return null;
}

function locateSessionAcrossLayouts(
  state: WorkspaceState,
  sessionId: string,
): { worktreePath: string | null; tabId: string; leafId: string | null } | null {
  const inActive = locateSessionLeaf(state.layout, sessionId);
  if (inActive) return { worktreePath: null, ...inActive };
  for (const [path, parked] of Object.entries(state.worktreeLayouts ?? {})) {
    const found = locateSessionLeaf(parked, sessionId);
    if (found) return { worktreePath: path, ...found };
  }
  return null;
}

/**
 * Whether a session can actually be navigated to: it exists in the sessions map AND is
 * referenced by a live layout leaf (active or a parked worktree). A session id that lingers
 * only in the map without a root leaf, or a closed session, is not navigable. This mirrors
 * exactly what the FOCUS_EXISTING_SESSION reducer will accept.
 */
export function hasNavigableSession(state: WorkspaceState, sessionId: string): boolean {
  return Boolean(state.sessions[sessionId]) && locateSessionAcrossLayouts(state, sessionId) !== null;
}

function clearWorktreeUnreadWhenRead(
  state: WorkspaceState,
  tabId: string,
  sourceState: WorkspaceState = state,
): WorkspaceState {
  const worktreePath = getTabWorktreePath(sourceState, tabId);
  if (!worktreePath) return state;
  const allTabs = getAllTabs(sourceState);
  const hasOtherUnreadTab = allTabs.some(
    (tab) => tab.id !== tabId && state.unreadTabIds[tab.id] && getTabWorktreePath(sourceState, tab.id) === worktreePath,
  );
  if (hasOtherUnreadTab) return state;
  const unreadWorktreePaths = { ...state.unreadWorktreePaths };
  delete unreadWorktreePaths[worktreePath];
  return { ...state, unreadWorktreePaths };
}

// resolvePairedStartup is imported from ../lib/remoteProject

type LogicalSpawnRequest = {
  workspaceId: string;
  worktree: WorktreeIdentity | null;
  cwd?: string | null;
  shell?: string | null;
  inheritFromSessionId?: string | null;
  cols?: number;
  rows?: number;
};

/** Geometry measurement must never block a spawn: any failure means "unavailable". */
async function measureFor(services: WorkspaceServices, target: InitialGeometryTarget): Promise<InitialPtyGrid | null> {
  if (!services.measureInitialGeometry) return null;
  try {
    return await services.measureInitialGeometry(target);
  } catch {
    return null;
  }
}

async function spawnTerminalForLogicalAction(
  services: WorkspaceServices,
  request: LogicalSpawnRequest,
): Promise<string> {
  const clientRequestId = createClientRequestId();
  const startup = resolvePairedStartup(request.workspaceId);
  const stableRequest = { ...request, clientRequestId, ...(startup ? { startup } : {}) };
  try {
    return await services.spawnTerminal(stableRequest);
  } catch (error) {
    if (!isAmbiguousRendererTransportError(error)) throw error;
    return services.spawnTerminal(stableRequest);
  }
}

async function spawnDetailedForLogicalAction(
  services: WorkspaceServices,
  request: LogicalSpawnRequest,
): Promise<Awaited<ReturnType<NonNullable<WorkspaceServices["spawnTerminalDetailed"]>>>> {
  const clientRequestId = createClientRequestId();
  const startup = resolvePairedStartup(request.workspaceId);
  const stableRequest = { ...request, clientRequestId, ...(startup ? { startup } : {}) };
  const spawnDetailed = services.spawnTerminalDetailed!;
  try {
    return await spawnDetailed(stableRequest);
  } catch (error) {
    if (!isAmbiguousRendererTransportError(error)) throw error;
    return spawnDetailed(stableRequest);
  }
}

function isAmbiguousRendererTransportError(error: unknown): boolean {
  if (!error || typeof error !== "object") return true;
  const code = (error as { code?: unknown }).code;
  return code === undefined || code === "UNKNOWN";
}

async function closeBackendSession(session: TerminalSession | undefined, services: WorkspaceServices) {
  if (session && localSplitIntent(session)) {
    await cancelLocalSplit(session.id);
    return;
  }
  if (!session?.backendSessionId) return;
  try {
    await services.closeTerminal(session.backendSessionId);
  } finally {
    terminalEventBus.clearSession(session.backendSessionId);
  }
}

/** Closes a PTY that never became bound to a pane, clearing its buffered events. */
async function closeOrphanBackend(backendSessionId: string, services: WorkspaceServices) {
  try {
    await services.closeTerminal(backendSessionId);
  } catch (error) {
    console.warn("Failed to close unowned terminal session:", backendSessionId, error);
  } finally {
    terminalEventBus.clearSession(backendSessionId);
  }
}

function isHealthyLocalPty(session: TerminalSession): boolean {
  if (!session.backendSessionId) return false;
  if (session.lifecycle !== "working" && session.lifecycle !== "waiting" && session.lifecycle !== "running") return false;
  if (session.remoteConnectionState != null && session.remoteConnectionState !== "connected") return false;
  return session.reconnectLifecycle == null || session.reconnectLifecycle === "idle";
}

function warnRejectedCloses(results: PromiseSettledResult<unknown>[], sessions: TerminalSession[]) {
  results.forEach((result, index) => {
    if (result.status === "rejected") {
      console.warn("Failed to close terminal session:", sessions[index]?.backendSessionId ?? sessions[index]?.id, result.reason);
    }
  });
}

async function closeBackendSessionAndWait(session: TerminalSession | undefined, services: WorkspaceServices) {
  if (session && localSplitIntent(session)) {
    await cancelLocalSplit(session.id);
    return;
  }
  if (!session?.backendSessionId) return;
  const backendSessionId = session.backendSessionId;
  const exitPromise = services.waitForTerminalExit(backendSessionId, LAST_TAB_EXIT_TIMEOUT_MS);
  try {
    await services.closeTerminal(backendSessionId);
    await exitPromise;
  } finally {
    terminalEventBus.clearSession(backendSessionId);
  }
}

function getActiveWorktree(state: WorkspaceState) {
  return state.worktrees.find((worktree) => worktree.path === state.activeWorktreePath) ?? state.worktrees[0] ?? null;
}

/**
 * Automatic fallback label for a new terminal tab: the worktree's branch. The visible text is
 * resolved live from the terminal title (see state/tabDisplay.ts), which also disambiguates
 * identical names, so no ordinal is baked into the stored label.
 */
function nextTabLabel(worktree: Worktree) {
  const branch = worktree.branch?.replace(/^refs\/heads\//, "") ?? "";
  const parts = branch.split("/");
  return isManagedBranchPrefix(parts[0]) && parts.length > 2
    ? parts.slice(2).join("/")
    : branch && branch !== "orca-lite"
      ? branch
      : "main";
}

function mapBackendLifecycle(payload: TerminalLifecyclePayload): TerminalLifecycle {
  if (payload.state === "started") return "working";
  if (payload.state === "failed") return "failed";
  return "exited";
}

function createClientRequestId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function collectPreviewIds(state: { layout: LayoutState; worktreeLayouts?: Record<string, LayoutState> }): Set<string> {
  const ids = new Set<string>();
  const layouts = [state.layout, ...Object.values(state.worktreeLayouts ?? {})];
  for (const layout of layouts) {
    for (const tab of layout.tabs) {
      if (tab.kind === "file") ids.add(tab.previewId);
    }
    for (const pane of Object.values(layout.layoutsByTabId ?? {})) {
      for (const content of Object.values(pane.contentsByLeafId ?? {})) {
        if (content.kind === "file") ids.add(content.previewId);
      }
    }
  }
  return ids;
}

/** Releases controllers no layout references. The registry is process-wide while only one
 * workspace is mounted, so previews parked in other workspaces' snapshots stay live. */
function releaseOrphanPreviews(state: WorkspaceState) {
  const live = collectPreviewIds(state);
  for (const [snapshotWorkspaceId, snapshot] of listWorkspaceSnapshots()) {
    if (snapshotWorkspaceId === state.workspaceId) continue;
    for (const id of collectPreviewIds(snapshot)) live.add(id);
  }
  for (const id of listFilePreviewIds()) {
    if (!live.has(id)) void releaseFilePreview(id);
  }
}

function createId(prefix: string) {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}:${randomPart}`;
}
