/**
 * Gap lane chat-gaps-20261126: the structured verbs the workspace already had props for, now
 * wired from `StructuredChatPane` to `useStructuredChat`.
 *
 * Each case presses the control the user sees and asserts the wire call it must produce, so the
 * promise is t3code's (composer.md:191-194 rewind, 283-293 queue/Resume, 308-315 commands) rather
 * than the lane's own internal shape.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { StructuredChatPane } from "./StructuredChatPane";
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
  models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low"] }],
  slashCommands: ["/compact"],
};

const SUMMARY: StructuredSessionSummary = {
  sessionId: "s1",
  provider: "codex",
  cwd: "/repo",
  state: "running",
  settings: { model: "gpt-5" },
};

interface FakeOptions {
  readonly parts?: readonly StructuredPart[];
  readonly capabilities?: StructuredCapabilities;
  readonly summary?: StructuredSessionSummary;
  /** The branch list `tree` answers with; an empty one by default. */
  readonly tree?: { readonly currentLeafId?: string; readonly nodes: readonly Record<string, unknown>[] };
  /** The agent's own command list, as `decodeStructuredCommands` would hand it over. */
  readonly commands?: readonly { readonly name: string; readonly source: string; readonly syntax: string; readonly description?: string }[];
}

/**
 * A transport over one in-memory journal, with the gateway's own read shape: `events` is a tail
 * read (`after`), `history` is a window (`before`). Every call is recorded with its body.
 */
function fakeTransport(options: FakeOptions = {}) {
  const calls: { op: StructuredChatOp; body: unknown }[] = [];
  let parts = [...(options.parts ?? [])];
  const capabilities = options.capabilities ?? CAPABILITIES;
  const summary = options.summary ?? SUMMARY;
  let seq = parts.reduce((highest, part) => Math.max(highest, part.seq), 0);
  const append = (extra: Record<string, unknown>) => {
    seq += 1;
    parts = [...parts, at(seq, extra)];
    return seq;
  };

  const transport = (async (request: { op: StructuredChatOp; body: unknown }) => {
    calls.push({ op: request.op, body: request.body });
    switch (request.op) {
      case "listSessions":
        return { ok: true, data: [summary] };
      case "capabilities":
        return { ok: true, data: capabilities };
      case "history": {
        const before = (request.body as { before?: number }).before;
        const window = before === undefined ? parts : parts.filter((part) => part.seq < before);
        return { ok: true, data: { parts: window, nextSeq: seq } };
      }
      case "events": {
        const after = (request.body as { after?: number }).after ?? 0;
        return { ok: true, data: { parts: parts.filter((part) => part.seq > after), nextSeq: seq } };
      }
      case "turn":
        append({ type: "userMessage", text: (request.body as { text: string }).text });
        return { ok: true, data: seq };
      case "interrupt":
        // A stop settles the open turn on the provider side, which is what the next tail read
        // shows; the store clears `activeTurnId` with it.
        append({ type: "assistantMessage", text: "stopped" });
        return { ok: true, data: undefined };
      case "rollback": {
        const turns = (request.body as { turns: number }).turns;
        const groups = [
          ...new Set(parts.map((part) => part.turnId).filter((id): id is string => id !== undefined)),
        ];
        const drop = new Set(groups.slice(-turns));
        parts = parts.filter((part) => part.turnId === undefined || !drop.has(part.turnId));
        return { ok: true, data: undefined };
      }
      case "clear":
        append({ type: "newSession" });
        return { ok: true, data: seq };
      case "compact":
        return { ok: true, data: undefined };
      case "tree":
        return { ok: true, data: options.tree ?? { nodes: [] } };
      case "commands":
        // The decoded list, which is what the real transport hands the lane after `decodeStructuredCommands`.
        return { ok: true, data: options.commands ?? [] };
      case "settings":
        // A gateway echoes the settings it applied; the lane reads that reply as the new state.
        return { ok: true, data: { ...summary.settings, ...(request.body as Record<string, unknown>) } };
      case "stats":
        return {
          ok: true,
          data: { turns: 2, totalTokens: 120, inputTokens: 80, outputTokens: 40 },
        };
      case "media":
        // One PNG byte, enough to build the data URL the view draws.
        return { ok: true, data: { mimeType: "image/png", dataBase64: "iVBORw0KGgo=" } };
      default:
        return { ok: true, data: undefined };
    }
  }) as unknown as StructuredChatTransport;

  return { transport, calls };
}

