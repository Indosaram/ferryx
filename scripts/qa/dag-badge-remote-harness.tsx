import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";

import "/src/index.css";
import { DagPaneBadge } from "/src/components/dag/DagPaneBadge";
import { parseDagRunSnapshot } from "/src/lib/dagTypes";
import type { DagRunSnapshot } from "/src/lib/dagTypes";
import type { TerminalSession } from "/src/lib/types";
import { dagStore } from "/src/state/dagStore";

export const FIXTURE_ROOT_SESSION_ID = "01a0f117-dfe2-7de2-8c71-3343ae3ab192";
export const FIXTURE_WORKSPACE_ID = "ssh:omarchy:omo-native-rs";
export const FIXTURE_REMOTE_REPO_ROOT = "/home/indo/projects/omo-native-rs";

export function createHarnessSession(
  id: string,
  providerSessionId: string | null,
  workspaceId: string = FIXTURE_WORKSPACE_ID,
  cwd: string = FIXTURE_REMOTE_REPO_ROOT,
): TerminalSession {
  return {
    id,
    cwd,
    worktreePath: cwd,
    workspaceId,
    worktree: null,
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
    providerSession: providerSessionId
      ? { key: "session_id", id: providerSessionId }
      : null,
  };
}

export function DagBadgeRemoteQaHarness(): JSX.Element {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [snapshots, setSnapshots] = useState<readonly DagRunSnapshot[]>([]);

  useEffect(() => {
    let cancelled = false;

    async function loadGraphs(): Promise<void> {
      try {
        dagStore.reset();

        const response = await fetch("/running-graphs.json");
        if (!response.ok) {
          throw new Error(`Failed to fetch /running-graphs.json: ${response.status} ${response.statusText}`);
        }

        const data: unknown = await response.json();
        if (!Array.isArray(data)) {
          throw new Error("Payload at /running-graphs.json is not an array");
        }

        if (data.length < 2) {
          throw new Error(`Payload at /running-graphs.json has ${data.length} runs, expected at least 2`);
        }

        const parsed: DagRunSnapshot[] = [];
        for (let i = 0; i < data.length; i++) {
          const snapshot = parseDagRunSnapshot(data[i]);
          if (!snapshot) {
            throw new Error(`Item ${i} at /running-graphs.json could not be parsed as DagRunSnapshot`);
          }
          parsed.push(snapshot);
        }

        if (cancelled) return;

        const syntheticWatchKey = `ssh:${FIXTURE_WORKSPACE_ID}:${FIXTURE_REMOTE_REPO_ROOT}`;
        for (const snap of parsed) {
          dagStore.applySnapshot(syntheticWatchKey, snap);
        }

        setSnapshots(parsed);
        setLoading(false);
      } catch (err) {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        setLoading(false);
      }
    }

    void loadGraphs();

    return () => {
      cancelled = true;
    };
  }, []);

  const ownerSession = createHarnessSession("pane-owner", FIXTURE_ROOT_SESSION_ID);
  const siblingSession = createHarnessSession("pane-sibling", "01a0ffff-different-session-id");
  const unownedSession = createHarnessSession("pane-unowned", null);
  const allSessions = [ownerSession, siblingSession, unownedSession];

  if (loading) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-zinc-950 text-zinc-300 font-sans">
        <div data-testid="qa-loading" className="flex items-center gap-3">
          <div className="size-4 animate-spin rounded-full border-2 border-zinc-500 border-t-zinc-200" />
          <span>Fetching /running-graphs.json...</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-zinc-950 p-8 text-zinc-100 font-sans">
        <div data-testid="qa-error" className="max-w-md rounded-lg border border-red-500/50 bg-red-950/40 p-6 text-red-200 shadow-lg">
          <h2 className="text-base font-bold text-red-400">Failed to Load /running-graphs.json</h2>
          <p className="mt-2 text-sm font-mono break-all">{error}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen flex-col bg-zinc-950 p-6 text-zinc-100 font-sans">
      <header className="mb-6 border-b border-zinc-800 pb-4">
        <h1 className="text-xl font-bold tracking-tight text-zinc-50">
          Ferryx DAG Remote Badge QA Harness
        </h1>
        <p className="mt-1 text-sm text-zinc-400">
          Standalone snapshot surface verifying exact rootSessionId ownership and workspace binding
          for remote SSH/paired graphs.
        </p>
        <div className="mt-2 flex gap-4 text-xs font-mono text-zinc-500">
          <span data-testid="qa-status">Status: Ready</span>
          <span data-testid="qa-run-count">Loaded runs: {snapshots.length}</span>
          <span data-testid="qa-target-root">Target rootSessionId: {FIXTURE_ROOT_SESSION_ID}</span>
        </div>
      </header>

      <main className="grid flex-1 grid-cols-3 gap-6">
        <section
          data-testid="qa-owner-pane-card"
          className="relative flex flex-col rounded-lg border border-emerald-500/30 bg-zinc-900/60 p-4 shadow-sm"
        >
          <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
            <span className="font-semibold text-emerald-400 text-sm">Owner Pane (Active Agent)</span>
            <span className="rounded bg-emerald-500/10 px-2 py-0.5 text-[11px] font-mono text-emerald-400">
              providerSession match
            </span>
          </div>
          <div className="mt-2 text-xs text-zinc-400 space-y-1">
            <p>Pane ID: <code className="text-zinc-300">pane-owner</code></p>
            <p>Workspace: <code className="text-zinc-300">{ownerSession.workspaceId}</code></p>
            <p>Path: <code className="text-zinc-300">{ownerSession.cwd}</code></p>
            <p>Provider Session: <code className="text-emerald-300">{ownerSession.providerSession?.id}</code></p>
          </div>
          <div className="mt-4 flex-1 rounded bg-black/40 p-4 font-mono text-xs text-zinc-500">
            Terminal output simulation for owner pane...
          </div>
          <div className="relative mt-2 h-10 w-full">
            <DagPaneBadge
              projectPath={ownerSession.worktreePath ?? ownerSession.cwd}
              workspaceId={ownerSession.workspaceId}
              paneId={ownerSession.id}
              providerSessionId={ownerSession.providerSession?.id ?? null}
              sessions={allSessions}
              agentPresent={true}
              agentWorking={true}
            />
          </div>
        </section>

        <section
          data-testid="qa-sibling-pane-card"
          className="relative flex flex-col rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 shadow-sm"
        >
          <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
            <span className="font-semibold text-zinc-300 text-sm">Sibling Pane (Negative Control)</span>
            <span className="rounded bg-zinc-800 px-2 py-0.5 text-[11px] font-mono text-zinc-400">
              different providerSession
            </span>
          </div>
          <div className="mt-2 text-xs text-zinc-400 space-y-1">
            <p>Pane ID: <code className="text-zinc-300">pane-sibling</code></p>
            <p>Workspace: <code className="text-zinc-300">{siblingSession.workspaceId}</code></p>
            <p>Path: <code className="text-zinc-300">{siblingSession.cwd}</code></p>
            <p>Provider Session: <code className="text-zinc-400">{siblingSession.providerSession?.id}</code></p>
          </div>
          <div className="mt-4 flex-1 rounded bg-black/40 p-4 font-mono text-xs text-zinc-500">
            Terminal output simulation for sibling pane (badge MUST NOT render here)...
          </div>
          <div className="relative mt-2 h-10 w-full">
            <DagPaneBadge
              projectPath={siblingSession.worktreePath ?? siblingSession.cwd}
              workspaceId={siblingSession.workspaceId}
              paneId={siblingSession.id}
              providerSessionId={siblingSession.providerSession?.id ?? null}
              sessions={allSessions}
              agentPresent={true}
              agentWorking={true}
            />
          </div>
        </section>

        <section
          data-testid="qa-unowned-pane-card"
          className="relative flex flex-col rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 shadow-sm"
        >
          <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
            <span className="font-semibold text-zinc-300 text-sm">Unowned Pane (Plain Shell)</span>
            <span className="rounded bg-zinc-800 px-2 py-0.5 text-[11px] font-mono text-zinc-400">
              providerSession null
            </span>
          </div>
          <div className="mt-2 text-xs text-zinc-400 space-y-1">
            <p>Pane ID: <code className="text-zinc-300">pane-unowned</code></p>
            <p>Workspace: <code className="text-zinc-300">{unownedSession.workspaceId}</code></p>
            <p>Path: <code className="text-zinc-300">{unownedSession.cwd}</code></p>
            <p>Provider Session: <code className="text-zinc-500">null</code></p>
          </div>
          <div className="mt-4 flex-1 rounded bg-black/40 p-4 font-mono text-xs text-zinc-500">
            Terminal output simulation for plain shell pane (badge MUST NOT render here)...
          </div>
          <div className="relative mt-2 h-10 w-full">
            <DagPaneBadge
              projectPath={unownedSession.worktreePath ?? unownedSession.cwd}
              workspaceId={unownedSession.workspaceId}
              paneId={unownedSession.id}
              providerSessionId={unownedSession.providerSession?.id ?? null}
              sessions={allSessions}
              agentPresent={false}
              agentWorking={false}
            />
          </div>
        </section>
      </main>
    </div>
  );
}

if (typeof document !== "undefined") {
  const rootEl = document.getElementById("root");
  if (rootEl) {
    ReactDOM.createRoot(rootEl).render(
      <React.StrictMode>
        <DagBadgeRemoteQaHarness />
      </React.StrictMode>,
    );
  }
}
