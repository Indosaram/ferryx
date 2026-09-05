/**
 * Binds a dag run to the single pane that owns it.
 *
 * Agent activity flaps between `working` and `waiting` for the whole life of a run, so gating a
 * pane indicator on the live state makes it vanish mid-run. A pane instead claims a run the first
 * time it observes itself working, and keeps that claim until the run stops running.
 */

type OwnerId = string;

const owners = new Map<string, OwnerId>();
const listeners = new Set<() => void>();
let snapshot: Readonly<Record<string, OwnerId>> = {};

function publish(): void {
  snapshot = Object.fromEntries(owners);
  for (const listener of listeners) listener();
}

export const dagRunOwnership = {
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },

  getState(): Readonly<Record<string, OwnerId>> {
    return snapshot;
  },

  ownerOf(runId: string): OwnerId | undefined {
    return owners.get(runId);
  },

  /** First claimer wins, so sibling panes of the same project never take over a live run. */
  claim(runId: string, ownerId: OwnerId, explicit = false): void {
    if (owners.get(runId) === ownerId || (owners.has(runId) && !explicit)) return;
    owners.set(runId, ownerId);
    publish();
  },

  /** Keeps only the runs still running, so finished runs stop reserving their pane. */
  retain(activeRunIds: Iterable<string>, allKnownRunIdsInScope?: Iterable<string>): void {
    const keep = new Set(activeRunIds);
    const scope = allKnownRunIdsInScope ? new Set(allKnownRunIdsInScope) : null;
    let changed = false;
    for (const runId of [...owners.keys()]) {
      const shouldDelete = scope
        ? scope.has(runId) && !keep.has(runId)
        : !keep.has(runId);
      if (shouldDelete) {
        owners.delete(runId);
        changed = true;
      }
    }
    if (changed) publish();
  },

  reset(): void {
    if (owners.size === 0) return;
    owners.clear();
    publish();
  },
};
