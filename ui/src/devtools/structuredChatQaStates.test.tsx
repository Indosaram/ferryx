import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  STRUCTURED_CHAT_PENDING_GAPS,
  STRUCTURED_CHAT_QA_STATES,
  StructuredChatQaFixture,
  isStructuredChatQaState,
  structuredChatQaParts,
  structuredChatQaPendingGaps,
  structuredChatQaTransport,
} from "./structuredChatQaStates";
import type { StructuredPart } from "../lib/structuredChatTypes";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

describe("structured chat QA states", () => {
  afterEach(() => cleanup());

  it("registers every structured state and rejects a chat state", () => {
    for (const state of STRUCTURED_CHAT_QA_STATES) expect(isStructuredChatQaState(state)).toBe(true);
    expect(isStructuredChatQaState("chips")).toBe(false);
    expect(isStructuredChatQaState("structured-unknown")).toBe(false);
  });

  it("seeds the follow-up state with the same running turn the streaming state uses", () => {
    // The follow-up state differs only in what the capture script types, so it must carry the
    // running turn (an in-progress tool call) that makes the composer offer Queue / Steer.
    const parts = structuredChatQaParts("structured-followup");
    expect(parts.some((part) => part.type === "toolCall" && part.status === "inProgress")).toBe(true);
  });

  it("seeds a streaming turn whose last tool call is still in progress", () => {
    const parts = structuredChatQaParts("structured-streaming");
    expect(parts.some((part) => part.type === "toolCall" && part.status === "inProgress")).toBe(true);
    expect(parts.some((part) => part.type === "userMessage")).toBe(true);
  });

  it("seeds an unanswered approval request for the approval state", () => {
    const parts = structuredChatQaParts("structured-approval");
    const approval = parts.find((part) => part.type === "approvalRequest");
    expect(approval).toBeDefined();
    expect(approval && approval.type === "approvalRequest" ? approval.answered : true).toBe(false);
  });

  it("answers reads from the fake wire and never re-delivers events", async () => {
    const parts = structuredChatQaParts("structured-streaming");
    const transport = structuredChatQaTransport(parts);
    const signal = new AbortController().signal;

    const history = await transport({ op: "history", sessionId: "qa-structured", body: { after: 0 } }, signal);
    expect(history.ok).toBe(true);
    expect(history.ok ? history.data.parts.length : 0).toBe(parts.length);

    const events = await transport({ op: "events", sessionId: "qa-structured", body: { after: parts.length } }, signal);
    expect(events.ok ? events.data.parts : [{ seq: 0 }]).toHaveLength(0);

    const capabilities = await transport({ op: "capabilities", sessionId: "qa-structured", body: {} }, signal);
    expect(capabilities.ok ? capabilities.data.slashCommands : []).toContain("/compact");
  });

  it("has no remaining pending-gaps now that every structured control is wired", () => {
    for (const state of STRUCTURED_CHAT_QA_STATES) {
      expect(structuredChatQaPendingGaps(state)).toHaveLength(0);
    }
    expect(Object.keys(STRUCTURED_CHAT_PENDING_GAPS)).toHaveLength(0);
  });

  it("renders the production pane with real Undo/Compact/Clear and Edit-from controls", async () => {
    render(<StructuredChatQaFixture state="structured-rollback" />);
    await waitFor(() => expect(screen.getByTestId("mobile-chat-workspace")).toBeInTheDocument());
    expect(screen.queryByTestId("qa-pending-gaps")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("structured-undo-button")).toBeInTheDocument());
    expect(screen.getByTestId("structured-compact-button")).toBeInTheDocument();
    expect(screen.getByTestId("structured-clear-button")).toBeInTheDocument();
    expect(screen.getAllByTestId("message-edit-from-here").length).toBeGreaterThan(0);
  });

  it("renders the load-older control when the history page reports hasOlder", async () => {
    render(<StructuredChatQaFixture state="structured-older" />);
    await waitFor(() => expect(screen.getByTestId("reference-older-button")).toBeInTheDocument());
  });

  it("renders the approval card from the seeded request", async () => {
    render(<StructuredChatQaFixture state="structured-approval" />);
    await waitFor(() => expect(screen.getByTestId("reference-prompt-card")).toBeInTheDocument());
    expect(screen.queryByTestId("qa-pending-gaps")).not.toBeInTheDocument();
  });

  it("seeds the older-page state with a page the lane can actually draw", () => {
    const parts: readonly StructuredPart[] = structuredChatQaParts("structured-older");
    expect(parts.length).toBeGreaterThan(0);
    expect(parts.every((part) => typeof part.seq === "number")).toBe(true);
  });
});
