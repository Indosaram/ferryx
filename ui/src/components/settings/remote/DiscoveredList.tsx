import type { MachineStatus } from "../../../lib/machineStatus";
import type { SshHost, SshTargetSummary } from "../../../lib/sshHosts";
import { Button } from "../../ui/button";
import { MachineStatusLabel } from "./MachineRow";

export interface DiscoveryBanner {
  tone: "status" | "alert";
  message: string;
}

export interface DiscoveredListProps {
  hosts: { host: SshHost; target?: SshTargetSummary; status: MachineStatus }[];
  banner: DiscoveryBanner | null;
  refreshing: boolean;
  onSetUp: () => void;
  onRetryDiscovery: () => void;
  /** Opens the machine drawer for a saved host, so a host stuck in setup can still be edited, disabled or deleted. */
  onDetails: (hostId: string) => void;
  detailsRef: (hostId: string) => (node: HTMLButtonElement | null) => void;
}

/** Tailscale peers that are not provisioned yet. They become "My machines" once setup finishes. */
export function DiscoveredList({ hosts, banner, refreshing, onSetUp, onRetryDiscovery, onDetails, detailsRef }: DiscoveredListProps) {
  return (
    <section aria-labelledby="remote-discovered" className="space-y-3">
      <header className="flex flex-col gap-1 border-b border-border pb-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 id="remote-discovered" className="text-[13px] font-semibold text-foreground">Discovered</h2>
          <p className="text-[12px] text-muted-foreground">Tailscale peers on your tailnet that are not set up yet.</p>
        </div>
        {banner && !refreshing ? (
          <Button type="button" size="sm" variant="ghost" onClick={onRetryDiscovery}>Retry discovery</Button>
        ) : null}
      </header>
      {banner ? (
        <p role={banner.tone} className={`text-[12px] ${banner.tone === "alert" ? "text-status-warning" : "text-muted-foreground"}`}>
          {banner.message}
        </p>
      ) : null}
      {hosts.length > 0 ? (
        <ul aria-label="Discovered machines" className="divide-y divide-border/60 rounded-lg border border-border bg-card/50">
          {hosts.map(({ host, target, status }) => (
            <li key={host.id} data-discovered-id={host.id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
              <div className="min-w-0">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="truncate text-[13px] font-medium text-foreground">{host.label}</span>
                  <MachineStatusLabel status={status} title={target?.lastError || status.label} />
                </div>
                <div className="truncate font-mono text-[11px] text-muted-foreground">{host.hostname}</div>
                {target?.diagnostic ? (
                  <p role="note" className="break-words text-[12px] text-muted-foreground">
                    {target.diagnostic.details?.stage ?? target.diagnostic.code}: {target.diagnostic.message}
                  </p>
                ) : target && !target.reachable ? (
                  <p className="text-[12px] text-muted-foreground">{target.lastError || "Offline"}</p>
                ) : null}
              </div>
              {/* Every row here is a saved SSH host, so each one also gets Details for Edit, Disable and Delete. */}
              <div className="flex shrink-0 items-center gap-2">
                <Button type="button" size="sm" variant="outline" aria-label={`Set up ${host.label}`} disabled={refreshing} onClick={onSetUp}>
                  Set up
                </Button>
                <Button type="button" size="sm" variant="outline" ref={detailsRef(host.id)} aria-label={`Details for ${host.label}`} aria-haspopup="dialog" onClick={() => onDetails(host.id)}>
                  Details
                </Button>
              </div>
            </li>
          ))}
        </ul>
      ) : !banner ? (
        <p className="text-[12px] text-muted-foreground">No machines waiting for setup.</p>
      ) : null}
    </section>
  );
}
