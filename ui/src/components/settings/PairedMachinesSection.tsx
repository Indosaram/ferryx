import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { pairedHostInventory, DEFAULT_RELAY_ORIGIN, DEFAULT_MACHINE_LABEL } from "../../lib/pairedHostInventory";
import { createPairedDaemonProjectAdapter, type PairedHostContext } from "../../lib/pairedDaemonProject";
import { remoteHostStore, selectHostList, type RemoteHostStore } from "../../state/remoteHostStore";
import type { MachineProjectTarget } from "../../lib/machineNavigation";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

const explanations: Record<string, string> = {
  NATIVE_CONTEXT_REQUIRED: "Native host inventory is unavailable. Use the desktop app with a compatible local daemon; browser mirror access cannot manage machines.",
  PAIR_FAILED: "Could not pair. Check connectivity and daemon compatibility, obtain a fresh machine-access PIN, and retry.",
  MACHINE_GRANT_REQUIRED: "Needs machine access. A mirror PIN cannot authorize projects. Re-pair with an owner-issued machine PIN; revoked credentials cannot reconnect.",
  UNSUPPORTED_CAPABILITY: "This relay, remote daemon, or local daemon does not advertise the required machine capabilities. Upgrade compatible components.",
  STALE_HOST_GENERATION: "Credentials changed during this request. Refresh the inventory and check capabilities again.",
  PAIRED_HOST_UNAVAILABLE: "The native host operation failed. Check the relay and daemon versions, connectivity, and PIN scope, then retry. Saved projects have not been removed.",
  OFFLINE: "Machine is offline. Saved projects remain available in the workspace; reconnect to the owning daemon.",
  UNCHECKED: "Check the remote machine capabilities before adding a project.",
  READY: "Machine project capabilities verified.",
};

async function negotiate(context: PairedHostContext) {
  const result = await createPairedDaemonProjectAdapter(context).capabilities();
  if (!["directoryBrowseV1", "machineWorkspaceV1"].every(capability => result.capabilities.includes(capability))) {
    throw new Error("UNSUPPORTED_CAPABILITY");
  }
}

