/**
 * Find is scoped to the pane the user is looking at.
 *
 * DesktopChatPane keeps MobileChatWorkspace mounted while the pane is hidden (`hidden={!visible}`),
 * and the Find hotkey binds `keydown` on `window`. A listener on window is not scoped by a CSS
 * `hidden` attribute: every mounted chat pane would answer the same keystroke, and a split (two
 * leaves both `visible`) would open two bars. Only the active pane may open Find, which is what
 * t3code does (background threads do not react).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DesktopChatPane } from "./DesktopChatPane";
import {
  desktopChatTransportFrom,
  type DesktopChatHandlers,
  type DesktopChatPromptRead,
  type DesktopChatResult,
} from "../lib/desktopChat";
import type { ReferenceHistoryPage } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "../lib/types";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

const ok = <T,>(data: T): DesktopChatResult<T> => ({ ok: true, data });

function emptyPage(): ReferenceHistoryPage {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: [],
    hasMore: false,
    generation: "g1",
  };
}

const noPrompt: DesktopChatPromptRead = { prompt: null, screenRevision: null };

type MockHandlers = { [K in keyof DesktopChatHandlers]: DesktopChatHandlers[K] };

function handlers(): MockHandlers {
  return {
    history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(emptyPage())),
    prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok(noPrompt)),
    submit: vi.fn<DesktopChatHandlers["submit"]>(async () => ok(null)),
    stop: vi.fn<DesktopChatHandlers["stop"]>(async () => ok(null)),
    answer: vi.fn<DesktopChatHandlers["answer"]>(async () => ok(null)),
    resolvePrompt: vi.fn<DesktopChatHandlers["resolvePrompt"]>(async () => ok(null)),
  };
}

function session(id: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId: `be-${id}`,
    lifecycle: "running",
    ownerId: "owner",
    daemonEpoch: "1",
    agentType: "claude",
  };
}

function pane(id: string, visible: boolean) {
  return (
    <DesktopChatPane
      session={session(id)}
      visible={visible}
      onViewTerminal={vi.fn()}
      transport={desktopChatTransportFrom(handlers())}
      identity={{ hostId: "local" }}
      pollMs={0}
    />
  );
}

describe("DesktopChatPane find-hotkey scoping", () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("opens the find bar only in the visible pane when two chat panes are mounted", async () => {
    render(pane("visible", true));
    render(pane("hidden", false));
    await waitFor(() => expect(screen.getAllByTestId("desktop-chat-pane")).toHaveLength(2));

    fireEvent.keyDown(window, { key: "f", ctrlKey: true });

    // The keystroke belongs to the pane the user is looking at. A hidden pane that also opens
    // its bar steals the query, and its own Escape/Enter handling then competes with the live one.
    expect(screen.queryAllByTestId("thread-find-bar")).toHaveLength(1);
  });

  it("does not open a hidden pane's find bar at all", async () => {
    render(pane("only-hidden", false));
    await waitFor(() => expect(screen.getByTestId("desktop-chat-pane")).toHaveAttribute("hidden"));

    fireEvent.keyDown(window, { key: "f", ctrlKey: true });

    expect(screen.queryByTestId("thread-find-bar")).not.toBeInTheDocument();
  });

  it("opens one bar for a split view where both panes are visible", async () => {
    // TerminalSplitView renders every leaf of the active tab with `visible: tabVisible`, so a
    // two-leaf split has two panes that are both `visible` at once. "Scoped to the active pane"
    // has to mean the focused one, which needs an explicit signal the pane is not given today.
    render(pane("left", true));
    render(pane("right", true));
    await waitFor(() => expect(screen.getAllByTestId("desktop-chat-pane")).toHaveLength(2));

    fireEvent.keyDown(window, { key: "f", ctrlKey: true });

    expect(screen.queryAllByTestId("thread-find-bar")).toHaveLength(1);
  });
});
