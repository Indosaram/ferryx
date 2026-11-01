import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as browserTauri from "../lib/browserTauri";
import type { BrowserStateChangedPayload } from "../lib/browserTauri";
import { createLayoutState } from "./layout";
import { clearHmrWorkspaceState, useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const workspaceId = "browser-title-follow";
const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

let emit: ((payload: BrowserStateChangedPayload) => void) | null = null;

beforeEach(() => {
  clearHmrWorkspaceState(workspaceId);
  emit = null;
  vi.spyOn(browserTauri, "onBrowserStateChanged").mockImplementation(async (listener) => {
    emit = listener;
    return () => undefined;
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearHmrWorkspaceState(workspaceId);
});

describe("browser tab title on same-tab navigation", () => {
  it("follows the new page title even when the engine reports an empty title in between", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId, services }));
    act(() => result.current.restoreWorkspace({
      workspaceId, worktrees: [], activeWorktreePath: null, sessions: {}, unreadTabIds: {}, unreadWorktreePaths: {},
      layout: createLayoutState([{
        kind: "browser", id: "tab", label: "rust at DuckDuckGo", title: "rust at DuckDuckGo",
        browserId: "b1", url: "https://duckduckgo.com/?q=rust",
      }]),
    }));
    await waitFor(() => expect(emit).toBeTypeOf("function"));

    act(() => emit?.({ browserId: "b1", url: "https://example.com/", title: "" }));
    act(() => emit?.({ browserId: "b1", url: "https://example.com/", title: "Example Domain" }));

    expect(result.current.state.layout.tabs[0]).toMatchObject({
      url: "https://example.com/",
      title: "Example Domain",
      label: "Example Domain",
    });
  });

  it("keeps a label the user renamed", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId, services }));
    act(() => result.current.restoreWorkspace({
      workspaceId, worktrees: [], activeWorktreePath: null, sessions: {}, unreadTabIds: {}, unreadWorktreePaths: {},
      layout: createLayoutState([{
        kind: "browser", id: "tab", label: "My docs", title: "Docs", browserId: "b1", url: "https://docs.example/",
      }]),
    }));
    await waitFor(() => expect(emit).toBeTypeOf("function"));

    act(() => emit?.({ browserId: "b1", url: "https://example.com/", title: "Example Domain" }));

    expect(result.current.state.layout.tabs[0]).toMatchObject({ title: "Example Domain", label: "My docs" });
  });
});