/** A settled turn: the tail is an assistant message, so nothing reads as in flight. */
const SETTLED: readonly StructuredPart[] = [
  at(1, { turnId: "t1", type: "userMessage", text: "first prompt" }),
  at(2, { turnId: "t1", type: "assistantMessage", text: "reply" }),
];

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

describe("structured lane rewind", () => {
  it("offers Undo, and reverts the conversation without touching files by default", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-undo-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-undo-button"));
    fireEvent.click(screen.getByTestId("structured-undo-keep-changes"));

    // composer.md:192: "Revert and keep changes" leaves workspace files as they are, so no
    // revertFiles bit rides the request.
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "rollback").map((call) => call.body)).toEqual([
        { turns: 1 },
      ]),
    );
  });

  it('offers "Revert files too" only when the agent advertises revertFiles, and sends the bit', async () => {
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, revertFiles: true },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-undo-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-undo-button"));
    await waitFor(() => expect(screen.getByTestId("structured-undo-revert-files")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-undo-revert-files"));

    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "rollback").map((call) => call.body)).toEqual([
        { turns: 1, revertFiles: true },
      ]),
    );
  });

  it('hides "Revert files too" when the agent does not advertise it', async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-undo-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-undo-button"));
    await waitFor(() => expect(screen.getByTestId("structured-undo-keep-changes")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-undo-revert-files")).toBeNull();
  });

  it("draws no rewind affordance at all for an agent that cannot roll back", async () => {
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, rollback: false },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-undo-button")).toBeNull();
    // The message's own "Edit from here" is a rewind too, so it goes with the capability.
    expect(screen.queryByTestId("message-edit-from-here")).toBeNull();
  });

  it("rewinds from an earlier user message and puts its text back in the composer", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    // composer.md:191-192: "Edit from here" beneath a sent message rewinds to before it and the
    // selected prompt returns to the composer for editing and resending.
    await waitFor(() => expect(screen.getByTestId("message-edit-from-here")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("message-edit-from-here"));

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("first prompt"));
    expect(fake.calls.some((call) => call.op === "rollback")).toBe(true);
  });
});

describe("structured lane commands", () => {
  it("offers /clear and /compact as visible controls and runs them", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-clear-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-clear-button"));
    await waitFor(() => expect(fake.calls.some((call) => call.op === "clear")).toBe(true));

    fireEvent.click(screen.getByTestId("structured-compact-button"));
    await waitFor(() => expect(fake.calls.some((call) => call.op === "compact")).toBe(true));
  });

  it("draws no Compact control for an agent that does not advertise the command", async () => {
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, slashCommands: [] },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-clear-button")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-compact-button")).toBeNull();
  });
});

describe("structured lane queue", () => {
  /** An agent that queues instead of steering: the only shape a stop can hold a row in. */
  const QUEUEING: StructuredCapabilities = { ...CAPABILITIES, steer: false };
  const RUNNING: readonly StructuredPart[] = [
    at(1, { turnId: "t1", type: "userMessage", text: "go" }),
    at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
  ];

  it("offers Resume on the composer warning after an interrupt holds the queue", async () => {
    // composer.md:282-284: a stop keeps queued messages in order and held; Resume continues.
    const fake = fakeTransport({ parts: RUNNING, capabilities: QUEUEING });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "queued one" } });
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter" });
    await waitFor(() => expect(screen.getByTestId("queued-follow-up-list")).toBeInTheDocument());

    // A stop holds the queue rather than letting it drain.
    fireEvent.click(screen.getByTestId("stop-button"));
    await waitFor(() => expect(fake.calls.some((call) => call.op === "interrupt")).toBe(true));

    const resume = await screen.findByTestId("chat-composer-warning-action");
    expect(resume).toHaveTextContent("Resume");
    // Held, not delivered: a stop must not turn the queued row into the next turn.
    expect(fake.calls.some((call) => call.op === "turn")).toBe(false);

    fireEvent.click(resume);
    // Resume releases the hold, so the queued row is delivered as the next turn.
    await waitFor(() =>
      expect(
        fake.calls.filter((call) => call.op === "turn").map((call) => (call.body as { text: string }).text),
      ).toEqual(["queued one"]),
    );
  });

  it("saves an edited queued row in place instead of sending a new one", async () => {
    // composer.md:289-293: the pencil opens the row in the composer and the checkmark saves it
    // in place.
    const fake = fakeTransport({ parts: RUNNING, capabilities: QUEUEING });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "before edit" } });
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter" });
    await waitFor(() => expect(screen.getByText("before edit")).toBeInTheDocument());

    fireEvent.click(screen.getByLabelText(/^Edit queued message/));
    // The row's own text moves into the composer for editing.
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("before edit"));
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "after edit" } });
    fireEvent.click(screen.getByLabelText(/^Save queued message/));

    await waitFor(() => expect(screen.getByText("after edit")).toBeInTheDocument());
    expect(screen.queryByText("before edit")).toBeNull();
    // Editing is not sending: the row stays queued behind the running turn.
    expect(fake.calls.some((call) => call.op === "turn")).toBe(false);
  });
});

