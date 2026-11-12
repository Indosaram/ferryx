import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { PanelActions } from '../../lib/supervisorPanel/actions';
import type { PanelEvent, PanelSnapshot, SessionRef } from '../../lib/supervisorPanel/contract';
import { desktopPanelActions } from '../../lib/supervisorPanel/desktopActions';
import { emptyPanelSnapshot } from '../../lib/supervisorPanel/emptySnapshot';
import { applyPanelEvent } from '../../lib/supervisorPanel/model';
import { legacyGoalsBySession } from '../../lib/supervisorPanel/legacyGoals';
import { loadDesktopTranscriptPage, type TranscriptPageLoader } from '../../lib/supervisorPanel/transcript';
import { useTranscript } from '../../lib/supervisorPanel/useTranscript';
import { adoptArchivedGoals, desktopUnifiedClient, getLastUnifiedSupervisor, moveSupervisorHere, useSupervisor } from '../../lib/supervisor';
import { SupervisorCallButton } from '../supervisor/SupervisorCallButton';
import { isStandbyRole } from '../../lib/supervisorModel';
import type { SupervisorCallResult, SupervisorMoveResult } from '../../lib/unifiedSupervisor';
import { getSupervisorPanel, getSupervisorPrs, listWorktrees, onSupervisorPanelEvent } from '../../lib/tauri';
import type { PrLane } from '../../lib/supervisorPanel/contract';
import type { WorktreeOnBranch } from '../../lib/supervisorPanel/prLane';
import { LeaseNotices, StandbyCard, type LeaseAdoptResult } from './LeaseStatus';
import { PanelSurface } from './PanelSurface';
import { desktopVoicePort } from '../../lib/voiceCallPort';
import { useVoiceConversation } from '../../lib/useVoiceConversation';
import { desktopProjectMemoryActions } from '../../lib/supervisorSetup';
import type { ProjectMemoryActions } from '../../lib/projectMemory';
import { ProjectMemorySwitches } from '../supervisor/ProjectMemorySwitches';

export type SupervisorPanelHostProps = {
  onFocusSession: (sessionRef: SessionRef) => void;
  /** Test seams; the defaults talk to the Tauri backend. */
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
  /** Replaces the Tauri-backed writes (tests). `onChanged` is the host's reload. */
  actionsFor?: (input: { openSession: (sessionRef: SessionRef) => void; onChanged: () => void }) => PanelActions;
  now?: () => number;
  loadTranscript?: TranscriptPageLoader;
  /** The supervisor lease writes; the defaults are the daemon's move and adopt commands. */
  moveHere?: (force: boolean) => Promise<SupervisorMoveResult>;
  adoptArchived?: (goalIds: readonly string[]) => Promise<SupervisorCallResult>;
  /** The per-project "give my project decisions to agents" switches (setup surface). */
  projectMemory?: ProjectMemoryActions;
  /** The hub's PR lane; the default reads it from this daemon. */
  loadPrs?: () => Promise<PrLane>;
  /** The worktrees a hand-off may use; the default lists the active project's. */
  loadWorktrees?: () => Promise<readonly WorktreeOnBranch[]>;
  /** The project whose worktrees a hand-off may use; absent, no worktree is listed. */
  workspaceId?: string | null;
};

async function adoptResult(call: Promise<SupervisorCallResult>): Promise<LeaseAdoptResult> {
  const result = await call;
  return result.kind === 'ok' ? { kind: 'ok' } : { kind: 'error', message: result.message };
}

/**
 * The left sidebar's Supervisor: loads the panel snapshot, folds in panel.* events through the
 * shared reducer and composes the shared PanelSurface. Until the backend answers (older build,
 * no hub) it shows the setup state instead of failing; writes the build lacks report failure.
 */
