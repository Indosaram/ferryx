import { Cable, Check, ChevronDown, Laptop, Plus, Radio, Wifi } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { cn } from "../lib/cn";
import { PairMachineModal } from "./PairMachineModal";
import {
  remoteHostStore,
  selectActiveHost,
  selectHostList,
  type HostEndpoint,
  type TransportType,
} from "../state/remoteHostStore";

const TRANSPORT_LABEL: Record<TransportType, string> = {
  tailscale: "Tailscale",
  mdns: "mDNS",
  sshTunnel: "SSH",
  relay: "Relay",
};

const TRANSPORT_ICON: Record<TransportType, typeof Wifi> = {
  tailscale: Wifi,
  mdns: Radio,
  sshTunnel: Cable,
  relay: Laptop,
};

function TransportBadge({ transport }: { transport: TransportType }) {
  const Icon = TRANSPORT_ICON[transport];
  return (
    <span
      data-testid="transport-badge"
      data-transport={transport}
      className="inline-flex shrink-0 items-center gap-1 rounded bg-white/[0.06] px-1.5 py-px text-[10px] font-medium leading-none text-muted-foreground"
    >
      <Icon className="size-2.5" aria-hidden="true" />
      {TRANSPORT_LABEL[transport]}
    </span>
  );
}

function OnlineIndicator({ host }: { host: HostEndpoint }) {
  if (!host.online) {
    return (
      <span
        data-testid="host-online-indicator"
        data-online="false"
        title="Offline"
        className="size-2 shrink-0 rounded-full bg-status-idle"
      />
    );
  }
  const label = host.latencyMs !== undefined ? `${Math.round(host.latencyMs)} ms` : "Online";
  return (
    <span
      data-testid="host-online-indicator"
      data-online="true"
      title={label}
      className="inline-flex shrink-0 items-center gap-1"
    >
      <span className="size-2 rounded-full bg-status-success" />
      {host.latencyMs !== undefined ? (
        <span className="text-[10px] leading-none text-muted-foreground">{Math.round(host.latencyMs)}ms</span>
      ) : null}
    </span>
  );
}

function hostSummaryLabel(host: HostEndpoint | null): string {
  if (!host) return "Local Machine";
  return host.name;
}

type RemoteHostSwitcherProps = {
  className?: string;
};

/**
 * Compact popover for switching the active host between the local machine and any
 * discovered/paired remote hosts. Switching only ever updates `activeHostId` in the remote
 * host registry — it never touches local workspace or terminal session state.
 */
export function RemoteHostSwitcher({ className }: RemoteHostSwitcherProps) {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const [open, setOpen] = useState(false);
  const [pairModalOpen, setPairModalOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const hosts = useMemo(() => selectHostList(state), [state]);
  const activeHost = useMemo(() => selectActiveHost(state), [state]);

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("keydown", closeOnEscape, true);
    return () => {
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("keydown", closeOnEscape, true);
    };
  }, [open]);

  const selectHost = (hostId: string | null) => {
    remoteHostStore.setActiveHost(hostId);
    setOpen(false);
  };

  return (
    <div ref={containerRef} className={cn("relative", className)}>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        data-testid="remote-host-switcher-trigger"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex h-6 min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 text-[11px] font-medium text-worktree-sidebar-foreground/80 transition-colors",
          "hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        )}
      >
        {activeHost ? (
          <span
            data-testid="host-online-indicator-inline"
            className={cn("size-1.5 shrink-0 rounded-full", activeHost.online ? "bg-status-success" : "bg-status-idle")}
          />
        ) : (
          <Laptop className="size-3 shrink-0" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 truncate text-left">{hostSummaryLabel(activeHost)}</span>
        <ChevronDown className={cn("size-3 shrink-0 transition-transform", open && "rotate-180")} aria-hidden="true" />
      </button>

      {open ? (
        <div
          role="listbox"
          aria-label="Switch host"
          data-testid="remote-host-switcher-menu"
          className="absolute left-0 top-full z-40 mt-1 w-64 max-w-[80vw] overflow-hidden rounded-md border border-worktree-sidebar-border bg-popover p-1 shadow-lg"
        >
          <button
            type="button"
            role="option"
            aria-selected={state.activeHostId === null}
            data-testid="remote-host-option-local"
            onClick={() => selectHost(null)}
            className={cn(
              "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs transition-colors hover:bg-accent",
              state.activeHostId === null ? "bg-accent/70 text-foreground" : "text-muted-foreground",
            )}
          >
            <Laptop className="size-3.5 shrink-0" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate font-medium">Local Machine</span>
            {state.activeHostId === null ? <Check className="size-3.5 shrink-0" aria-hidden="true" /> : null}
          </button>

          {hosts.length > 0 ? (
            <div className="my-1 h-px bg-worktree-sidebar-border" />
          ) : null}

          {hosts.length === 0 ? (
            <p className="px-2 py-2 text-[11px] leading-relaxed text-muted-foreground">
              No remote hosts discovered yet.
            </p>
          ) : (
            hosts.map((host) => {
              const active = state.activeHostId === host.hostId;
              return (
                <button
                  key={host.hostId}
                  type="button"
                  role="option"
                  aria-selected={active}
                  data-testid={`remote-host-option-${host.hostId}`}
                  onClick={() => selectHost(host.hostId)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs transition-colors hover:bg-accent",
                    active ? "bg-accent/70 text-foreground" : "text-muted-foreground",
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-1.5">
                      <span className="min-w-0 flex-1 truncate font-medium">{host.name}</span>
                      {active ? <Check className="size-3.5 shrink-0" aria-hidden="true" /> : null}
                    </span>
                    <span className="mt-0.5 flex items-center gap-1.5">
                      <TransportBadge transport={host.transport} />
                      <OnlineIndicator host={host} />
                      {host.authStatus !== "paired" ? (
                        <span
                          data-testid="host-auth-status"
                          data-auth-status={host.authStatus}
                          className="text-[10px] leading-none text-status-warning"
                        >
                          {host.authStatus === "unpaired" ? "Unpaired" : "Unknown"}
                        </span>
                      ) : null}
                    </span>
                  </span>
                </button>
              );
            })
          )}

          <div className="my-1 h-px bg-worktree-sidebar-border" />
          <button
            type="button"
            data-testid="pair-remote-machine-button"
            onClick={() => {
              setOpen(false);
              setPairModalOpen(true);
            }}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs font-medium text-worktree-sidebar-foreground transition-colors hover:bg-accent"
          >
            <Plus className="size-3.5 shrink-0" aria-hidden="true" />
            <span>Pair Remote Machine...</span>
          </button>
        </div>
      ) : null}

      <PairMachineModal open={pairModalOpen} onClose={() => setPairModalOpen(false)} />
    </div>
  );
}
