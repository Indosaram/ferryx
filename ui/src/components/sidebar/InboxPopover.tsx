import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

import { floatingSurface } from "../../lib/surface";
import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { PanelEvent, PanelSnapshot } from "../../lib/supervisorPanel/contract";
import { desktopPanelActions } from "../../lib/supervisorPanel/desktopActions";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import { applyPanelEvent, buildPanelView } from "../../lib/supervisorPanel/model";
import { getSupervisorPanel, onSupervisorPanelEvent } from "../../lib/tauri";
import { IconButton } from "../ui/IconButton";
import { useModalDialog } from "../useModalDialog";
import { LiveDecision } from "../supervisorPanel/LiveDecision";

export type InboxPopoverProps = {
  onClose: () => void;
  onFocusSession: (sessionRef: string) => void;
  load?: () => Promise<PanelSnapshot>;
  subscribe?: (handler: (event: PanelEvent) => void) => Promise<() => void>;
  actionsFor?: (input: { openSession: (sessionRef: string) => void; onChanged: () => void }) => PanelActions;
  now?: () => number;
};

/**
 * The Inbox: the decisions the Supervisor panel is holding for the owner, as answerable cards in a
 * popup over the window. It reads the same snapshot and `panel.*` events the panel does, so the
 * titlebar count is exactly the number of cards here and an answer is the same guarded answer.
 */
export function InboxPopover({
  onClose,
  onFocusSession,
  load = getSupervisorPanel,
  subscribe = onSupervisorPanelEvent,
  actionsFor = desktopPanelActions,
  now = Date.now,
}: InboxPopoverProps) {
  const [snapshot, setSnapshot] = useState<PanelSnapshot | null>(null);
  const [host, setHost] = useState<HTMLElement | null>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  const [tick, setTick] = useState(() => now());
  const loadRef = useRef(load);
  loadRef.current = load;
  const reloadSeq = useRef(0);

  const { dialogRef } = useModalDialog({ onEscape: onClose });

  // Portalled to <body> so the popup sits above the sidebar's scroller; role="dialog" is what
  // makes a native terminal surface yield to it.
  useLayoutEffect(() => {
    const element = document.createElement("div");
    element.setAttribute("data-testid", "inbox-popover-host");
    document.body.appendChild(element);
    setHost(element);
    return () => {
      element.remove();
    };
  }, []);

  // The sidebar clips its own children, so the popup cannot be a child of the button. It opens
  // beside the button it belongs to instead of at the window's right edge, and flips to the
  // button's left when the window has no room on the right.
  useLayoutEffect(() => {
    const trigger = document.querySelector<HTMLElement>('[data-testid="inbox-open-button"]');
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const margin = 8;
    const width = Math.min(380, window.innerWidth - margin * 2);
    const right = rect.right + margin;
    const left = right + width <= window.innerWidth - margin ? right : Math.max(margin, rect.left - width - margin);
    setPosition({ left, top: Math.min(rect.bottom + 6, Math.max(margin, window.innerHeight - 120)) });
  }, []);

  useEffect(() => {
    let disposed = false;
    let loaded = false;
    const early: PanelEvent[] = [];
    let unlisten: (() => void) | undefined;

    // Subscribe before the first load so an event racing it is replayed, not lost.
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

  // After an accepted write the hub is read again; only the newest read lands.
  const reload = () => {
    const seq = (reloadSeq.current += 1);
    loadRef.current().then(
      (next) => {
        if (reloadSeq.current === seq) setSnapshot(next);
      },
      () => undefined,
    );
  };

  const actions = useMemo(() => actionsFor({ openSession: onFocusSession, onChanged: reload }), [actionsFor, onFocusSession]);

  const view = useMemo(() => (snapshot ? buildPanelView(snapshot, { nowMs: tick }) : null), [snapshot, tick]);

  if (!host) return null;
  return createPortal(
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-label="Inbox"
      data-testid="inbox-popover"
      style={position ? { left: position.left, top: position.top } : undefined}
      className={floatingSurface(
        "fixed z-50 flex max-h-[70vh] w-[380px] max-w-[calc(100vw-1rem)] flex-col rounded-xl p-0 focus-visible:outline-none",
      )}
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <h2 className="text-[13px] font-semibold text-foreground">Inbox</h2>
        <span className="text-xs text-muted-foreground tabular-nums">
          {view ? (view.decisions.length > 0 ? `${view.decisions.length} waiting` : "Nothing waiting") : ""}
        </span>
        <span className="flex-1" />
        <IconButton label="Close inbox" size="sm" onClick={onClose}>
          <X className="size-3.5" />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3 text-xs scrollbar-sleek">
        {!view ? (
          <p className="text-muted-foreground">Loading inbox…</p>
        ) : view.setupRequired ? (
          <p className="text-muted-foreground">Turn the supervisor on to collect decisions here.</p>
        ) : view.decisions.length === 0 ? (
          <p data-testid="inbox-empty" className="text-muted-foreground">
            Nothing is waiting on you.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {view.decisions.map((decision) => (
              <LiveDecision key={decision.sessionRef} decision={decision} actions={actions} />
            ))}
          </div>
        )}
      </div>
      <div className="shrink-0 border-t border-border px-3 py-2 text-[10px] text-muted-foreground">
        The Supervisor panel in the sidebar has the rest.
      </div>
    </div>,
    host,
  );
}