export function SupervisorPanelHost({
  onFocusSession,
  load = getSupervisorPanel,
  subscribe = onSupervisorPanelEvent,
  actionsFor = desktopPanelActions,
  now = Date.now,
  loadTranscript = loadDesktopTranscriptPage,
  moveHere = moveSupervisorHere,
  adoptArchived = adoptArchivedGoals,
  projectMemory = desktopProjectMemoryActions,
  loadPrs = getSupervisorPrs,
  loadWorktrees,
  workspaceId = null,
}: SupervisorPanelHostProps) {
  const [snapshot, setSnapshot] = useState<PanelSnapshot | null>(null);
  const [prLane, setPrLane] = useState<PrLane | null>(null);
  const [worktrees, setWorktrees] = useState<readonly WorktreeOnBranch[]>([]);
  const [tick, setTick] = useState(() => now());
  const loadRef = useRef(load);
  loadRef.current = load;
  const reloadSeq = useRef(0);

  useEffect(() => {
    let disposed = false;
    let loaded = false;
    const early: PanelEvent[] = [];
    let unlisten: (() => void) | undefined;

    // Subscribe before the first load so an event racing it is replayed, not lost.
    void subscribe((event) => {
      if (disposed) return;
      // The PR lane is not part of the snapshot: its own frame replaces it whole.
      if (event.type === 'panel.prs') {
        setPrLane(event.lane);
        return;
      }
      if (!loaded) early.push(event);
      else setSnapshot((current) => (current ? applyPanelEvent(current, event) : current));
    })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => undefined);

    load()
      .catch(() => emptyPanelSnapshot())
      .then((initial) => {
        if (disposed) return;
        loaded = true;
        setSnapshot(early.reduce(applyPanelEvent, initial));
        early.length = 0;
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load, subscribe]);

  // Ages ("waiting 23 min") move without an event; one slow tick keeps them honest.
  useEffect(() => {
    const timer = window.setInterval(() => setTick(now()), 30_000);
    return () => window.clearInterval(timer);
  }, [now]);

  // The PR lane is its own read and its own frame (`panel.prs`); a build without the route keeps
  // the section hidden instead of showing an empty lane that claims nothing waits on the owner.
  const loadPrsRef = useRef(loadPrs);
  loadPrsRef.current = loadPrs;
  const refreshPrs = useCallback(() => {
    loadPrsRef.current().then(
      (lane) => setPrLane(lane),
      () => setPrLane(null),
    );
  }, []);
  useEffect(() => {
    refreshPrs();
  }, [refreshPrs]);

  // A hand-off needs the checkout on the PR's head branch; it is read when the lane arrives and
  // after a hand-off, never created here.
  const loadWorktreesRef = useRef(loadWorktrees);
  loadWorktreesRef.current = loadWorktrees;
  useEffect(() => {
    const read = loadWorktreesRef.current ?? (workspaceId ? () => listWorktrees(workspaceId) : null);
    if (!read) return;
    let disposed = false;
    read().then(
      (list) => {
        if (!disposed) setWorktrees(list);
      },
      () => undefined,
    );
    return () => {
      disposed = true;
    };
  }, [prLane, workspaceId]);

  // After an accepted write the hub is read again; only the newest read lands, and a failed
  // read keeps what is on screen.
  const reload = useCallback(() => {
    const seq = (reloadSeq.current += 1);
    loadRef.current().then(
      (next) => {
        if (reloadSeq.current === seq) setSnapshot(next);
      },
      () => undefined,
    );
    refreshPrs();
  }, [refreshPrs]);

  const actions = useMemo(
    () => actionsFor({ openSession: onFocusSession, onChanged: reload }),
    [actionsFor, onFocusSession, reload],
  );
  const [voicePort] = useState(() => desktopVoicePort());
  const voiceConversation = useVoiceConversation({
    port: voicePort,
    send: (text) =>
      actions.converse(text, []).then((result) => (result.ok && result.reply ? result.reply.text : "")),
  });

  // Legacy lanes are a property of their session: the hub snapshot is read only to find which
  // sessions carry a goal and whether it is paused.
  const { snapshot: hub } = useSupervisor();
  // The call reads the supervisor through the daemon the same way the panel does; before the
  // first unified view arrives there is nothing to talk to, so no button is offered.
  const lastUnified = getLastUnifiedSupervisor();
  const goals = useMemo(
    () => legacyGoalsBySession(snapshot?.sessions.map((session) => session.sessionRef) ?? [], hub.lanes),
    [snapshot, hub.lanes],
  );

  // The stored conversation is read again whenever the panel or the hub snapshot changed: a sent
  // prompt, its Reply and a worker report all end in one of those.
  const transcriptVersion = useMemo(() => ({ snapshot, hub }), [snapshot, hub]);
  const transcript = useTranscript(loadTranscript, transcriptVersion);

  return (
    <div
      data-testid="supervisor-panel"
      className="min-h-0 flex-1 overflow-y-auto px-3 pb-3 pt-1 text-xs scrollbar-sleek"
    >
      {isStandbyRole(hub.hub.role) ? (
        // Another computer holds the account supervisor lease: this one never offers to turn a
        // second supervisor on, only to move the one there is.
        <StandbyCard
          hub={hub.hub}
          here="This computer"
          moveHere={moveHere}
          adoptArchived={(goalIds) => adoptResult(adoptArchived(goalIds))}
        />
      ) : snapshot ? (
        <div className="flex flex-col gap-3">
          <LeaseNotices hub={hub.hub} here="This computer" />
          {lastUnified ? (
            <div data-testid="supervisor-call" className="flex min-h-9 items-center gap-2">
              <span className="text-muted-foreground">Talk to the supervisor</span>
              <SupervisorCallButton
                view={lastUnified}
                client={desktopUnifiedClient}
                projectId={lastUnified.projects[0]?.projectId ?? null}
              />
            </div>
          ) : null}
          <PanelSurface snapshot={snapshot} actions={actions} nowMs={tick} goals={goals} computerLabel="this computer" transcript={transcript} voiceConversation={voiceConversation} />
          <ProjectMemorySwitches actions={projectMemory} />
          <PanelSurface snapshot={snapshot} actions={actions} nowMs={tick} goals={goals} computerLabel="this computer" transcript={transcript} voiceConversation={voiceConversation} prLane={prLane} worktrees={worktrees} />
          <ProjectMemorySwitches actions={projectMemory} />
        </div>
      ) : (
        <p className="text-muted-foreground">Loading supervisor…</p>
      )}
    </div>
  );
}
