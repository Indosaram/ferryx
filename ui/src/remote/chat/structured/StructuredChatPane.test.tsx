/**
 * `StructuredChatPane` renders the structured lane through the existing workspace.
 *
 * The assertions stay on what the user can see and press: the messages the stream produced, the
 * approval card the lane mounts, the composer's own controls, and the queue/steer routing the
 * workspace already offers. Nothing here reaches into the hook's internals.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { StructuredChatPane } from "./StructuredChatPane";
import {
  STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
  STRUCTURED_CHAT_SESSION_ENDED,
} from "../../../lib/structuredChat";
import type {
  StructuredCapabilities,
  StructuredPart,
  StructuredSessionSummary,
} from "../../../lib/structuredChatTypes";
import type { StructuredChatOp, StructuredChatTransport } from "../../../lib/structuredChatTransport";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

function at(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1000 + seq, ...extra } as StructuredPart;
}

const CAPABILITIES: StructuredCapabilities = {
  steer: true,
  rollback: true,
  permissionModes: ["supervised", "full-access"],
  models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
  slashCommands: ["/compact"],
};

const SUMMARY: StructuredSessionSummary = {
  sessionId: "s1",
  provider: "codex",
  cwd: "/repo",
  state: "running",
  settings: { model: "gpt-5", permissionMode: "supervised" },
};

function transportFor(parts: StructuredPart[], approved: { requestId: string; decision: string }[] = []) {
  const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }) => {
    switch (request.op) {
      case "listSessions":
        return { ok: true, data: [SUMMARY] };
      case "capabilities":
        return { ok: true, data: CAPABILITIES };
      case "history":
      case "events":
        return { ok: true, data: { parts, nextSeq: parts.length } };
      case "approve": {
        approved.push(request.body as { requestId: string; decision: string });
        return { ok: true, data: undefined };
      }
      case "turn":
      case "steer":
        return { ok: true, data: 99 };
      // The lane reads these three per session; a fake that answered `undefined` would hand the
      // hook a shape the real transport's decoders never produce.
      case "commands":
        return { ok: true, data: [] };
      case "tree":
        return { ok: true, data: { nodes: [] } };
      case "stats":
        return { ok: true, data: { turns: 0, totalTokens: 0, inputTokens: 0, outputTokens: 0 } };
      default:
        return { ok: true, data: undefined };
    }
  }) as unknown as StructuredChatTransport;
  return transport;
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

describe("StructuredChatPane", () => {
  it("draws the structured stream and lights up the model and permission controls", async () => {
    const transport = transportFor([
      at(1, { turnId: "t1", type: "userMessage", text: "STRUCTURED_USER_MARKER" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "STRUCTURED_ASSISTANT_MARKER" }),
    ]);
    render(<StructuredChatPane transport={transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("STRUCTURED_ASSISTANT_MARKER")).toBeInTheDocument());
    expect(screen.getByText("STRUCTURED_USER_MARKER")).toBeInTheDocument();
    // the composer's model and permission controls are armed from the session's settings
    await waitFor(() => expect(screen.getByTestId("composer-model-control")).toHaveValue("gpt-5"));
    expect(screen.getByTestId("composer-option-permissionMode")).toHaveValue("supervised");
  });

  it("mounts the approval card for a pending request and answers it with the picked decision", async () => {
    const approved: { requestId: string; decision: string }[] = [];
    const transport = transportFor(
      [
        at(1, { turnId: "t1", type: "userMessage", text: "clean up" }),
        at(2, {
          turnId: "t1",
          type: "approvalRequest",
          requestId: "req-1",
          kind: "command",
          summary: "APPROVAL_SUMMARY_MARKER",
          answered: false,
        }),
      ],
      approved,
    );
    render(<StructuredChatPane transport={transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("APPROVAL_SUMMARY_MARKER")).toBeInTheDocument());
    // the card is the reference prompt card: pick the decision, then send it
    fireEvent.click(screen.getByTestId("reference-prompt-option-0"));
    fireEvent.click(screen.getByTestId("reference-prompt-submit"));
    await waitFor(() => expect(approved).toEqual([{ requestId: "req-1", decision: "accept" }]));
  });

  it("routes a composer send through the lane and clears the box", async () => {
    const calls: string[] = [];
    const transport = (async (request: { op: StructuredChatOp; body: unknown }) => {
      calls.push(request.op);
      if (request.op === "listSessions") return { ok: true, data: [SUMMARY] };
      if (request.op === "capabilities") return { ok: true, data: CAPABILITIES };
      if (request.op === "history" || request.op === "events") {
        return { ok: true, data: { parts: [], nextSeq: 0 } };
      }
      return { ok: true, data: 1 };
    }) as unknown as StructuredChatTransport;
    render(<StructuredChatPane transport={transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "SEND_MARKER" } });
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter" });
    await waitFor(() => expect(calls).toContain("turn"));
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(""));
  });

  it("shows the lane's warning instead of a silent empty composer when the host refuses", async () => {
    const transport = (async (request: { op: StructuredChatOp }) => {
      if (request.op === "createSession") {
        return { ok: false, code: "INVALID_REQUEST", message: "no such cwd" };
      }
      return { ok: true, data: { parts: [], nextSeq: 0 } };
    }) as unknown as StructuredChatTransport;
    render(
      <StructuredChatPane
        transport={transport}
        create={{ provider: "codex", cwd: "/nope" }}
        pollMs={0}
      />,
    );
    await waitFor(() => expect(screen.getByText(/no such cwd/)).toBeInTheDocument());
    expect(screen.getByTestId("chat-composer-textarea")).toBeDisabled();
  });

  it("says a session ended, and does not keep a banner the lane can never clear", async () => {
    const ended = { ...SUMMARY, state: "ended" as const };
    const transport = (async (request: { op: StructuredChatOp }) => {
      if (request.op === "listSessions") return { ok: true, data: [ended] };
      if (request.op === "capabilities") return { ok: true, data: CAPABILITIES };
      if (request.op === "history" || request.op === "events") {
        return { ok: true, data: { parts: [], nextSeq: 0 } };
      }
      return { ok: true, data: 1 };
    }) as unknown as StructuredChatTransport;
    render(<StructuredChatPane transport={transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText(STRUCTURED_CHAT_SESSION_ENDED)).toBeInTheDocument());
    // The unsupported file-revert notice is a lane field, not a warning: it never clears.
    expect(screen.queryByText(STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED)).toBeNull();
  });
});
