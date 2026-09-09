import { Cable, Check, Laptop, Radio, Wifi, X } from "lucide-react";
import { useEffect, useMemo, useSyncExternalStore } from "react";

import { cn } from "../lib/cn";
import {
  remoteHostStore,
  selectHostList,
  type HostEndpoint,
  type RemoteHostState,
  type TransportType,
} from "../state/remoteHostStore";

const TRANSPORT_LABEL: Record<TransportType, string> = {
  tailscale: "Tailscale",
  mdns: "mDNS",
  sshTunnel: "SSH",
};

const TRANSPORT_ICON: Record<TransportType, typeof Wifi> = {
  tailscale: Wifi,
  mdns: Radio,
  sshTunnel: Cable,
};

/**
 * Per-host rollup of agent activity. This isn't part of `HostEndpoint` yet — hosts may carry an
 * optional `agentSummary` payload (populated by discovery/status sync) that we read defensively
 * so the drawer degrades to "no data" instead of crashing when it's absent.
 */
export type HostAgentSummary = {
  running: number;
  waiting: number;
};

export type HostWithAgentSummary = HostEndpoint & {
  agentSummary?: Partial<HostAgentSummary>;
};

export function getHostAgentSummary(host: HostEndpoint): HostAgentSummary {
  const raw = (host as HostWithAgentSummary).agentSummary;
  const running = typeof raw?.running === "number" ? raw.running : 0;
  const waiting = typeof raw?.waiting === "number" ? raw.waiting : 0;
  return { running, waiting };
}

function TransportBadge({ transport }: { transport: TransportType }) {
  const Icon = TRANSPORT_ICON[transport];
  return (
    <span
      data-testid="mobile-host-transport-badge"
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
        data-testid="mobile-host-online-indicator"
        data-online="false"
        title="Offline"
        className="size-2 shrink-0 rounded-full bg-status-idle"
      />
    );
  }
  const label = host.latencyMs !== undefined ? `${Math.round(host.latencyMs)} ms` : "Online";
  return (
    <span
      data-testid="mobile-host-online-indicator"
      data-online="true"
      title={label}
      className="size-2 shrink-0 rounded-full bg-status-success"
    />
  );
}

function AgentSummaryPill({ summary }: { summary: HostAgentSummary }) {
  if (summary.running === 0 && summary.waiting === 0) return null;
  return (
    <span data-testid="mobile-host-agent-summary" className="flex shrink-0 items-center gap-1">
      {summary.running > 0 ? (
        <span
          data-testid="mobile-host-agent-running"
          className="inline-flex items-center gap-0.5 rounded bg-status-working/15 px-1 text-[10px] font-mono leading-tight text-status-working"
        >
          {summary.running} running
        </span>
      ) : null}
      {summary.waiting > 0 ? (
        <span
          data-testid="mobile-host-agent-waiting"
          className="inline-flex items-center gap-0.5 rounded bg-status-warning/15 px-1 text-[10px] font-mono leading-tight text-status-warning"
        >
          {summary.waiting} waiting
        </span>
      ) : null}
    </span>
  );
}

export function hostAgentTotals(state: RemoteHostState): HostAgentSummary {
  return selectHostList(state).reduce<HostAgentSummary>(
    (acc, host) => {
      const summary = getHostAgentSummary(host);
      return { running: acc.running + summary.running, waiting: acc.waiting + summary.waiting };
    },
    { running: 0, waiting: 0 },
  );
}

export type MobileHostDrawerProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/**
 * Mobile-friendly full-width drawer for switching the active host. Mirrors the desktop
 * `RemoteHostSwitcher` behavior (switching only ever updates `activeHostId`) but uses a
 * bottom-sheet layout sized for touch targets instead of a compact popover.
 */
export function MobileHostDrawer({ open, onOpenChange }: MobileHostDrawerProps) {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const hosts = useMemo(() => selectHostList(state), [state]);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onOpenChange(false);
    };
    window.addEventListener("keydown", closeOnEscape, true);
    return () => window.removeEventListener("keydown", closeOnEscape, true);
  }, [open, onOpenChange]);

  if (!open) return null;

  const selectHost = (hostId: string | null) => {
    remoteHostStore.setActiveHost(hostId);
    onOpenChange(false);
  };

  return (
    <div className="fixed inset-0 z-50 flex flex-col justify-end" data-testid="mobile-host-drawer">
      <button
        type="button"
        aria-label="Close host switcher"
        data-testid="mobile-host-drawer-backdrop"
        onClick={() => onOpenChange(false)}
        className="absolute inset-0 bg-black/40"
      />
      <div
        role="dialog"
        aria-label="Switch host"
        className="relative z-10 max-h-[80vh] overflow-y-auto rounded-t-xl border-t border-border bg-popover text-popover-foreground shadow-2xl scrollbar-sleek"
      >
        <div className="flex items-center justify-between border-b border-border px-3 py-2.5">
          <h2 className="text-sm font-semibold">Switch host</h2>
          <button
            type="button"
            aria-label="Close host switcher"
            onClick={() => onOpenChange(false)}
            className="flex size-9 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>

        <div className="p-2">
          <button
            type="button"
            aria-selected={state.activeHostId === null}
            data-testid="mobile-host-option-local"
            onClick={() => selectHost(null)}
            className={cn(
              "flex min-h-11 w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
              state.activeHostId === null ? "bg-accent/70 text-foreground" : "text-muted-foreground",
            )}
          >
            <Laptop className="size-4 shrink-0" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate text-sm font-medium">Local Machine</span>
            {state.activeHostId === null ? <Check className="size-4 shrink-0" aria-hidden="true" /> : null}
          </button>

          {hosts.length === 0 ? (
            <p className="px-3 py-6 text-center text-xs text-muted-foreground">
              No remote hosts discovered yet.
            </p>
          ) : (
            <div className="mt-1 space-y-1">
              {hosts.map((host) => {
                const active = state.activeHostId === host.hostId;
                const summary = getHostAgentSummary(host);
                return (
                  <button
                    key={host.hostId}
                    type="button"
                    aria-selected={active}
                    data-testid={`mobile-host-option-${host.hostId}`}
                    onClick={() => selectHost(host.hostId)}
                    className={cn(
                      "flex min-h-11 w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      active ? "bg-accent/70 text-foreground" : "text-muted-foreground",
                    )}
                  >
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="min-w-0 flex-1 truncate text-sm font-medium">{host.name}</span>
                        {active ? <Check className="size-4 shrink-0" aria-hidden="true" /> : null}
                      </span>
                      <span className="mt-1 flex flex-wrap items-center gap-1.5">
                        <TransportBadge transport={host.transport} />
                        <OnlineIndicator host={host} />
                        {host.authStatus !== "paired" ? (
                          <span
                            data-testid="mobile-host-auth-status"
                            data-auth-status={host.authStatus}
                            className="text-[10px] leading-none text-status-warning"
                          >
                            {host.authStatus === "unpaired" ? "Unpaired" : "Unknown"}
                          </span>
                        ) : null}
                        <AgentSummaryPill summary={summary} />
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
