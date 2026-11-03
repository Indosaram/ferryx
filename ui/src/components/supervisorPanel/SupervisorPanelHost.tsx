import { useEffect, useState } from 'react';

import type { PanelEvent, PanelSnapshot, SessionRef } from '../../lib/supervisorPanel/contract';
import { emptyPanelSnapshot } from '../../lib/supervisorPanel/emptySnapshot';
import { getSupervisorPanel, onSupervisorPanelEvent } from '../../lib/tauri';
import { applyPanelEvent } from './applyPanelEvent';
import { SupervisorPanelSections } from './SupervisorPanelSections';

export type SupervisorPanelHostProps = {
  onFocusSession: (sessionRef: SessionRef) => void;
  /** Test seams; the defaults talk to the Tauri backend. */
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
};

/**
 * Left-sidebar Supervisor: loads the panel snapshot, then folds in panel.* events. Until the
 * backend answers (older build, no hub) it shows the setup state instead of failing.
 */
export function SupervisorPanelHost({
  onFocusSession,
  load = getSupervisorPanel,
  subscribe = onSupervisorPanelEvent,
}: SupervisorPanelHostProps) {
  const [snapshot, setSnapshot] = useState<PanelSnapshot | null>(null);

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

  return (
    <div data-testid="supervisor-panel" className="max-h-[45%] min-h-0 shrink-0 overflow-y-auto px-2 pb-2 text-xs scrollbar-sleek">
      {snapshot ? (
        <SupervisorPanelSections snapshot={snapshot} onFocusSession={onFocusSession} />
      ) : (
        <p>Loading supervisor…</p>
      )}
    </div>
  );
}
