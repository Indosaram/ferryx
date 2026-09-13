import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseDagRunSnapshot } from "../lib/dagTypes";
import type { LayoutState, TerminalSession } from "../lib/types";
import sample from "../state/__fixtures__/dagRunSample.json";
import { dagStore } from "../state/dagStore";
import { TerminalSplitView } from "./TerminalSplitView";

// Only the platform boundary is replaced. Pane handlers, native sink, modal
// portal, visibility observer and focus restoration are production components.
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));

const session: TerminalSession = {
  id: "focus-session", backendSessionId: "focus-backend", cwd: "/repo",
  worktreePath: "/repo", workspaceId: "ws", worktree: null,
  lifecycle: "working", providerSession: { key: "session_id", id: "provider" },
};
const layout: LayoutState = {
  tabs: [{ id: "tab", label: "tab", sessionId: session.id }], activeTabId: "tab",
  layoutsByTabId: { tab: {
    root: { type: "leaf", leafId: "leaf" }, activeLeafId: "leaf", expandedLeafId: null,
    sessionIdsByLeafId: { leaf: session.id },
  } },
};

// Subscribe before DOM mutation; act drains the visibility observer's React
// update. Fake timers explicitly flush both rAF and the 40ms focus retry.
async function mutate(action: () => void) {
  const changed = new Promise<void>((resolve) => {
    const observer = new MutationObserver(() => { observer.disconnect(); resolve(); });
    observer.observe(document.body, { childList: true, subtree: true });
  });
  act(action);
  await act(async () => { await changed; });
}
function flushFocus() { act(() => { vi.advanceTimersByTime(40); }); }

beforeEach(() => {
  vi.useFakeTimers();
  dagStore.reset();
  dagStore.applySnapshot("/repo", {
    ...parseDagRunSnapshot(sample)!, runId: "focus-run", rootSessionId: "provider", status: "running",
  });
});
afterEach(() => { cleanup(); dagStore.reset(); vi.useRealTimers(); });

async function openModal() {
  const onFocusPane = vi.fn();
  render(<TerminalSplitView layout={layout} sessions={{ [session.id]: session }} onFocusPane={onFocusPane} />);
  flushFocus();
  const badge = screen.getByTestId("dag-pane-badge-button");
  badge.focus();
  await mutate(() => fireEvent.click(badge));
  flushFocus();
  expect(screen.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-input-enabled", "false");
  onFocusPane.mockClear();
  return { badge, onFocusPane };
}

describe("real pane/modal portal focus ownership", () => {
  it.each(["pointerDown", "click"] as const)("does not route modal summary %s into the native sink", async (event) => {
    const { onFocusPane } = await openModal();
    const modal = screen.getByRole("dialog");
    const summary = document.createElement("summary");
    summary.tabIndex = 0;
    modal.appendChild(summary);
    summary.focus();
    fireEvent[event](summary);
    expect(document.activeElement).toBe(summary);
    flushFocus();
    expect(document.activeElement).toBe(summary);
    expect(onFocusPane).not.toHaveBeenCalled();
    fireEvent.keyDown(summary, { key: "Tab" });
    expect(modal.contains(document.activeElement)).toBe(true);
  });

  it.each(["escape", "backdrop", "close"])("preserves saved badge focus after %s and still focuses ordinary pane clicks", async (close) => {
    const { badge, onFocusPane } = await openModal();
    await mutate(() => {
      if (close === "escape") fireEvent.keyDown(screen.getByTestId("dag-pane-modal-close"), { key: "Escape" });
      else if (close === "close") fireEvent.click(screen.getByTestId("dag-pane-modal-close"));
      else {
        fireEvent.pointerDown(screen.getByTestId("dag-pane-modal-backdrop"));
        fireEvent.click(screen.getByTestId("dag-pane-modal-backdrop"));
      }
    });
    expect(document.activeElement).toBe(badge);
    flushFocus();
    expect(document.activeElement).toBe(badge);
    expect(onFocusPane).not.toHaveBeenCalled();
    expect(screen.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-input-enabled", "true");
    const leaf = screen.getByTestId("pane-leaf");
    for (const event of ["pointerDown", "click"] as const) {
      badge.focus();
      fireEvent[event](leaf);
      expect(document.activeElement).toBe(screen.getByTestId("native-terminal-focus-sink"));
      flushFocus();
      expect(document.activeElement).toBe(screen.getByTestId("native-terminal-focus-sink"));
    }
    expect(onFocusPane).toHaveBeenCalledWith("tab", "leaf");
  });
});
