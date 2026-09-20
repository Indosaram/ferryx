import { useEffect, useRef, useState } from "react";
import { dagStore } from "../state/dagStore";
import { listenDagRunUpdated, listenDagWatchStatus, unwatchDagProject, watchDagPairedProject, watchDagProject, watchDagSshProject } from "./tauri";
import type { DagRunUpdatedEvent } from "./tauri";

type RemoteTarget = { readonly kind: "ssh" | "pairedDaemon"; readonly workspaceId: string; readonly remotePath: string };
type Input = { readonly localRoots: readonly string[]; readonly remoteTargets: readonly RemoteTarget[]; readonly watchKey: string };

export function dagRemoteWatchTargets(projects: readonly {
  readonly workspaceId: string; readonly repoRoot: string; readonly target?: { readonly kind: string } | null;
}[]): RemoteTarget[] {
  return projects.flatMap((project): RemoteTarget[] => {
    const kind = project.target?.kind;
    return kind === "ssh" || kind === "pairedDaemon"
      ? [{ kind, workspaceId: project.workspaceId, remotePath: project.repoRoot }] : [];
  });
}

export function useDagWatchLifecycle(input: Input): void {
  const latest = useRef(input);
  latest.current = input;
  const queue = useRef<Promise<void>>(Promise.resolve());
  const [retryGeneration, setRetryGeneration] = useState(0);
  useEffect(() => {
    let disposed = false;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    const retry = () => {
      if (disposed || retryTimer !== undefined) return;
      retryTimer = setTimeout(() => {
        if (!disposed) setRetryGeneration((value) => value + 1);
      }, 1500);
    };
    let unlisten: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;
    const active = new Set<string>();
    const generations = new Map<string, number>();
    const pending = new Map<string, Map<string, DagRunUpdatedEvent>>();
    const observed = new Map<string, Set<string>>();
    const current = latest.current;
    const release = async (key: string) => {
      try { await unwatchDagProject(key); }
      catch (error) { console.error("DAG unsubscribe failed", key, error); }
    };
    const setup = queue.current.then(async () => {
      if (disposed) return;
      try {
        unlisten = await listenDagRunUpdated((event) => {
          if (disposed) return;
          const remote = event.projectPath.startsWith("ssh:") || event.projectPath.startsWith("paired:");
          if (remote && !active.has(event.projectPath)) return;
          if (remote && generations.has(event.projectPath) && event.generation !== generations.get(event.projectPath)) return;
          if (remote && event.generation !== undefined) {
            const generation = generations.get(event.projectPath);
            if (generation === undefined) {
              let events = pending.get(event.projectPath);
              if (!events) { events = new Map(); pending.set(event.projectPath, events); }
              const previous = events.get(event.snapshot.runId);
              if (!previous || (previous.generation ?? 0) <= event.generation) {
                events.set(event.snapshot.runId, event);
              }
              return;
            }
            if (generation !== event.generation) return;
          }
          let runs = observed.get(event.projectPath);
          if (!runs) { runs = new Set(); observed.set(event.projectPath, runs); }
          runs.add(event.snapshot.runId);
          dagStore.applySnapshot(event.projectPath, event.snapshot);
        });
        if (disposed) { unlisten(); unlisten = undefined; return; }
        void (async () => {
          try {
            const stop = await listenDagWatchStatus((event) => {
              if (disposed) return;
              // Mirror the run listener: a status tagged with a foreign (or missing) generation
              // must not poison a watch whose generation is already established.
              const remote = event.projectPath.startsWith("ssh:") || event.projectPath.startsWith("paired:");
              if (remote && !active.has(event.projectPath)) return;
              if (remote) {
                const generation = generations.get(event.projectPath);
                if (generation !== undefined && event.generation !== generation) return;
              }
              dagStore.setWatchFailure(event.projectPath, event);
            });
            if (disposed) stop();
            else unlistenStatus = stop;
          } catch (error) {
            // A bridge without the status channel must not break run subscriptions.
            console.error("DAG watch-status listener registration failed", error);
          }
        })();
        if (disposed) { unlistenStatus?.(); unlistenStatus = undefined; return; }
        const requests = [
          ...current.localRoots.map((key) => ({ key, start: () => watchDagProject(key) })),
          ...current.remoteTargets.map((target) => ({
            key: `${target.kind === "ssh" ? "ssh" : "paired"}:${target.workspaceId}:${target.remotePath}`,
            start: () => target.kind === "ssh"
              ? watchDagSshProject(target.workspaceId, target.remotePath)
              : watchDagPairedProject(target.workspaceId, target.remotePath),
          })),
        ];
        await Promise.all(requests.map(async ({ key, start }) => {
          if (active.has(key)) return;
          active.add(key);
          try {
            const result = await start();
            active.delete(key);
            active.add(result.projectPath);
            if (disposed) return;
            if (result.generation != null) generations.set(result.projectPath, result.generation);
            dagStore.setWatchFailure(result.projectPath, result.failure ?? null);
            for (const event of pending.get(result.projectPath)?.values() ?? []) {
              if (event.generation !== result.generation) continue;
              let runs = observed.get(result.projectPath);
              if (!runs) { runs = new Set(); observed.set(result.projectPath, runs); }
              runs.add(event.snapshot.runId);
              dagStore.applySnapshot(result.projectPath, event.snapshot);
            }
            pending.delete(result.projectPath);
            for (const snapshot of result.runs) {
              if (!observed.get(result.projectPath)?.has(snapshot.runId)) {
                dagStore.applySnapshot(result.projectPath, snapshot);
              }
            }
          } catch (error) {
            active.delete(key);
            console.error("DAG subscription failed", key, error);
            retry();
          }
        }));
      } catch (error) {
        console.error("DAG listener registration failed", error);
        retry();
      }
    });
    queue.current = setup;
    return () => {
      disposed = true;
      clearTimeout(retryTimer);
      unlisten?.();
      unlisten = undefined;
      unlistenStatus?.();
      unlistenStatus = undefined;
      queue.current = setup.then(async () => {
        await Promise.all([...active].map(release));
        active.clear();
      });
    };
  }, [input.watchKey, retryGeneration]);
}
