/**
 * The persisted pane chat preferences reach a rendered pane.
 *
 * `lib/paneChatPreferences.ts` owns `ferryx.paneChat.toolCallVisibility` and
 * `LocalChatPane` is the only production mount of `DesktopChatPane`. A mount that passes no
 * `toolCallVisibility` still honours what the user saved in Settings -> Chat, because the pane
 * reads the persisted value itself; otherwise the preference would be dead storage and every
 * pane would stay at the `"all"` default.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { DesktopChatPane } from "./DesktopChatPane";
import {
  desktopChatTransportFrom,
  type DesktopChatHandlers,
  type DesktopChatPromptRead,
  type DesktopChatResult,
} from "../lib/desktopChat";
import { PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY } from "../lib/paneChatPreferences";
import type { ReferenceHistoryPage } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "../lib/types";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

const ok = <T,>(data: T): DesktopChatResult<T> => ({ ok: true, data });
const noPrompt: DesktopChatPromptRead = { prompt: null, screenRevision: null };

type MockHandlers = { [K in keyof DesktopChatHandlers]: DesktopChatHandlers[K] };

function handlers(text: string): MockHandlers {
  const page: ReferenceHistoryPage = {
    source: "claude-transcript",
    availability: "native",
    turns: [
      {
        role: "assistant",
        id: "a1",
        parts: [
          { kind: "text", text },
          {
            kind: "tool",
            name: "bash",
            summary: "TOOL_ROW_MARKER",
            input: "echo hi",
            output: "hi",
            status: "completed",
          },
        ],
      },
    ],
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

function session(): TerminalSession {
  return {
    id: "pane-settings",
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId: "be-settings",
    lifecycle: "running",
    ownerId: "owner",
    daemonEpoch: "1",
    agentType: "claude",
  };
}

describe("pane chat tool-call visibility persistence", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("honours the saved ferryx.paneChat.toolCallVisibility when the production mount passes no prop", async () => {
    // What the user saved in Settings -> Chat, under the key this lane declares.
    window.localStorage.setItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY, "hide-all");

    // LocalChatPane supplies exactly these props and nothing more: the pane reads the stored
    // preference itself, so no `toolCallVisibility` prop is needed for it to take effect.
    render(
      <DesktopChatPane
        session={session()}
        visible
        onViewTerminal={vi.fn()}
        transport={desktopChatTransportFrom(handlers("ANSWER_MARKER"))}
        identity={{ hostId: "local" }}
        pollMs={0}
      />,
    );

    expect(await screen.findByText("ANSWER_MARKER")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("TOOL_ROW_MARKER")).not.toBeInTheDocument());
  });
});
