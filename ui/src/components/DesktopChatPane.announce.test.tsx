/**
 * The screen-reader announcer is wired from the persisted setting (t3code activity-log.md
 * "Screen reader announcements"): a pane whose owner passes no `announceLevel` still announces,
 * because the pane reads the stored level itself. Before this, the level was a prop nobody
 * supplied, so the announcer never rendered in the product and the "Done" at the end of a turn
 * was never said.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

import { DesktopChatPane } from "./DesktopChatPane";
import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import {
  desktopChatTransportFrom,
  type DesktopChatHandlers,
  type DesktopChatPromptRead,
  type DesktopChatResult,
} from "../lib/desktopChat";
import { PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY } from "../lib/paneChatPreferences";
import type { ReferenceHistoryPage } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "../lib/types";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

const ok = <T,>(data: T): DesktopChatResult<T> => ({ ok: true, data });
const noPrompt: DesktopChatPromptRead = { prompt: null, screenRevision: null };

type MockHandlers = { [K in keyof DesktopChatHandlers]: DesktopChatHandlers[K] };

function handlers(): MockHandlers {
  const page: ReferenceHistoryPage = {
    source: "claude-transcript",
    availability: "native",
    turns: [{ role: "assistant", id: "a1", parts: [{ kind: "text", text: "ANSWER_MARKER" }] }],
    hasMore: false,
    generation: "g1",
  };
  return {
    history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(page)),
    prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok(noPrompt)),
    submit: vi.fn<DesktopChatHandlers["submit"]>(async () => ok(null)),
    stop: vi.fn<DesktopChatHandlers["stop"]>(async () => ok(null)),
    answer: vi.fn<DesktopChatHandlers["answer"]>(async () => ok(null)),
    resolvePrompt: vi.fn<DesktopChatHandlers["resolvePrompt"]>(async () => ok(null)),
  };
}

function session(lifecycle: TerminalSession["lifecycle"] = "working"): TerminalSession {
  return {
    id: "pane-announce",
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId: "be-announce",
    lifecycle,
    ownerId: "owner",
    daemonEpoch: "1",
    agentType: "claude",
  };
}

function pane(lifecycle: TerminalSession["lifecycle"]) {
  return (
    <DesktopChatPane
      session={session(lifecycle)}
      visible
      onViewTerminal={vi.fn()}
      transport={desktopChatTransportFrom(handlers())}
      identity={{ hostId: "local" }}
      pollMs={0}
    />
  );
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("pane chat screen-reader announcements", () => {
  it("reads the live turn from the persisted level, with no prop from the owner", async () => {
    window.localStorage.setItem(PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY, "status-only");
    render(pane("working"));
    expect(await screen.findByText("ANSWER_MARKER")).toBeInTheDocument();
    // A running turn's status is the one announcement every level makes.
    await waitFor(() => expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Thinking"));
  });

  it("uses the reference's default level on an install that never opened Settings", async () => {
    render(pane("working"));
    expect(await screen.findByText("ANSWER_MARKER")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Thinking"));
  });

  it("says Done when the live turn ends, and says nothing before one ran", async () => {
    const messages = [{ id: "a1", role: "assistant" as const, content: "answer", timestamp: 1 }];
    // A thread opened after its turns finished has no turn ending here, so nothing is said.
    const { rerender } = render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} announceLevel="status-only" />,
    );
    expect(screen.queryByTestId("thread-announcer")).not.toBeInTheDocument();

    rerender(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
        announceLevel="status-only"
        isRunning
        activityState="thinking"
      />,
    );
    rerender(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} announceLevel="status-only" />,
    );
    expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Done");
  });
});
