import type { TargetRef } from "../../../lib/scopedContracts";
import type { Agent } from "./client";
import { useState } from "react";
import { targetKey, opaqueTarget } from "./client";
import { Button } from "../../../components/ui/button";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../../../components/ui/select";
export function HostSessionSelection({agents, selected, onSelect}: {agents: readonly Agent[]; selected: TargetRef | null; onSelect: (target: TargetRef) => void}) {
  const [host, setHost] = useState("all");
  const [workspace, setWorkspace] = useState("all");
  const hosts = [...new Set(agents.map(a => a.target.hostId))];
  const workspaces = [...new Set(agents.filter(a => host === "all" || host === a.target.hostId).map(a => a.workspaceId))];
  const rows = agents.filter(a => (host === "all" || a.target.hostId === host) && (workspace === "all" || a.workspaceId === workspace));
  return <section aria-label="Host and session selection" className="min-w-0 space-y-2 text-foreground">
    <div className="flex flex-wrap gap-2">
      <Select value={host} onValueChange={value => {setHost(value);setWorkspace("all");}}><SelectTrigger data-testid="host-select" aria-label="Host" className="min-w-0 flex-1"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">All hosts</SelectItem>{hosts.map(h => <SelectItem key={h} value={h}>{h}</SelectItem>)}</SelectContent></Select>
      <Select value={workspace} onValueChange={setWorkspace}><SelectTrigger data-testid="workspace-select" aria-label="Workspace" className="min-w-0 flex-1"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">All workspaces</SelectItem>{workspaces.map(w => <SelectItem key={w} value={w}>{w}</SelectItem>)}</SelectContent></Select>
    </div>
    {rows.length === 0 && <p className="text-xs text-muted-foreground">No registered sessions in this selection.</p>}
    <ul>{rows.map(a => <li key={targetKey(a.target)}><Button variant="ghost" className="w-full justify-start truncate" aria-label={`${a.target.hostId} / ${a.workspaceId} / ${a.label}`} aria-current={selected && targetKey(selected) === targetKey(a.target) ? "true" : undefined} data-target={opaqueTarget(a.target)} disabled={a.state === "exited"} onClick={() => onSelect(a.target)}>{a.label}<span className="truncate text-xs text-muted-foreground">{a.target.hostId} / {a.workspaceId}</span></Button></li>)}</ul>
  </section>;
}