describe("structured lane history paging", () => {
  it("loads an older page and prepends it", async () => {
    const parts = [
      at(1, { turnId: "t1", type: "userMessage", text: "OLDEST_MARKER" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "old reply" }),
      at(3, { turnId: "t2", type: "userMessage", text: "NEWEST_MARKER" }),
      at(4, { turnId: "t2", type: "assistantMessage", text: "new reply" }),
    ];
    const calls: { op: StructuredChatOp; body: unknown }[] = [];
    const transport = (async (request: { op: StructuredChatOp; body: unknown }) => {
      calls.push({ op: request.op, body: request.body });
      switch (request.op) {
        case "listSessions":
          return { ok: true, data: [SUMMARY] };
        case "capabilities":
          return { ok: true, data: CAPABILITIES };
        case "history": {
          const before = (request.body as { before?: number }).before;
          if (before === undefined) {
            // The newest window only, with the cursor that walks back to it.
            return { ok: true, data: { parts: parts.slice(2), nextSeq: 4, hasOlder: true, nextBefore: 3 } };
          }
          return {
            ok: true,
            data: { parts: parts.filter((part) => part.seq < before), nextSeq: 2, hasOlder: false },
          };
        }
        case "events":
          return { ok: true, data: { parts: [], nextSeq: 4 } };
        default:
          return { ok: true, data: undefined };
      }
    }) as unknown as StructuredChatTransport;

    render(<StructuredChatPane transport={transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("NEWEST_MARKER")).toBeInTheDocument());
    expect(screen.queryByText("OLDEST_MARKER")).toBeNull();

    // t3code's transcript prepends older turns and offers the control that fetches them.
    fireEvent.click(await screen.findByTestId("reference-older-button"));
    await waitFor(() => expect(screen.getByText("OLDEST_MARKER")).toBeInTheDocument());
    expect(calls.some((call) => call.op === "history" && (call.body as { before?: number }).before === 3)).toBe(
      true,
    );
    // The end of the conversation's start is drawn once there is nothing older left.
    await waitFor(() => expect(screen.getByTestId("reference-older-endcap")).toBeInTheDocument());
  });
});

describe("structured lane turn-in-flight", () => {
  it("shows the running state from SessionSummary.activeTurnId even when the journal reads settled", async () => {
    // The gateway names a turn the stream has not written a part for yet, so the summary is the
    // only thing that knows the turn exists (chat-t3-parity plan, gap item 4).
    const fake = fakeTransport({
      parts: SETTLED,
      summary: { ...SUMMARY, activeTurnId: "t2" },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("stop-button")).toBeInTheDocument());
  });

  it("stays idle when the summary reports no active turn and the journal ends settled", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("reply")).toBeInTheDocument());
    expect(screen.queryByTestId("stop-button")).toBeNull();
  });
});

