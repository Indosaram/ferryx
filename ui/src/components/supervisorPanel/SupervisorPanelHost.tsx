import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { PanelActions } from '../../lib/supervisorPanel/actions';
import type { PanelEvent, PanelSnapshot, SessionRef } from '../../lib/supervisorPanel/contract';
import { desktopPanelActions } from '../../lib/supervisorPanel/desktopActions';
import { emptyPanelSnapshot } from '../../lib/supervisorPanel/emptySnapshot';
import { applyPanelEvent } from '../../lib/supervisorPanel/model';
import { legacyGoalsBySession } from '../../lib/supervisorPanel/legacyGoals';
import { loadDesktopTranscriptPage, type TranscriptPageLoader } from '../../lib/supervisorPanel/transcript';
import { useTranscript } from '../../lib/supervisorPanel/useTranscript';
import { useSupervisor } from '../../lib/supervisor';
import { getSupervisorPanel, onSupervisorPanelEvent } from '../../lib/tauri';
import { PanelSurface } from './PanelSurface';

export type SupervisorPanelHostProps = {
  onFocusSession: (sessionRef: SessionRef) => void;
  /** Test seams; the defaults talk to the Tauri backend. */
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
  /** Replaces the Tauri-backed writes (tests). `onChanged` is the host's reload. */
  actionsFor?: (input: { openSession: (sessionRef: SessionRef) => void; onChanged: () => void }) => PanelActions;
  now?: () => number;
  loadTranscript?: TranscriptPageLoader;
};

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
}: SupervisorPanelHostProps) {
  const [snapshot, setSnapshot] = useState<PanelSnapshot | null>(null);
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
  }, []);

  const actions = useMemo(
    () => actionsFor({ openSession: onFocusSession, onChanged: reload }),
    [actionsFor, onFocusSession, reload],
  );

  // Legacy lanes are a property of their session: the hub snapshot is read only to find which
  // sessions carry a goal and whether it is paused.
  const { snapshot: hub } = useSupervisor();
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
      {snapshot ? (
        <PanelSurface snapshot={snapshot} actions={actions} nowMs={tick} goals={goals} computerLabel="this computer" transcript={transcript} />
      ) : (
        <p className="text-muted-foreground">Loading supervisor…</p>
      )}
    </div>
  );
}
