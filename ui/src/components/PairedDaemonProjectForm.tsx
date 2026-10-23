import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPairedDaemonProjectAdapter } from "../lib/pairedDaemonProject";
import type { DiscoveredRepositories, DiscoveredRepository } from "../lib/pairedDaemonContracts";
import type { MachineProjectTarget } from "../lib/machineNavigation";
import type { DirectorySource } from "../lib/remoteDirectories";
import type { RegisteredProject } from "../lib/tauri";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";
import { safeRandomUUID } from "../lib/uuid";
import { RemoteDirectoryPicker } from "./RemoteDirectoryPicker";
import { useModalDialog } from "./useModalDialog";

function unavailable(host: HostEndpoint | undefined, nativeReady: boolean): string | null {
  if (!nativeReady) return "Native machine access is unavailable. Upgrade the desktop and local daemon.";
  if (!host) return "Pair a daemon in Settings > Remote Access, then select a machine.";
  if (host.authStatus !== "paired" || host.grantScope !== "machine") return "This machine needs machine access. Connect it from your Ferryx account in Settings > Remote Access.";
  if (!host.generation || !host.machineId) return "Upgrade the desktop and daemon, then pair this machine again.";
  if (!host.online) return "This machine is offline. Connect its daemon to the relay before adding a project.";
  return null;
}

export function PairedDaemonProjectForm({ onBack, onClose, onRegistered, initialTarget }: {
  initialTarget?: Extract<MachineProjectTarget, { kind: "pairedDaemon" }>; 
  onBack: () => void; onClose: () => void; onRegistered: (project: RegisteredProject) => void;
}) {
  const state = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const [registering, setRegistering] = useState(false);
  // Escape must not dismiss mid-registration, or onRegistered is skipped for a project that did register.
  const { dialogRef } = useModalDialog({ onEscape: onClose, dismissible: !registering });
  const hosts = Object.values(state.hosts);
  const [selectedHostId, setHostId] = useState(() => hosts[0]?.hostId ?? "");
  const hostId = initialTarget?.hostId ?? selectedHostId;
  const host = state.hosts[hostId];
  const issue = initialTarget && host?.generation !== initialTarget.generation
    ? "Machine credentials changed. Return to Settings and check this machine again."
    : unavailable(host, state.nativeStatus === "ready");
  const key = JSON.stringify([hostId, host?.generation, issue]);
  // Remounting discards selected paths, cached listings, negotiated capabilities and pending UI adoption.
  return <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
    <div ref={dialogRef} role="dialog" aria-modal="true" aria-label="Add Paired Daemon Project" className="w-full max-w-[480px] space-y-3 rounded-lg border border-border bg-card p-3 shadow-2xl">
      <h2 className="text-[13px] font-medium">Add Paired Daemon Project</h2>
      <p className="text-xs text-muted-foreground">Browse folders on the paired machine without SSH. Pair or manage machines in Settings &gt; Remote Access.</p>
      <PairedFolder key={key} host={host} issue={issue} onClose={onClose} onBack={onBack} onRegistered={onRegistered} onBusyChange={setRegistering}
        hostSelector={(busy) => <label className="block text-xs">Paired machine
          <select aria-label="Paired machine" value={hostId} disabled={busy || !!initialTarget} className="mt-1 h-8 w-full rounded border border-input bg-background px-2"
            onChange={event => setHostId(event.target.value)}>
            <option value="">Select a paired machine</option>
            {hosts.map(row => <option key={row.hostId} value={row.hostId}>{row.displayName ?? row.name}</option>)}
          </select>
        </label>} />
    </div>
  </div>;
}

