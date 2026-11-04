// How many decisions the Supervisor panel holds for the owner, read from the same snapshot and
// `panel.*` events the panel renders, so a surface outside the panel (the collapsed sidebar's
// badge) counts exactly the cards the panel shows.
import { useEffect, useState } from "react";

import { getSupervisorPanel, onSupervisorPanelEvent } from "../tauri";
import type { PanelEvent, PanelSnapshot } from "./contract";
import { applyPanelEvent, validateEscalation } from "./model";

export type PanelDecisionSource = {
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
};

/**
 * Decision cards the panel would render for this snapshot: one per session, invalid escalations
 * are not cards (the same rule as `buildPanelView`), and the setup state shows none.
 */
export function panelDecisionCount(snapshot: PanelSnapshot): number {
  if (snapshot.setupRequired || !Array.isArray(snapshot.decisions)) return 0;
  const refs = new Set<string>();
  for (const decision of snapshot.decisions) {
    if (validateEscalation(decision) === null) refs.add(decision.sessionRef);
  }
  return refs.size;
}

export function usePanelDecisionCount({ load = getSupervisorPanel, subscribe = onSupervisorPanelEvent }: PanelDecisionSource = {}): number {
  const [snapshot, setSnapshot] = useState<PanelSnapshot | null>(null);

  useEffect(() => {
    let disposed = false;
    let loaded = false;
    const early: PanelEvent[] = [];
    let unlisten: (() => void) | undefined;

    // Subscribe before the first load so an event racing it is replayed, not lost. A build
    // without the panel commands rejects; the count then stays 0.
    subscribe((event) => {
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
      .then((initial) => {
        if (disposed) return;
        loaded = true;
        setSnapshot(early.reduce(applyPanelEvent, initial));
        early.length = 0;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load, subscribe]);

  return snapshot ? panelDecisionCount(snapshot) : 0;
}
