import { describe, expect, it } from "vitest";

import type { WorkspaceState } from "../../state/workspaceStore";
import { findSessionByBackendId } from "./findSession";

const state = (sessions: Record<string, { id: string; backendSessionId: string | null }>) =>
  ({ sessions }) as unknown as WorkspaceState;

describe("findSessionByBackendId", () => {
  it("matches the backend id after the host prefix across workspaces", () => {
    const found = findSessionByBackendId("mac:b-2", [
      ["w1", state({ a: { id: "a", backendSessionId: "b-1" } })],
      ["w2", state({ c: { id: "c", backendSessionId: "b-2" } })],
    ]);
    expect(found).toEqual({ workspaceId: "w2", sessionId: "c" });
  });

  it("returns null when no open pane runs the session", () => {
    expect(findSessionByBackendId("mac:b-9", [["w1", state({})]])).toBeNull();
  });
});