function PairedFolder({ host, issue, hostSelector, onClose, onBack, onRegistered, onBusyChange }: {
  host: HostEndpoint | undefined; issue: string | null; hostSelector: (busy: boolean) => React.ReactNode;
  onClose: () => void; onBack: () => void; onRegistered: (project: RegisteredProject) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const adapter = useMemo(() => host && !issue ? createPairedDaemonProjectAdapter({ hostId: host.hostId, generation: host.generation! }) : null, [host?.hostId, host?.generation, issue]);
  const [ready, setReady] = useState(false);
  const [discovery, setDiscovery] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const submitting = useRef(false);
  const select = useCallback((value: string | null) => setPath(value), []);
  useEffect(() => {
    onBusyChange(busy);
    return () => onBusyChange(false);
  }, [busy, onBusyChange]);
  useEffect(() => {
    alive.current = true;
    if (adapter) void adapter.capabilities().then(result => {
      if (!alive.current) return;
      if (!result.capabilities.includes("directoryBrowseV1") || !result.capabilities.includes("machineWorkspaceV1")) {
        setError("Upgrade the paired daemon and relay to support machine folders and project registration.");
      } else {
        setDiscovery(result.capabilities.includes("repoDiscoveryV1"));
        setReady(true);
      }
    }).catch(cause => { if (alive.current) setError(`Machine access unavailable (${cause instanceof Error ? cause.message : "PAIRED_HOST_UNAVAILABLE"}). Upgrade or re-pair in Settings > Remote Access.`); });
    return () => { alive.current = false; };
  }, [adapter]);
  const source = useMemo<DirectorySource | undefined>(() => adapter ? {
    key: JSON.stringify(adapter.context), directories: (value, hidden) => adapter.directories(value ?? undefined, hidden),
  } : undefined, [adapter]);
  // Stable per adapter so the list scans once per captured host generation.
  const discover = useMemo(() => adapter ? () => adapter.discoverRepos() : undefined, [adapter]);
  const openDiscovered = async (repo: DiscoveredRepository) => {
    if (!adapter || !ready || submitting.current) return;
    if (repo.registeredWorkspaceId !== null && repo.remoteWorkspaceId !== null) {
      onRegistered({ workspaceId: repo.registeredWorkspaceId, repoRoot: repo.repoRoot, gitBranch: repo.branch,
        gitCommonDir: repo.gitCommonDir, target: repo.target, remoteWorkspaceId: repo.remoteWorkspaceId });
      onClose();
      return;
    }
    submitting.current = true; setBusy(true); setError(null);
    try {
      const project = await adapter.registerProject({ requestId: safeRandomUUID(), repoPath: repo.repoRoot });
      if (!alive.current) return;
      onRegistered(project);
      onClose();
    } catch (cause) {
      if (alive.current) setError(`Could not add the paired project (${cause instanceof Error ? cause.message : "PAIRED_HOST_UNAVAILABLE"}).`);
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  };
  return <form className="space-y-3" onSubmit={async event => {
    event.preventDefault();
    if (!adapter || !ready || !path || submitting.current) return;
    submitting.current = true; setBusy(true); setError(null);
    try {
      const project = await adapter.registerProject({ requestId: safeRandomUUID(), repoPath: path });
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
    {ready && discovery && discover ? <DiscoveredRepositoriesList discover={discover} disabled={busy} onOpen={openDiscovered} /> : null}
    {ready && source ? <RemoteDirectoryPicker source={source} disabled={busy} onSelect={select} /> : null}
    <div className="flex justify-end gap-2 border-t border-border pt-2">
      <button type="button" disabled={busy} onClick={onBack} className="rounded border px-3 py-1 text-xs">Back</button>
      <button type="button" disabled={busy} onClick={onClose} className="rounded border px-3 py-1 text-xs disabled:opacity-50">Cancel</button>
      <button type="submit" data-testid="add-project-confirm-paired" disabled={busy || !ready || !path || !!issue}
        className="rounded bg-primary px-3 py-1 text-xs text-primary-foreground disabled:opacity-50">{busy ? "Adding..." : "Add Project"}</button>
    </div>
  </form>;
}

function DiscoveredRepositoriesList({ discover, disabled, onOpen }: {
  discover: () => Promise<DiscoveredRepositories>; disabled: boolean; onOpen: (repo: DiscoveredRepository) => void;
}) {
  const [result, setResult] = useState<DiscoveredRepositories | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    void discover().then(value => { if (current) setResult(value); })
      .catch(cause => { if (current) setFailed(cause instanceof Error ? cause.message : "PAIRED_HOST_UNAVAILABLE"); });
    return () => { current = false; };
  }, [discover]);
  return <section aria-label="Discovered repositories" className="space-y-1">
    <h3 className="text-xs font-medium">Discovered repositories</h3>
    {failed ? <p className="text-xs text-muted-foreground">Could not scan for repositories ({failed}).</p>
      : !result ? <p role="status" className="text-xs text-muted-foreground">Scanning for repositories...</p>
      : result.repos.length === 0 ? <p className="text-xs text-muted-foreground">No repositories found under {result.path}.</p>
      : <ul className="max-h-40 space-y-1 overflow-y-auto">
        {result.repos.map(repo => <li key={repo.repoRoot} data-testid="discovered-repo" className="flex items-center gap-2 rounded border border-border px-2 py-1 text-xs">
          <span className="min-w-0 flex-1 truncate" title={repo.repoRoot}>{repo.repoRoot}</span>
          {repo.branch ? <span className="shrink-0 text-muted-foreground">{repo.branch}</span> : null}
          {repo.isLinkedWorktree ? <span className="shrink-0 rounded border border-border px-1 text-[10px]">worktree</span> : null}
          <button type="button" disabled={disabled} onClick={() => onOpen(repo)} className="shrink-0 rounded border px-2 py-0.5 disabled:opacity-50">
            {repo.registeredWorkspaceId ? "Open" : "Add & open"}
          </button>
        </li>)}
      </ul>}
    {result?.truncated ? <p className="text-xs text-muted-foreground">Showing partial results; browse below for other folders.</p> : null}
  </section>;
}
