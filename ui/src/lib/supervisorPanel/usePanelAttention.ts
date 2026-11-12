// The daemon's attention group per backend session, read from the same supervisor panel snapshot
// and `panel.session` frames the panel renders, so the sidebar, tabs and panel draw one judgment.
import { useEffect, useState } from "react";

import { attentionBySessionIdOf, type AttentionBySessionId } from "../attentionGroup";
import * as tauri from "../tauri";
import type { PanelEvent, PanelSnapshot } from "./contract";
import { applyPanelEvent } from "./model";

export type PanelAttentionSource = {
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
};

const EMPTY: AttentionBySessionId = {};

// Resolved lazily: test doubles of "../tauri" without the panel commands keep working (the map
// stays empty and every pane uses the old-peer mapping in groupOf).
function panelFn<K extends "getSupervisorPanel" | "onSupervisorPanelEvent">(name: K): (typeof tauri)[K] | undefined {
  try {
    return (tauri as Partial<typeof tauri>)[name];
  } catch {
    return undefined;
  }
}

export function usePanelAttention(source: PanelAttentionSource = {}): AttentionBySessionId {
  const [attention, setAttention] = useState<AttentionBySessionId>(EMPTY);
  const load = source.load ?? panelFn("getSupervisorPanel");
  const subscribe = source.subscribe ?? panelFn("onSupervisorPanelEvent");

  useEffect(() => {
    if (!load) return;
    let disposed = false;
    let current: PanelSnapshot | null = null;
    const early: PanelEvent[] = [];
    let unlisten: (() => void) | undefined;
    const show = (next: PanelSnapshot) => {
      current = next;
      setAttention(attentionBySessionIdOf(next.sessions ?? []));
    };
    // Subscribe before the first load so a frame racing it is replayed, not lost.
    subscribe?.((event) => {
      if (disposed || event.type !== "panel.session") return;
      if (current) show(applyPanelEvent(current, event));
      else early.push(event);
    })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => undefined);
    Promise.resolve()
      .then(load)
      .then(
        (initial) => {
          if (disposed || !initial) return;
          show(early.reduce(applyPanelEvent, initial));
          early.length = 0;
        },
        () => undefined,
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load, subscribe]);

  return attention;
}
