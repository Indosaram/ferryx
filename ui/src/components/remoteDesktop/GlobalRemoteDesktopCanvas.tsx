import { useMemo, useState, useSyncExternalStore } from "react";
import { Monitor, ScreenShare, Search, X } from "lucide-react";

import { cn } from "../../lib/cn";
import {
  remoteDesktopStore,
  type RemoteDesktopSession,
  type RemoteDesktopState,
} from "../../state/remoteDesktopStore";
import { RemoteDesktopPane } from "../RemoteDesktopPane";
import { remoteHostStore, selectHostList } from "../../state/remoteHostStore";
import { PairedMachineRow } from "./RemoteDesktopActivitySidebar";
import { RemoteDesktopHostSection } from "../settings/remote/RemoteDesktopHostSection";
import { Input } from "../ui/input";
import { Button } from "../ui/button";

function ComputerLibrary({ onSelectHost }: Pick<GlobalRemoteDesktopCanvasProps, "onSelectHost">) {
  const inventory = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState, remoteHostStore.getState);
  const [query, setQuery] = useState("");
  const [availableOnly, setAvailableOnly] = useState(false);
  const hosts = selectHostList(inventory);
  const filtered = hosts.filter((host) => (!availableOnly || host.online) &&
    `${host.displayName ?? host.name} ${host.address}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return (
    <div data-testid="remote-desktop-library" className="h-full overflow-y-auto p-6">
      <div className="mx-auto flex max-w-5xl flex-col gap-6">
        <header>
          <h1 className="text-xl font-semibold text-foreground">Computers</h1>
          <p className="mt-1 text-[13px] text-muted-foreground">Connect to a paired computer or share this computer with your devices.</p>
        </header>
        <section aria-label="This Computer (Host)" className="rounded-xl border border-border bg-card p-4">
          <h2 className="mb-3 flex items-center gap-2 text-[15px] font-semibold text-foreground"><ScreenShare aria-hidden="true" className="size-5" />This Computer (Host)</h2>
          <RemoteDesktopHostSection />
        </section>
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-0 max-w-md flex-1">
            <Search aria-hidden="true" className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input type="search" aria-label="Search computers" placeholder="Search computers" value={query} onChange={(event) => setQuery(event.target.value)} className="pl-9" />
          </div>
          <Button type="button" variant={availableOnly ? "secondary" : "ghost"} size="sm" aria-pressed={availableOnly} onClick={() => setAvailableOnly((value) => !value)}>Available only</Button>
        </div>
        <h2 className="text-[13px] font-medium text-muted-foreground">Listed computers <span className="ml-1 rounded-full bg-muted px-2 py-0.5">{filtered.length}</span></h2>
        {filtered.length === 0 ? (
          <div data-testid="remote-desktop-empty" role="status" className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-border px-6 py-12 text-center text-[13px] text-muted-foreground">
            <Monitor aria-hidden="true" className="size-8" />
            <p>{hosts.length === 0 ? "No paired computers yet. Pair a machine from Settings > Remote to get started." : "No computers match these filters."}</p>
          </div>
        ) : [true, false].map((online) => {
          const group = filtered.filter((host) => host.online === online);
          return group.length ? (
            <section key={String(online)} aria-label={online ? "Available computers" : "Offline computers"} className="space-y-3">
              <h3 className="text-[13px] font-medium text-muted-foreground">{online ? "Available" : "Offline"} · {group.length}</h3>
              <ul className="grid grid-cols-[repeat(auto-fill,minmax(min(17rem,100%),1fr))] gap-3">
                {group.map((host) => <PairedMachineRow key={host.hostId} host={host} active={false} library onSelect={onSelectHost} onConnect={(target) => {
                  onSelectHost(target.hostId);
                  void remoteDesktopStore.connect(target.hostId, target.displayName ?? target.name);
                }} />)}
              </ul>
            </section>
          ) : null;
        })}
      </div>
    </div>
  );
}

/** Failed and host-ended attempts retain their recovery UI until explicitly dismissed. */
function isOpenSession(session: RemoteDesktopSession): boolean {
  return session.status !== "idle" || session.wireState === "ended";
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
  return Object.values(state.sessions).filter((session) => session.status === "connecting" || session.status === "connected").length;
}

export type GlobalRemoteDesktopCanvasProps = {
  /** False while another Activity Bar mode owns the main area; the canvas hides, never unmounts. */
  active: boolean;
  activeHostId: string | null;
  onSelectHost: (hostId: string | null) => void;
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
  const selectedId = sessions.some((session) => session.hostId === activeHostId) ? activeHostId : null;

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
      <div
          role="tablist"
          aria-label="Remote desktop hosts"
          className="flex h-titlebar shrink-0 items-center gap-1 overflow-x-auto border-b border-border px-2"
        >
          <button type="button" role="tab" aria-selected={selectedId === null} onClick={() => onSelectHost(null)} className="inline-flex items-center gap-2 rounded-md px-3 py-1.5 text-[12px] text-foreground aria-selected:bg-accent">
            <Monitor aria-hidden="true" className="size-4" /> Computers
          </button>
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
                  onClick={() => {
                    if (selected) onSelectHost(null);
                    void remoteDesktopStore.disconnect(session.hostId);
                  }}
                  className="mr-1 inline-flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <X aria-hidden="true" className="size-3" />
                </button>
              </span>
            );
          })}
        </div>
      <div className="relative min-h-0 flex-1">
        {selectedId === null ? (
          <ComputerLibrary onSelectHost={onSelectHost} />
        ) : null}
        {sessions.map((session) => {
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
          })}
      </div>
    </section>
  );
}
