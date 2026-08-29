import type { DagRunSnapshot } from "../lib/dagTypes";

export type DagStoreState = {
  readonly runsByProject: Readonly<Record<string, Readonly<Record<string, DagRunSnapshot>>>>;
};

export type DagStoreActions = {
  applySnapshot: (projectPath: string, snapshot: DagRunSnapshot) => void;
  removeRun: (projectPath: string, runId: string) => void;
  activeRunIds: (projectPath: string) => readonly DagRunSnapshot["runId"][];
  runSummaries: (projectPath: string) => readonly DagRunSnapshot[];
};

export type DagStore = DagStoreActions & {
  getState: () => DagStoreState;
  setState: (updater: (prev: DagStoreState) => DagStoreState) => void;
  subscribe: (listener: (state: DagStoreState) => void) => () => void;
  reset: () => void;
};

const INITIAL_STATE: DagStoreState = {
  runsByProject: {},
};

export function selectActiveRunIds(
  state: DagStoreState,
  projectPath: string,
): readonly DagRunSnapshot["runId"][] {
  const projectRuns = state.runsByProject[projectPath];
  if (!projectRuns) return [];
  const activeIds: string[] = [];
  for (const run of Object.values(projectRuns)) {
    if (run.status === "running") {
      activeIds.push(run.runId);
    }
  }
  return activeIds;
}

export function selectRunSummaries(
  state: DagStoreState,
  projectPath: string,
): readonly DagRunSnapshot[] {
  const projectRuns = state.runsByProject[projectPath];
  if (!projectRuns) return [];
  const runs = Object.values(projectRuns);
  return runs.slice().sort((a, b) => {
    const bUpdated = b.updatedAt ?? "";
    const aUpdated = a.updatedAt ?? "";
    return bUpdated.localeCompare(aUpdated);
  });
}

export function createDagStore(initialState: DagStoreState = INITIAL_STATE): DagStore {
  let state = initialState;
  const listeners = new Set<(s: DagStoreState) => void>();

  function getState(): DagStoreState {
    return state;
  }

  function setState(updater: (prev: DagStoreState) => DagStoreState): void {
    const nextState = updater(state);
    if (nextState === state) return;
    state = nextState;
    for (const listener of listeners) {
      listener(state);
    }
  }

  function subscribe(listener: (s: DagStoreState) => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }

  function applySnapshot(projectPath: string, snapshot: DagRunSnapshot): void {
    setState((prev) => {
      const prevProjectRuns = prev.runsByProject[projectPath] ?? {};
      const nextProjectRuns = {
        ...prevProjectRuns,
        [snapshot.runId]: snapshot,
      };
      return {
        ...prev,
        runsByProject: {
          ...prev.runsByProject,
          [projectPath]: nextProjectRuns,
        },
      };
    });
  }

  function removeRun(projectPath: string, runId: string): void {
    setState((prev) => {
      const prevProjectRuns = prev.runsByProject[projectPath];
      if (!prevProjectRuns || !(runId in prevProjectRuns)) return prev;
      const { [runId]: _removed, ...nextProjectRuns } = prevProjectRuns;
      return {
        ...prev,
        runsByProject: {
          ...prev.runsByProject,
          [projectPath]: nextProjectRuns,
        },
      };
    });
  }

  function activeRunIds(projectPath: string): readonly DagRunSnapshot["runId"][] {
    return selectActiveRunIds(getState(), projectPath);
  }

  function runSummaries(projectPath: string): readonly DagRunSnapshot[] {
    return selectRunSummaries(getState(), projectPath);
  }

  function reset(): void {
    setState(() => INITIAL_STATE);
  }

  return {
    getState,
    setState,
    subscribe,
    applySnapshot,
    removeRun,
    activeRunIds,
    runSummaries,
    reset,
  };
}

export const dagStore = createDagStore();