describe("structured lane extension UI", () => {
  const ASKING: readonly StructuredPart[] = [
    at(1, { turnId: "t1", type: "userMessage", text: "deploy" }),
    at(2, {
      turnId: "t1",
      type: "extensionUiRequest",
      requestId: "ext-1",
      method: "select",
      title: "Which target?",
      options: ["staging", "production"],
      answered: false,
    }),
  ];

  it("draws the open request as a card and answers it by id", async () => {
    const fake = fakeTransport({ parts: ASKING, capabilities: { ...CAPABILITIES, extensionUi: true } });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("extension-ui-card")).toBeInTheDocument());
    expect(screen.getByTestId("extension-ui-title")).toHaveTextContent("Which target?");
    fireEvent.click(screen.getByTestId("extension-ui-option-1"));
    fireEvent.click(screen.getByTestId("extension-ui-submit"));

    await waitFor(() =>
      expect(
        fake.calls.filter((call) => call.op === "answerExtensionUi").map((call) => call.body),
      ).toEqual([{ requestId: "ext-1", value: "production" }]),
    );
  });

  it("does not leave the transcript asking the same question the card asks", async () => {
    const fake = fakeTransport({ parts: ASKING, capabilities: { ...CAPABILITIES, extensionUi: true } });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("extension-ui-card")).toBeInTheDocument());
    // One card, and no second copy of the question in the stream behind it.
    expect(screen.getAllByTestId("extension-ui-card")).toHaveLength(1);
    expect(screen.queryByText("Which target? (answered)")).toBeNull();
  });

  it("keeps an answered request as the record of what the agent asked", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "deploy" }),
        at(2, {
          turnId: "t1",
          type: "extensionUiRequest",
          requestId: "ext-1",
          method: "select",
          title: "Which target?",
          options: ["staging"],
          answered: true,
        }),
        at(3, { turnId: "t1", type: "assistantMessage", text: "deployed to staging" }),
      ],
      capabilities: { ...CAPABILITIES, extensionUi: true },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("deployed to staging")).toBeInTheDocument());
    expect(screen.queryByTestId("extension-ui-card")).toBeNull();
    // The notice draws its text in the summary and again in the expanded body, so both nodes
    // carry it; what matters is that the record is there and the card is gone.
    expect(screen.getAllByText("Which target? (answered)").length).toBeGreaterThan(0);
  });
});

describe("structured lane side channel", () => {
  it("draws statuses, widgets, subagents and goals above the transcript", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "extensionStatus", statusKey: "websearch", statusText: "Searching the web" }),
        at(3, { turnId: "t1", type: "extensionWidget", widgetKey: "todo", lines: ["write the test"] }),
        at(4, { turnId: "t1", type: "subagentProgress", taskId: "task-1", name: "auditor", status: "inProgress" }),
        at(5, { turnId: "t1", type: "goalUpdate", goalId: "g1", title: "Ship the lane", status: "inProgress", progress: "2/5" }),
        at(6, { turnId: "t1", type: "assistantMessage", text: "done" }),
      ],
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("extension-status-websearch")).toHaveTextContent("Searching the web"));
    expect(screen.getByTestId("extension-widget-todo")).toHaveTextContent("write the test");
    expect(screen.getByTestId("extension-subagent-task-1")).toHaveTextContent("auditor");
    expect(screen.getByTestId("extension-goal-g1")).toHaveTextContent("Ship the lane");
    // The side channel is not a transcript row.
    expect(screen.queryByText("Searching the web", { selector: "[data-testid^='chat-message']" })).toBeNull();
  });

  it("draws no strip at all for a session whose extension never speaks", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("reply")).toBeInTheDocument());
    expect(screen.queryByTestId("extension-status-strip")).toBeNull();
  });

  it("draws a rollback divider where the conversation was taken back", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "first" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "first reply" }),
        at(3, { type: "rollback", turns: 1 }),
      ],
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    // The notice draws its text in the summary and again in the body, so the divider is found by
    // its testid rather than by a single text node.
    await waitFor(() => expect(screen.getByTestId("reference-notice")).toBeInTheDocument());
    expect(screen.getAllByText("Rolled back 1 turn").length).toBeGreaterThan(0);
  });
});

describe("structured lane media", () => {
  it("reads a referenced picture once and draws it above the transcript", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "screenshot it" }),
        at(2, { turnId: "t1", type: "mediaRef", toolCallId: "call-1", contentIndex: 0, mimeType: "image/png", alt: "the page" }),
        at(3, { turnId: "t1", type: "assistantMessage", text: "here it is" }),
      ],
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-media-strip")).toBeInTheDocument());
    await waitFor(() => expect(screen.getByTestId("structured-media-thumbnail")).toHaveAttribute("alt", "the page"));
    expect(
      fake.calls.filter((call) => call.op === "media").map((call) => call.body),
    ).toEqual([{ toolCallId: "call-1", contentIndex: 0 }]);
  });

  it("draws no media strip for a stream that refers to none", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByText("reply")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-media-strip")).toBeNull();
  });
});

