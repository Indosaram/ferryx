import { useMemo, useSyncExternalStore } from "react";
import { X } from "lucide-react";

import { cn } from "../../lib/cn";
import {
  remoteDesktopStore,
  type RemoteDesktopSession,
  type RemoteDesktopState,
} from "../../state/remoteDesktopStore";
import { RemoteDesktopPane } from "../RemoteDesktopPane";

/** A session the canvas can draw: dialing or live. A failed attempt stays in the drawer. */
function isOpenSession(session: RemoteDesktopSession): boolean {
  return session.status === "connecting" || session.status === "connected";
}

/**
 * Sessions that must be kept mounted: the drawer's tabs, in connect order. Never derived from
 * `remoteDesktopStore.disconnect` - only an explicit user action tears a session down.
 */
export function openRemoteDesktopSessions(state: RemoteDesktopState): RemoteDesktopSession[] {
  return Object.values(state.sessions).filter(isOpenSession);
}

/** The Activity Bar's Remote Desktop badge: machines currently viewing or being dialed. */
export function liveRemoteDesktopSessionCount(state: RemoteDesktopState): number {
  return openRemoteDesktopSessions(state).length;
}

export type GlobalRemoteDesktopCanvasProps = {
  /** False while another Activity Bar mode owns the main area; the canvas hides, never unmounts. */
  active: boolean;
  activeHostId: string | null;
  onSelectHost: (hostId: string) => void;
};

/**
 * The Remote Desktop main area, mounted exactly once by `App`: one tab per open host and one
 * `RemoteDesktopPane` per tab. Switching tabs or Activity Bar modes only toggles `visible`, so the
 * native session behind a hidden pane survives every switch; only the tab's close button (or the
 * pane's own Disconnect) calls `remoteDesktopStore.disconnect`.
 */
export function GlobalRemoteDesktopCanvas({ active, activeHostId, onSelectHost }: GlobalRemoteDesktopCanvasProps) {
  const state = useSyncExternalStore(remoteDesktopStore.subscribe, remoteDesktopStore.getState, remoteDesktopStore.getState);
  const sessions = useMemo(() => openRemoteDesktopSessions(state), [state]);
  const selectedId = sessions.some((session) => session.hostId === activeHostId)
    ? activeHostId
    : sessions[0]?.hostId ?? null;

  return (
    <section
      data-testid="activity-canvas-remote-desktop"
      data-active={active ? "true" : "false"}
      aria-label="Remote desktop"
      hidden={!active}
      className={cn(
        "h-full min-w-0 flex-1 flex-col overflow-hidden bg-background",
        active ? "flex" : "hidden",
      )}
    >
      {sessions.length > 0 ? (
        <div
          role="tablist"
          aria-label="Remote desktop hosts"
          className="flex h-titlebar shrink-0 items-center gap-1 overflow-x-auto border-b border-border px-2"
        >
          {sessions.map((session) => {
            const selected = session.hostId === selectedId;
            const name = session.label || session.hostId;
            return (
              <span
                key={session.hostId}
                className={cn(
                  "flex shrink-0 items-center rounded-md border",
                  selected ? "border-border bg-accent" : "border-transparent",
                )}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  data-testid={`rd-tab-${session.hostId}`}
                  data-rd-tab-status={session.status}
                  onClick={() => onSelectHost(session.hostId)}
                  className={cn(
                    "max-w-[180px] truncate rounded-l-md py-1 pl-2 pr-1 text-[11px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                    selected ? "text-foreground" : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {name}
                </button>
                <button
                  type="button"
                  aria-label={`Disconnect ${name}`}
                  data-testid={`rd-tab-close-${session.hostId}`}
                  onClick={() => void remoteDesktopStore.disconnect(session.hostId)}
                  className="mr-1 inline-flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <X aria-hidden="true" className="size-3" />
                </button>
              </span>
            );
          })}
        </div>
      ) : null}
      <div className="relative min-h-0 flex-1">
        {sessions.length === 0 ? (
          <div
            data-testid="remote-desktop-empty"
            className="flex h-full flex-col items-center justify-center gap-1 p-6 text-center text-[12px] text-muted-foreground"
          >
            <p className="text-[13px] font-medium text-foreground">No machine is connected</p>
            <p>Choose a paired machine in the Remote Desktop drawer to start a session.</p>
          </div>
        ) : (
          sessions.map((session) => {
            const selected = session.hostId === selectedId;
            return (
              <div
                key={session.hostId}
                data-testid={`rd-view-${session.hostId}`}
                hidden={!selected}
                className={cn("absolute inset-0", selected ? "block" : "hidden")}
              >
                <RemoteDesktopPane
                  hostId={session.hostId}
                  label={session.label || session.hostId}
                  visible={selected && active}
                />
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
