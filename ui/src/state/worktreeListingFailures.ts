/**
 * Inactive projects whose worktree listing currently fails (e.g. the SSH host
 * resets the connection), keyed by workspaceId with the error description.
 * The sidebar renders this as a badge on the project folder instead of a
 * toast, so a persistent outage does not raise a notification on every reload.
 */
export type WorktreeListingFailures = Readonly<Record<string, string>>;

let currentListingFailures: WorktreeListingFailures = {};
const listingFailureListeners = new Set<() => void>();

export function updateListingFailures(
  update: (current: WorktreeListingFailures) => WorktreeListingFailures,
): void {
  const next = update(currentListingFailures);
  if (next === currentListingFailures) return;
  currentListingFailures = next;
  for (const listener of listingFailureListeners) listener();
}

export const worktreeListingFailureStore = {
  subscribe(listener: () => void): () => void {
    listingFailureListeners.add(listener);
    return () => listingFailureListeners.delete(listener);
  },
  getState(): WorktreeListingFailures {
    return currentListingFailures;
  },
  reset(): void {
    updateListingFailures(() => ({}));
  },
};
