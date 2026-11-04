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
    let current: PanelSnapshot | null = null;
    let reloadSeq = 0;
    const early: PanelEvent[] = [];
    let unlisten: (() => void) | undefined;
    const show = (next: PanelSnapshot | null) => {
      current = next;
      setSnapshot(next);
    };
    // The panel host reads the hub again after its own writes (Turn on among them); the badge
    // has no such hook, so a panel event that arrives while it holds no usable snapshot (the
    // first load failed, or setup was still required) reads the truth again. Only the newest
    // read lands; a failed read keeps what there is.
    const reload = () => {
      const seq = (reloadSeq += 1);
      load().then(
        (next) => {
          if (!disposed && seq === reloadSeq) show(next);
        },
        () => undefined,
      );
    };

    // Subscribe before the first load so an event racing it is replayed, not lost. A build
    // without the panel commands rejects; the count then stays 0.
    subscribe((event) => {
      if (disposed) return;
      if (!loaded) early.push(event);
      else if (!current || current.setupRequired) reload();
      else show(applyPanelEvent(current, event));
    })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => undefined);

    load().then(
      (initial) => {
        if (disposed) return;
        loaded = true;
        show(early.reduce(applyPanelEvent, initial));
        early.length = 0;
      },
      () => {
        if (disposed) return;
        loaded = true;
        // An event that raced a failed load is a reason to read again.
        if (early.length > 0) reload();
        early.length = 0;
      },
    );

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load, subscribe]);

  return snapshot ? panelDecisionCount(snapshot) : 0;
}
