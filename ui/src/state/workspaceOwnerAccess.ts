import { setHmrWorkspaceState } from "./hmrWorkspaceState";
import { getWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { workspaceReducer, type WorkspaceAction, type WorkspaceState } from "./workspaceStore";

export type OwnedWorkspaceAccess = {
  getState: () => WorkspaceState | null;
  dispatch: (action: WorkspaceAction) => void;
};

/**
 * Binds async work (an agent reconnect) to the workspace that started it.
 *
 * The user can switch projects while a reconnect is in flight. Its later lifecycle updates must
 * still land on the pane's own workspace: dispatching them to whichever workspace is mounted
 * drops them (that store has no such session), and the parked snapshot keeps the pane in
 * "spawning" forever.
 */
export function createOwnedWorkspaceAccess(
  ownerWorkspaceId: string,
  mounted: {
    isMounted: () => boolean;
    getState: () => WorkspaceState;
    dispatch: (action: WorkspaceAction) => void;
  },
): OwnedWorkspaceAccess {
  return {
    getState: () => (mounted.isMounted() ? mounted.getState() : getWorkspaceSnapshot(ownerWorkspaceId)),
    dispatch: (action) => {
      if (mounted.isMounted()) {
        mounted.dispatch(action);
        return;
      }
      const snapshot = getWorkspaceSnapshot(ownerWorkspaceId);
      if (!snapshot) return;
      const next = workspaceReducer(snapshot, action);
      if (next === snapshot) return;
      setWorkspaceSnapshot(ownerWorkspaceId, next);
      setHmrWorkspaceState(ownerWorkspaceId, next);
    },
  };
}
