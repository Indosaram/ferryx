import type { RunTarget } from "../../../lib/scopedContracts";
import { useId, useState } from "react";
import { validateHost, type HostConfig } from "./model";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { Label } from "../../../components/ui/label";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "../../../components/ui/select";

export function RunOn({ value = { kind: "local" }, hosts, immutable, onChange }: { value?: RunTarget; hosts: readonly HostConfig[]; immutable?: boolean; onChange: (target: RunTarget) => void }) {
  const hostId = value.kind === "ssh" ? value.hostId : undefined;
  const missing = hostId !== undefined && !hosts.some(host => host.id === hostId);
  return <div className="space-y-1 text-sm">
    <Select value={hostId ? `ssh:${hostId}` : "local"} disabled={immutable || missing} onValueChange={value => onChange(value === "local" ? { kind: "local" } : { kind: "ssh", hostId: value.slice(4) })}>
      <SelectTrigger aria-label="Run on" data-testid="run-on" data-host-id={hostId}><SelectValue /></SelectTrigger>
      <SelectContent><SelectItem value="local">Local</SelectItem>{hosts.map(host => <SelectItem key={host.id} value={`ssh:${host.id}`}>{host.name}</SelectItem>)}{missing && <SelectItem value={`ssh:${hostId}`}>Unavailable host</SelectItem>}</SelectContent>
    </Select>
    {missing && <p role="alert" className="text-muted-foreground">This host was removed. Restore its configuration to reconnect; this worktree will not run locally.</p>}
    {immutable && !missing && <p className="text-muted-foreground">Worktrees keep the target chosen at creation.</p>}
  </div>;
}

export interface HostSettingsProps {
  initial: HostConfig;
  save: (host: HostConfig) => Promise<void>;
  test: (host: HostConfig) => Promise<{ hostId: string; protocol: number }>;
  remove?: (hostId: string) => Promise<void>;
}
/** Desktop settings only: key/trust paths are not rendered in mobile host inventories. */
export function HostSettings({ initial, save, test, remove }: HostSettingsProps) {
  const id = useId();
  const [host, setHost] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const act = async (action: "save" | "test" | "remove") => {
    const invalid = action !== "remove" && validateHost(host);
    if (invalid) { setError(invalid); return; }
    setBusy(true); setError(""); setStatus("");
    try {
      if (action === "test") { const result = await test(host); if (result.protocol !== 1 || result.hostId !== host.id) throw new Error("Remote helper identity or protocol does not match this host."); setStatus("Connected to the configured remote helper."); }
      else if (action === "save") { await save(host); setStatus("Host saved."); }
      else { await remove?.(host.id); setStatus("Host removed. Remote processes remain running."); }
    } catch (error) { setError(error instanceof Error ? error.message : "SSH operation failed. Check the host configuration."); }
    finally { setBusy(false); }
  };
  const fields = [["name", "Name"], ["hostname", "Hostname"], ["user", "User"], ["identityFile", "Identity file"], ["proxyJump", "Proxy jump (optional)"], ["knownHostsFile", "Verified known-hosts file"]] as const;
  return <section data-testid="ssh-host" aria-labelledby={`${id}-heading`} className="space-y-4 text-foreground">
    <h3 id={`${id}-heading`} className="text-sm font-medium">SSH host</h3>
    <p className="text-sm text-muted-foreground">Install the matching ferryx-remote-helper on this host and run its private daemon before connecting. Host keys are never accepted automatically.</p>
    <form className="space-y-3" onSubmit={event => { event.preventDefault(); void act("save"); }}>
      <fieldset disabled={busy} className="grid gap-3 sm:grid-cols-2">
        {fields.map(([field, label]) => <div key={field} className="space-y-1"><Label htmlFor={`${id}-${field}`}>{label}</Label><Input id={`${id}-${field}`} value={host[field] ?? ""} onChange={event => setHost({ ...host, [field]: event.target.value || (field === "identityFile" || field === "proxyJump" ? null : "") })} autoComplete="off" spellCheck={false} /></div>)}
        <div className="space-y-1"><Label htmlFor={`${id}-port`}>Port</Label><Input id={`${id}-port`} type="number" min={1} max={65535} step={1} value={host.port} onChange={event => setHost({ ...host, port: Number(event.target.value) })} /></div>
      </fieldset>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <p role="status" className="text-sm text-muted-foreground">{busy ? "Connecting..." : status}</p>
      <div className="flex flex-wrap gap-2"><Button type="submit" size="sm" disabled={busy}>Save host</Button><Button type="button" variant="outline" size="sm" data-testid="ssh-test" disabled={busy} onClick={() => void act("test")}>Test connection</Button>{remove && <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void act("remove")}>Remove host</Button>}</div>
    </form>
  </section>;
}

export function RemoteReconnect({ reconnect }: { reconnect: () => Promise<void> }) {
  const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  return <div><Button data-testid="remote-reconnect" disabled={busy} onClick={async () => { setBusy(true); setError(""); try { await reconnect(); } catch (e) { setError(e instanceof Error ? e.message : "Reconnect failed"); } finally { setBusy(false); } }}>{busy ? "Reconnecting..." : "Reconnect"}</Button>{error && <p role="alert">{error}</p>}</div>;
}