describe("structured lane branch drawer", () => {
  const TREE = {
    currentLeafId: "n2",
    nodes: [
      { id: "n1", role: "user", preview: "first prompt", active: false },
      { id: "n2", parentId: "n1", role: "assistant", preview: "reply", active: true },
    ],
  };

  it("opens from the lane's own control and reads the tree", async () => {
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, treeNavigation: true },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-tree-button")).toBeInTheDocument());
    expect(screen.queryByTestId("session-tree-drawer")).toBeNull();
    fireEvent.click(screen.getByTestId("structured-tree-button"));
    await waitFor(() => expect(screen.getByTestId("session-tree-drawer")).toBeInTheDocument());
    await waitFor(() => expect(fake.calls.some((call) => call.op === "tree")).toBe(true));
  });

  it("draws no branch control for an agent that exposes no tree", async () => {
    const fake = fakeTransport({ parts: SETTLED });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-clear-button")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-tree-button")).toBeNull();
  });

  it("navigates to a branch from the drawer and re-reads it", async () => {
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, treeNavigation: true },
      tree: TREE,
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-tree-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("structured-tree-button"));
    await waitFor(() => expect(screen.getByTestId("session-tree-node-n1")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("session-tree-go-n1"));

    await waitFor(() =>
      expect(
        fake.calls.filter((call) => call.op === "navigateTree").map((call) => call.body),
      ).toEqual([{ targetId: "n1" }]),
    );
  });
});

describe("structured lane pickers the composer contract has no seam for", () => {
  const RICH: StructuredCapabilities = {
    ...CAPABILITIES,
    executionModes: ["read-only", "workspace-write"],
    agents: ["default", "reviewer"],
    thinkingLevels: ["low", "high"],
    fastMode: true,
    additionalDirs: true,
    queueModes: true,
  };

  it("draws each advertised picker and writes the settings key it names", async () => {
    const fake = fakeTransport({ parts: SETTLED, capabilities: RICH });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-extra-controls")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("structured-control-executionMode"), {
      target: { value: "workspace-write" },
    });
    await waitFor(() =>
      expect(
        fake.calls.filter((call) => call.op === "settings").map((call) => call.body),
      ).toContainEqual({ executionMode: "workspace-write" }),
    );

    fireEvent.change(screen.getByTestId("structured-control-agentName"), { target: { value: "reviewer" } });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "settings").map((call) => call.body)).toContainEqual({
        agentName: "reviewer",
      }),
    );

    fireEvent.change(screen.getByTestId("structured-control-effort"), { target: { value: "high" } });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "settings").map((call) => call.body)).toContainEqual({
        effort: "high",
      }),
    );

    fireEvent.click(screen.getByTestId("structured-control-fastMode"));
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "settings").map((call) => call.body)).toContainEqual({
        fastMode: true,
      }),
    );

    fireEvent.change(screen.getByTestId("structured-control-steeringMode"), {
      target: { value: "one-at-a-time" },
    });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "settings").map((call) => call.body)).toContainEqual({
        steeringMode: "one-at-a-time",
      }),
    );
  });

  it("adds and removes an extra directory as the settings list it is", async () => {
    const fake = fakeTransport({ parts: SETTLED, capabilities: RICH });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("structured-dirs-editor")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("structured-dir-input"), { target: { value: "/extra/root" } });
    fireEvent.submit(screen.getByTestId("structured-dirs-form"));

    await waitFor(() =>
      expect(
        fake.calls.filter((call) => call.op === "settings").map((call) => call.body),
      ).toContainEqual({ additionalDirs: ["/extra/root"] }),
    );
    // The input clears so the next root is a fresh entry, not an edit of the last one.
    expect(screen.getByTestId("structured-dir-input")).toHaveValue("");
  });

  it("draws no picker at all for an agent that advertises none of them", async () => {
    // An agent with no execution modes, agents, fast mode, extra roots, queue modes and no models:
    // every control this row can draw is `null`, so the row itself is not drawn. (A model that
    // lists efforts still yields a thinking picker, which is why this fixture lists none.)
    const fake = fakeTransport({
      parts: SETTLED,
      capabilities: { ...CAPABILITIES, models: [] },
    });
    render(<StructuredChatPane transport={fake.transport} sessionId="s1" pollMs={0} />);

    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument());
    expect(screen.queryByTestId("structured-extra-controls")).toBeNull();
    expect(screen.queryByTestId("structured-dirs-editor")).toBeNull();
  });
});