export function PairedMachinesSection({
  store = remoteHostStore, inventory = pairedHostInventory, negotiate: check = negotiate, onOpenProject, selectedMachine, searchQuery,
}: {
  store?: RemoteHostStore;
  inventory?: typeof pairedHostInventory;
  negotiate?: (context: PairedHostContext) => Promise<unknown>;
  onOpenProject?: (target: MachineProjectTarget) => void;
  selectedMachine?: MachineProjectTarget;
  searchQuery?: string;
}) {
  const state = useSyncExternalStore(store.subscribe, store.getState);
  const [pin, setPin] = useState("");
  const submitting = useRef(false);
  const rows = useRef(new Map<string, HTMLDivElement>());
  const [selected, setSelected] = useState(selectedMachine?.hostId);
  const [repair, setRepair] = useState<string | null>(null);
  useEffect(() => { if (selected) rows.current.get(selected)?.focus(); }, [selected]);
  const [confirm, setConfirm] = useState<PairedHostContext | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checks, setChecks] = useState<Record<string, { generation: string; code: string }>>({});

  async function pair() {
    if (submitting.current || !pin || state.nativeStatus !== "ready") return;
    submitting.current = true;
    setBusy(true); setError(null);
    const request = { relayOrigin: DEFAULT_RELAY_ORIGIN, displayLabel: DEFAULT_MACHINE_LABEL, pin };
    setPin("");
    let context: PairedHostContext | undefined;
    try {
      const paired = await inventory.pair(request, host => {
        context = { hostId: host.hostId, generation: host.generation! };
      });
      if (!paired || !context) setError("PAIR_FAILED");
      else {
        setSelected(context.hostId);
        await checkHost(context);
      }
    } catch { setError("PAIR_FAILED"); }
    finally { setPin(""); submitting.current = false; setBusy(false); }
  }
  async function forget() {
    if (!confirm) return;
    if (state.hosts[confirm.hostId]?.generation !== confirm.generation) {
      setError("STALE_HOST_GENERATION"); setConfirm(null); return;
    }
    setBusy(true); setError(null);
    try { if (await inventory.forget(confirm.hostId)) setConfirm(null); else setError("PAIRED_HOST_UNAVAILABLE"); }
    finally { setBusy(false); }
  }
  async function checkHost(context: PairedHostContext) {
    setBusy(true);
    let code = "READY";
    setChecks(previous => ({ ...previous, [context.hostId]: { generation: context.generation, code: "UNCHECKED" } }));
    try {
      const host = store.getState().hosts[context.hostId];
      if (host?.generation !== context.generation) throw new Error("STALE_HOST_GENERATION");
      if (host.authStatus !== "paired" || host.grantScope !== "machine") throw new Error("MACHINE_GRANT_REQUIRED");
      await check(context);
    }
    catch (failure) {
      const message = failure instanceof Error ? failure.message : "";
      code = Object.prototype.hasOwnProperty.call(explanations, message) ? message : "PAIRED_HOST_UNAVAILABLE";
    } finally { setBusy(false); }
    // Results are presentation only, generation-bound and never an authorization grant.
    if (store.getState().hosts[context.hostId]?.generation !== context.generation) return;
    setChecks(previous => ({ ...previous, [context.hostId]: { generation: context.generation, code } }));
  }

  return <section aria-label="Paired machines" className="mt-8 space-y-3">
    <h3 className="text-[12px] font-semibold">Paired machines</h3>
    <p className="text-[12px] text-muted-foreground">Manage project-owning daemons, not phone mirrors. On the remote machine, the daemon owner starts <code>ferryx-cli --daemon</code> and issues <code>ferryx-cli pair generate --access machine</code>. Enter only the PIN here. New connections use https://relay.checka.cc. Machine traffic stays on the paired relay; no SSH credentials or remote GUI are needed.</p>
    {error ? <p role="alert" data-code={error} className="text-[12px] text-destructive">{explanations[error] ?? error}</p> : null}
    <form className="space-y-2" onSubmit={event => { event.preventDefault(); void pair(); }}>
      <Input aria-label="Machine PIN" type="password" autoComplete="off" disabled={busy} required value={pin} placeholder="PIN from the remote machine" onChange={event => setPin(event.target.value)} />
      <div className="flex flex-wrap gap-2">
        <Button type="submit" disabled={busy || state.nativeStatus !== "ready"}>Pair machine</Button>
        <Button type="button" variant="outline" disabled={busy} onClick={() => void inventory.refresh()}>Refresh machines</Button>
        <Button type="button" variant="ghost" disabled={busy} onClick={() => { setPin(""); setError(null); }}>Cancel pairing</Button>
      </div>

    </form>
    {state.nativeStatus !== "ready" ? <p className="text-[12px] text-status-warning">{explanations.NATIVE_CONTEXT_REQUIRED}</p> : null}
    {selectHostList(state)
      .filter(host => !searchQuery || host.name.toLowerCase().includes(searchQuery.toLowerCase()) || host.hostId.toLowerCase().includes(searchQuery.toLowerCase()) || (host.machineId && host.machineId.toLowerCase().includes(searchQuery.toLowerCase())))
      .map(host => {
      const result = checks[host.hostId];
      const code = state.nativeStatus !== "ready" ? "NATIVE_CONTEXT_REQUIRED"
        : host.authStatus !== "paired" || host.grantScope !== "machine" ? "MACHINE_GRANT_REQUIRED"
        : !host.online ? "OFFLINE"
        : result?.generation === host.generation ? result.code : "UNCHECKED";
      return <div key={host.hostId} tabIndex={-1} ref={node => { if (node) rows.current.set(host.hostId, node); else rows.current.delete(host.hostId); }} data-selected={selected === host.hostId} className="space-y-2 rounded-md border border-border p-3">
        <h4 className="text-[13px] font-medium">{host.name}{host.name === DEFAULT_MACHINE_LABEL ? ` · ${host.machineId?.slice(-8)}` : ""}</h4>
        <p className="break-all text-[11px] text-muted-foreground">{host.machineId} · {host.relayOrigin} · {host.transport} · {host.online ? "Online" : "Offline"} · {host.grantScope ?? "unknown"} grant · {host.authStatus} · generation {host.generation ?? "unknown"}</p>
        <p role="status" data-testid="machine-status" data-code={code} className="text-[12px] text-status-warning">{explanations[code]}</p>
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" aria-label={`Check capabilities for ${host.name}`} disabled={busy || !host.generation || state.nativeStatus !== "ready"} onClick={() => void checkHost({ hostId: host.hostId, generation: host.generation! })}>Check capabilities</Button>
          <Button type="button" aria-label={`Add Project on ${host.name}`} disabled={busy || code !== "READY" || !onOpenProject} onClick={() => onOpenProject?.({ kind: "pairedDaemon", hostId: host.hostId, generation: host.generation! })}>Add Project</Button>
          <Button type="button" variant="outline" aria-label={`Re-pair ${host.name}`} disabled={busy} onClick={() => { setRepair(host.hostId); setPin(""); }}>Re-pair</Button>
          <Button type="button" variant="outline" aria-label={`Forget ${host.name}`} disabled={busy || !host.generation} onClick={() => setConfirm({ hostId: host.hostId, generation: host.generation! })}>Forget credentials</Button>
        </div>
        {repair === host.hostId ? <p role="status" data-code="REPAIR_REQUIRES_IDENTITY_SUPPORT" className="text-xs">Obtain a fresh machine-access PIN from the owner. Identity-checked re-pair is not supported by this desktop API yet. No credentials have been replaced. You can explicitly forget credentials and pair a new connection; verify the returned machine identity.</p> : null}
        {!onOpenProject ? <p className="text-[11px] text-muted-foreground">Use the workspace Add Project dialog and select Paired Daemon. Direct navigation from settings is not connected in this build.</p> : null}
        {confirm?.hostId === host.hostId ? <div role="group" aria-label="Confirm credential removal" className="space-y-2">
          <p className="text-[12px] text-status-warning">Forget credentials for {host.name}? Saved project/layout references and files remain. This does not unregister projects, close remote sessions, or revoke other devices. Re-pair to reconnect.</p>
          <Button type="button" variant="destructive" disabled={busy} aria-label={`Confirm forget ${host.name}`} onClick={() => void forget()}>Confirm forget</Button>
          <Button type="button" variant="outline" disabled={busy} aria-label="Cancel forget" onClick={() => setConfirm(null)}>Cancel</Button>
        </div> : null}
      </div>;
    })}
  </section>;
}
