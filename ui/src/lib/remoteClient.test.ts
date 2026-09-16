import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  RemoteClient,
  setRemoteAuthToken,
} from "./remoteClient";

describe("remoteClient storage helpers", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("reads canonical token when set", () => {
    localStorage.setItem("ferryx_remote_token", "test-canonical-token");
    expect(getRemoteAuthToken()).toBe("test-canonical-token");
  });

  it("reads legacy token as fallback and migrates on setRemoteAuthToken", () => {
    localStorage.setItem("rorca_remote_token", "test-legacy-token");
    expect(getRemoteAuthToken()).toBe("test-legacy-token");

    setRemoteAuthToken("test-new-token");
    expect(localStorage.getItem("ferryx_remote_token")).toBe("test-new-token");
    expect(localStorage.getItem("rorca_remote_token")).toBeNull();
    expect(getRemoteAuthToken()).toBe("test-new-token");
  });

  it("clears both canonical and legacy tokens on clearRemoteAuthToken", () => {
    localStorage.setItem("ferryx_remote_token", "canonical");
    localStorage.setItem("rorca_remote_token", "legacy");

    clearRemoteAuthToken();
    expect(localStorage.getItem("ferryx_remote_token")).toBeNull();
    expect(localStorage.getItem("rorca_remote_token")).toBeNull();
    expect(getRemoteAuthToken()).toBeNull();
  });
});

describe("RemoteClient listWorktrees (P15 regression)", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("forwards workspaceId in listWorktrees and returns that workspace's worktrees", async () => {
    const client = new RemoteClient("http://localhost:5173", "test-token");
    let requestedUrl = "";
    global.fetch = vi.fn().mockImplementation(async (url: string) => {
      requestedUrl = url;
      if (url === "http://localhost:5173/api/v1/workspace/worktrees?workspaceId=target-ws") {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            revision: "1",
            worktrees: [
              {
                workspaceId: "target-ws",
                path: "/path/to/target-ws",
                head: "commit-1",
                branch: "refs/heads/main",
                bare: false,
                detached: false,
                locked: null,
                prunable: null,
              },
            ],
          }),
        };
      }
      return {
        ok: false,
        status: 404,
        text: async () => JSON.stringify({ error: { code: "PROJECT_NOT_FOUND" } }),
      };
    });

    const worktrees = await client.listWorktrees("target-ws");
    expect(requestedUrl).toBe("http://localhost:5173/api/v1/workspace/worktrees?workspaceId=target-ws");
    expect(worktrees).toHaveLength(1);
    expect(worktrees[0].path).toBe("/path/to/target-ws");
  });

  it("rejects when unknown workspaceId returns 404 rather than silently returning another workspace", async () => {
    const client = new RemoteClient("http://localhost:5173", "test-token");
    global.fetch = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/v1/workspace/worktrees?workspaceId=unknown-ws")) {
        return {
          ok: false,
          status: 404,
          text: async () => JSON.stringify({ error: { code: "PROJECT_NOT_FOUND" } }),
        };
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({
          activeWorkspaceId: "different-ws",
          worktrees: [{ path: "/path/to/different-ws" }],
        }),
      };
    });

    await expect(client.listWorktrees("unknown-ws")).rejects.toThrow("404");
  });
});
