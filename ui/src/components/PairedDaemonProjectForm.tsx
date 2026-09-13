import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPairedDaemonProjectAdapter } from "../lib/pairedDaemonProject";
import type { DirectorySource } from "../lib/remoteDirectories";
import type { RegisteredProject } from "../lib/tauri";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import { RemoteDirectoryPicker } from "./RemoteDirectoryPicker";

function unavailable(host: HostEndpoint | undefined, nativeReady: boolean, enabled: boolean): string | null {
  if (!enabled) return "Paired daemon projects are disabled. Enable machine access in Settings.";
  if (!nativeReady) return "Native machine access is unavailable. Upgrade the desktop and local daemon.";
  if (!host) return "Pair a daemon in Settings > Remote Access, then select a machine.";
  if (host.authStatus !== "paired" || host.grantScope !== "machine") return "Pair this daemon with a machine-access PIN in Settings > Remote Access.";
  if (!host.generation || !host.machineId) return "Upgrade the desktop and daemon, then pair this machine again.";
  if (!host.online) return "This machine is offline. Connect its daemon to the relay before adding a project.";
  return null;
}

export function PairedDaemonProjectForm({ onBack, onClose, onRegistered }: {
  onBack: () => void; onClose: () => void; onRegistered: (project: RegisteredProject) => void;
}) {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const hosts = Object.values(state.hosts);
  const [hostId, setHostId] = useState(() => hosts[0]?.hostId ?? "");
  const host = state.hosts[hostId];
  const issue = unavailable(host, state.nativeStatus === "ready", state.machineFeaturesEnabled !== false);
  const key = JSON.stringify([hostId, host?.generation, issue]);
  // Remounting discards selected paths, cached listings, negotiated capabilities and pending UI adoption.
  return <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
    <div role="dialog" aria-label="Add Paired Daemon Project" className="w-full max-w-[480px] space-y-3 rounded-lg border border-border bg-card p-3 shadow-2xl">
      <h2 className="text-[13px] font-medium">Add Paired Daemon Project</h2>
      <p className="text-xs text-muted-foreground">Browse folders on the paired machine without SSH. Pair or manage machines in Settings &gt; Remote Access.</p>
      <PairedFolder key={key} host={host} issue={issue} onClose={onClose} onBack={onBack} onRegistered={onRegistered}
        hostSelector={(busy) => <label className="block text-xs">Paired machine
          <select aria-label="Paired machine" value={hostId} disabled={busy} className="mt-1 h-8 w-full rounded border border-input bg-background px-2"
            onChange={event => setHostId(event.target.value)}>
            <option value="">Select a paired machine</option>
            {hosts.map(row => <option key={row.hostId} value={row.hostId}>{row.displayName ?? row.name}</option>)}
          </select>
        </label>} />
    </div>
  </div>;
}

function PairedFolder({ host, issue, hostSelector, onClose, onBack, onRegistered }: {
  host: HostEndpoint | undefined; issue: string | null; hostSelector: (busy: boolean) => React.ReactNode;
  onClose: () => void; onBack: () => void; onRegistered: (project: RegisteredProject) => void;
}) {
  const adapter = useMemo(() => host && !issue ? createPairedDaemonProjectAdapter({ hostId: host.hostId, generation: host.generation! }) : null, [host?.hostId, host?.generation, issue]);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const submitting = useRef(false);
  const select = useCallback((value: string | null) => setPath(value), []);
  useEffect(() => {
    alive.current = true;
    if (adapter) void adapter.capabilities().then(result => {
      if (!alive.current) return;
      if (!result.capabilities.includes("directoryBrowseV1") || !result.capabilities.includes("machineWorkspaceV1")) {
        setError("Upgrade the paired daemon and relay to support machine folders and project registration.");
      } else setReady(true);
    }).catch(cause => { if (alive.current) setError(`Machine access unavailable (${cause instanceof Error ? cause.message : "PAIRED_HOST_UNAVAILABLE"}). Upgrade or re-pair in Settings > Remote Access.`); });
    return () => { alive.current = false; };
  }, [adapter]);
  const source = useMemo<DirectorySource | undefined>(() => adapter ? {
    key: JSON.stringify(adapter.context), directories: (value, hidden) => adapter.directories(value ?? undefined, hidden),
  } : undefined, [adapter]);
  return <form className="space-y-3" onSubmit={async event => {
    event.preventDefault();
    if (!adapter || !ready || !path || submitting.current) return;
    submitting.current = true; setBusy(true); setError(null);
    try {
      const project = await adapter.registerProject({ requestId: crypto.randomUUID(), repoPath: path });
      if (!alive.current) return;
      onRegistered(project);
      onClose();
    } catch (cause) {
      if (alive.current) setError(`Could not add the paired project (${cause instanceof Error ? cause.message : "PAIRED_HOST_UNAVAILABLE"}).`);
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  }}>
    {hostSelector(busy)}
    {issue || error ? <p role="alert" className="text-xs text-destructive">{issue ?? error}</p> : null}
    {!issue && !ready && !error ? <p role="status" className="text-xs">Checking machine capabilities...</p> : null}
    {ready && source ? <RemoteDirectoryPicker source={source} disabled={busy} onSelect={select} /> : null}
    <div className="flex justify-end gap-2 border-t border-border pt-2">
      <button type="button" disabled={busy} onClick={onBack} className="rounded border px-3 py-1 text-xs">Back</button>
      <button type="button" onClick={onClose} className="rounded border px-3 py-1 text-xs">Cancel</button>
      <button type="submit" data-testid="add-project-confirm-paired" disabled={busy || !ready || !path || !!issue}
        className="rounded bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50">{busy ? "Adding..." : "Add Project"}</button>
    </div>
  </form>;
}
