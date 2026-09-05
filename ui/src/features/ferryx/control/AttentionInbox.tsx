import type { InventorySnapshot, TargetRef } from "../../../lib/scopedContracts";
import type { Agent } from "./client";
import { opaqueTarget, targetKey } from "./client";
import { Button } from "../../../components/ui/button";
export function AttentionInbox({snapshot, onSelect, isUnread, loading, error}: {snapshot: InventorySnapshot<Agent>; onSelect: (target: TargetRef) => void; isUnread: (agent: Agent) => boolean; loading?: boolean; error?: string}) {
  const rows = snapshot.items.filter(a => a.state === "waiting" || isUnread(a));
  return <section data-testid="waiting-inbox" aria-label="Attention inbox" aria-busy={loading} className="min-w-0 text-foreground">
    <h2 className="px-3 py-2 text-sm font-medium">Attention <span className="text-muted-foreground">{rows.length}</span></h2>
    {loading && <p role="status" className="px-3 text-xs text-muted-foreground">Loading agents...</p>}
    {error && <p role="alert" className="px-3 text-xs text-destructive">{error}</p>}
    {snapshot.completeness !== "complete" && <p role="status" className="px-3 py-2 text-xs text-muted-foreground">Inventory incomplete{snapshot.unavailableHosts.length > 0 ? `: ${snapshot.unavailableHosts.join(", ")}` : ""}</p>}
    {!loading && !error && rows.length === 0 && <p className="px-3 py-2 text-xs text-muted-foreground">No agents need attention.</p>}
    <ul>{rows.map(a => <li key={targetKey(a.target)}><Button variant="ghost" data-testid="waiting-target" data-target={opaqueTarget(a.target)} disabled={a.state === "exited"} onClick={() => onSelect(a.target)} className="h-auto w-full justify-start px-3 py-2 text-left">
      <span className="min-w-0 flex-1"><span className="block truncate">{a.label}</span><span className="block truncate text-xs text-muted-foreground">{a.target.hostId} / {a.workspaceId}</span></span>
      <span className="shrink-0 text-xs">{a.state === "waiting" ? "Waiting" : a.state}{isUnread(a) ? " · Unread" : ""}</span>
    </Button></li>)}</ul>
  </section>;
}
