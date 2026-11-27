import { afterEach, describe, expect, it, vi, type Mock } from "vitest";
import { StrictMode, createElement } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DesktopChatPane } from "./DesktopChatPane";
import {
  desktopChatTransportFrom,
  type DesktopChatHandlers,
  type DesktopChatPromptRead,
  type DesktopChatResult,
} from "../lib/desktopChat";
import { DESKTOP_CHAT_FILE_CAPABILITY, type DesktopChatFileStaging } from "../lib/desktopChatLane";
import { referenceSha256Hex } from "../remote/chat/referenceFiles";
import type { ReferenceHistoryPage, ReferencePrompt } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "../lib/types";

// Capture the workspace props each mount receives, then render the real component, so the
// lane-to-workspace seams (sentPrompts, attachments) are observable without changing behaviour.
const capturedWorkspaceProps = vi.hoisted(() => {
  const calls: Record<string, unknown>[] = [];
  return { calls };
});
vi.mock("../remote/chat/MobileChatWorkspace", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../remote/chat/MobileChatWorkspace")>();
  return {
    ...actual,
    MobileChatWorkspace: (props: Record<string, unknown>) => {
      capturedWorkspaceProps.calls.push(props);
      return createElement(actual.MobileChatWorkspace, props as unknown as Parameters<typeof actual.MobileChatWorkspace>[0]);
    },
  };
});

// The prompt card's answer-file seam is a prop the card forwards to the lane; recording the
// props keeps the wiring observable without depending on the card's own attach UI (lane G6).
const capturedPromptProps = vi.hoisted(() => {
  const calls: Record<string, unknown>[] = [];
  return { calls };
});
vi.mock("../remote/chat/ReferencePromptCard", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../remote/chat/ReferencePromptCard")>();
  return {
    ...actual,
    ReferencePromptCard: (props: Record<string, unknown>) => {
      capturedPromptProps.calls.push(props);
      return createElement(actual.ReferencePromptCard, props as unknown as Parameters<typeof actual.ReferencePromptCard>[0]);
    },
  };
});

window.HTMLElement.prototype.scrollIntoView = vi.fn();

let sequence = 0;

function session(overrides: Partial<TerminalSession> = {}): TerminalSession {
  sequence += 1;
  return {
    id: `pane-${sequence}`,
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId: `be-${sequence}`,
    lifecycle: "running",
    ownerId: "owner",
    daemonEpoch: "1",
    agentType: "claude",
    ...overrides,
  };
}

function page(texts: readonly string[] = []): ReferenceHistoryPage {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: texts.map((text, index) => ({ role: "user" as const, id: `t${index}`, parts: [{ kind: "text" as const, text }] })),
    hasMore: false,
    generation: "g1",
  };
}

const ok = <T,>(data: T): DesktopChatResult<T> => ({ ok: true, data });
const noPrompt: DesktopChatPromptRead = { prompt: null, screenRevision: null };

type MockHandlers = { [K in keyof DesktopChatHandlers]: Mock<DesktopChatHandlers[K]> };

function handlers(overrides: Partial<MockHandlers> = {}): MockHandlers {
  return {
    history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(page())),
    prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok(noPrompt)),
    submit: vi.fn<DesktopChatHandlers["submit"]>(async () => ok(null)),
    stop: vi.fn<DesktopChatHandlers["stop"]>(async () => ok(null)),
    answer: vi.fn<DesktopChatHandlers["answer"]>(async () => ok(null)),
    resolvePrompt: vi.fn<DesktopChatHandlers["resolvePrompt"]>(async () => ok(null)),
    ...overrides,
  };
}

function mount(s: TerminalSession, h = handlers(), onViewTerminal = vi.fn()) {
  const transport = desktopChatTransportFrom(h);
  capturedWorkspaceProps.calls.length = 0;
  capturedPromptProps.calls.length = 0;
  const view = render(
    <DesktopChatPane session={s} visible onViewTerminal={onViewTerminal} transport={transport} identity={{ hostId: "local" }} pollMs={0} />,
  );
  return { view, h, transport, onViewTerminal };
}

function type(text: string) {
  fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: text } });
  fireEvent.click(screen.getByTestId("send-button"));
}

describe("DesktopChatPane", () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("reads history for the pane's own target and switches to the terminal on request", async () => {
    const s = session();
    const h = handlers({ history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(page(["HISTORY_MARKER"]))) });
    const { onViewTerminal } = mount(s, h);
    expect(await screen.findByText("HISTORY_MARKER")).toBeInTheDocument();
    expect(h.history.mock.calls[0][0].target.target).toEqual({
      hostId: "local",
      ownerId: "owner",
      epoch: "1",
      backendSessionId: s.backendSessionId,
    });
    fireEvent.click(screen.getByTestId("open-terminal-button"));
    expect(onViewTerminal).toHaveBeenCalledTimes(1);
  });

  it("reads with a live signal after StrictMode replays effects", async () => {
    const h = handlers({ history: vi.fn<DesktopChatHandlers["history"]>(async (_request, signal) => {
      signal.throwIfAborted();
      return ok(page(["STRICT_HISTORY"]));
    }) });
    render(
      <StrictMode>
        <DesktopChatPane session={session()} visible onViewTerminal={vi.fn()}
          transport={desktopChatTransportFrom(h)} identity={{ hostId: "local" }} pollMs={0} />
      </StrictMode>,
    );
    expect(await screen.findByText("STRICT_HISTORY")).toBeInTheDocument();
  });

  it("submits the trimmed text as a chat-origin payload", async () => {
    const { h } = mount(session());
    await waitFor(() => expect(h.history).toHaveBeenCalled());
    type("  run the tests  ");
    await waitFor(() => expect(h.submit).toHaveBeenCalledTimes(1));
    expect(h.submit.mock.calls[0][0].params).toEqual({ text: "run the tests", attachmentIds: [], origin: "chat" });
    expect(h.submit.mock.calls[0][0].registryId).toBe("claude");
  });

  it("puts a refused message back in the box and says so", async () => {
    const h = handlers({
      submit: vi.fn<DesktopChatHandlers["submit"]>(async () => ({ ok: false, code: "INTERNAL", message: "no" })),
    });
    mount(session(), h);
    await waitFor(() => expect(h.history).toHaveBeenCalled());
    type("REFUSED_DRAFT");
    await waitFor(() =>
      expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toBe("REFUSED_DRAFT"),
    );
    expect(screen.getByTestId("chat-history-warning")).toHaveTextContent("not delivered");
  });

  it("keeps the draft when the pane is unmounted for terminal mode and mounted again", async () => {
    const s = session();
    const first = mount(s);
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "KEPT_DRAFT" } });
    first.view.unmount();
    mount(s);
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toBe("KEPT_DRAFT");
  });

  it("stops a working agent with its provider interrupt, and refuses where the agent is unknown", async () => {
    const working = mount(session({ lifecycle: "working" }));
    fireEvent.click(await screen.findByTestId("stop-button"));
    await waitFor(() => expect(working.h.stop).toHaveBeenCalledTimes(1));
    expect(working.h.stop.mock.calls[0][0].params).toEqual({ capability: "providerInterrupt" });
    cleanup();

    const shell = mount(session({ lifecycle: "working", agentType: null }));
    fireEvent.click(await screen.findByTestId("stop-button"));
    expect(await screen.findByTestId("chat-history-warning")).toHaveTextContent("Stop isn't available");
    expect(shell.h.stop).not.toHaveBeenCalled();
  });

  it("answers the prompt bound to its screen revision", async () => {
    const prompt: ReferencePrompt = {
      id: "p1",
      agent: "claude",
      kind: "question",
      title: "Question",
      question: "Which model?",
      options: [{ label: "A" }, { label: "B" }],
      multiSelect: false,
    };
    const h = handlers({ prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok({ prompt, screenRevision: "rev-3" })) });
    mount(session(), h);
    fireEvent.click(await screen.findByTestId("reference-prompt-option-1"), { detail: 1 });
    fireEvent.click(screen.getByTestId("reference-prompt-submit"), { detail: 1 });
    await waitFor(() => expect(h.answer).toHaveBeenCalledTimes(1));
    expect(h.answer.mock.calls[0][0].params).toEqual({ promptId: "p1", screenRevision: "rev-3", answer: { optionIndex: 1 } });
  });

  it("shows an exited session read-only and never asks for its prompt", async () => {
    const h = handlers({ history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(page(["OLD_TURN"]))) });
    mount(session({ lifecycle: "exited" }), h);
    expect(await screen.findByText("OLD_TURN")).toBeInTheDocument();
    expect(screen.getByTestId("desktop-chat-pane")).toHaveAttribute("data-exited", "true");
    expect(screen.getByTestId("chat-composer-textarea")).toBeDisabled();
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("has exited");
    expect(h.prompt).not.toHaveBeenCalled();
  });

  it("aborts a read for the previous target and never shows its answer", async () => {
    let release!: (value: DesktopChatResult<ReferenceHistoryPage>) => void;
    const signals: AbortSignal[] = [];
    const h = handlers({
      history: vi.fn<DesktopChatHandlers["history"]>(async (request, signal) => {
        signals.push(signal);
        if (request.target.target.backendSessionId === "be-old") {
          return new Promise<DesktopChatResult<ReferenceHistoryPage>>((resolve) => {
            release = resolve;
          });
        }
        return ok(page(["NEW_TARGET_TURN"]));
      }),
    });
    const transport = desktopChatTransportFrom(h);
    const props = { visible: true, onViewTerminal: vi.fn(), transport, identity: { hostId: "local" }, pollMs: 0 };
    const { rerender } = render(<DesktopChatPane {...props} session={session({ backendSessionId: "be-old" })} />);
    await waitFor(() => expect(signals).toHaveLength(1));
    rerender(<DesktopChatPane {...props} session={session({ backendSessionId: "be-new" })} />);
    expect(await screen.findByText("NEW_TARGET_TURN")).toBeInTheDocument();
    expect(signals[0].aborted).toBe(true);
    release(ok(page(["STALE_TURN"])));
    await waitFor(() => expect(screen.getByText("NEW_TARGET_TURN")).toBeInTheDocument());
    expect(screen.queryByText("STALE_TURN")).not.toBeInTheDocument();
  });

  it("omits the attachment control when no file staging transport is supplied, and renders it when supplied", async () => {
    const h = handlers();
    const transport = desktopChatTransportFrom(h);
    const s = session();
    const { rerender } = render(
      <DesktopChatPane session={s} visible onViewTerminal={vi.fn()} transport={transport} identity={{ hostId: "local" }} pollMs={0} />,
    );
    await waitFor(() => expect(h.history).toHaveBeenCalled());
    expect(screen.queryByTestId("attach-file-button")).not.toBeInTheDocument();

    const fileStaging: DesktopChatFileStaging = {
      capabilities: [DESKTOP_CHAT_FILE_CAPABILITY],
      transport: {
        stage: vi.fn(async () => ({
          receipt: { hostId: "local", attachmentId: "att-1", sha256: "0".repeat(64), sizeBytes: 1, mediaType: "text/plain" },
          displayName: "a.txt",
          mentionText: "@a.txt ",
        })),
        cancel: vi.fn(async () => true),
        remove: vi.fn(async () => true),
      },
    };
    rerender(
      <DesktopChatPane session={s} visible onViewTerminal={vi.fn()} transport={transport} identity={{ hostId: "local" }} pollMs={0} fileStaging={fileStaging} />,
    );
    expect(screen.getByTestId("attach-file-button")).toBeInTheDocument();
  });

  it("derives sentPrompts from the lane's own user turns, deduplicated in order", async () => {
    const mixed: ReferenceHistoryPage = {
      source: "claude-transcript",
      availability: "native",
      turns: [
        { role: "user", id: "t0", parts: [{ kind: "text", text: "run the suite" }] },
        { role: "assistant", id: "t1", parts: [{ kind: "text", text: "suite failed" }] },
        { role: "user", id: "t2", parts: [{ kind: "text", text: "run the suite" }] },
        { role: "user", id: "t3", parts: [{ kind: "text", text: "fix it then" }] },
      ],
      hasMore: false,
      generation: "g1",
    };
    mount(session(), handlers({ history: vi.fn<DesktopChatHandlers["history"]>(async () => ok(mixed)) }));
    await screen.findByText("suite failed");
    const latest = capturedWorkspaceProps.calls.at(-1) ?? {};
    expect(latest.sentPrompts).toEqual(["run the suite", "fix it then"]);
  });

  it("gives the prompt card an answer-file seam that stages on the pane's host and hands back receipts", async () => {
    const prompt: ReferencePrompt = {
      id: "p-files",
      agent: "claude",
      kind: "question",
      title: "Question",
      question: "Attach the failing log",
      options: [{ label: "A" }],
      multiSelect: false,
    };
    const stage = vi.fn<DesktopChatFileStaging["transport"]["stage"]>(async (target, payload) => ({
      receipt: {
        hostId: target.hostId,
        attachmentId: "att-answer",
        sha256: await referenceSha256Hex(new TextEncoder().encode("log")),
        sizeBytes: payload.sizeBytes,
        mediaType: payload.mediaType,
      },
      displayName: payload.name,
      mentionText: `@${payload.name} `,
    }));
    const fileStaging: DesktopChatFileStaging = {
      capabilities: [DESKTOP_CHAT_FILE_CAPABILITY],
      transport: { stage, cancel: vi.fn(async () => true), remove: vi.fn(async () => true) },
    };
    const s = session();
    const h = handlers({ prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok({ prompt, screenRevision: "rev-1" })) });
    render(
      <DesktopChatPane session={s} visible onViewTerminal={vi.fn()} transport={desktopChatTransportFrom(h)}
        identity={{ hostId: "local" }} pollMs={0} fileStaging={fileStaging} />,
    );
    await waitFor(() => expect(capturedPromptProps.calls.length).toBeGreaterThan(0));
    const attach = capturedPromptProps.calls.at(-1)?.onAttachAnswerFiles as
      | ((files: readonly File[]) => Promise<readonly { mentionText: string }[]>)
      | undefined;
    expect(attach).toBeTypeOf("function");

    const bytes = new TextEncoder().encode("log");
    const file = new File([bytes], "fail.log", { type: "text/plain" });
    Object.defineProperty(file, "arrayBuffer", { value: async () => bytes.slice().buffer });
    const receipts = await attach!([file]);
    // The card's answer text carries this mention: the staged file's own relative path.
    expect(receipts.map((each) => each.mentionText)).toEqual(["@fail.log "]);
    expect(stage).toHaveBeenCalledTimes(1);
    expect(stage.mock.calls[0]?.[0]).toMatchObject({ backendSessionId: s.backendSessionId });
  });

  it("adds no answer-file seam where the pane has no staging route", async () => {
    const prompt: ReferencePrompt = {
      id: "p-no-files",
      agent: "claude",
      kind: "question",
      title: "Question",
      question: "Which model?",
      options: [{ label: "A" }],
      multiSelect: false,
    };
    mount(session(), handlers({ prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok({ prompt, screenRevision: "rev-1" })) }));
    await waitFor(() => expect(capturedPromptProps.calls.length).toBeGreaterThan(0));
    expect(capturedPromptProps.calls.at(-1)).not.toHaveProperty("onAttachAnswerFiles");
  });

  it("scopes the answer set to the pane's own chat target key", async () => {
    const prompt: ReferencePrompt = {
      id: "p-scope",
      agent: "claude",
      kind: "question",
      title: "Question",
      question: "Which model?",
      options: [{ label: "A" }],
      multiSelect: false,
    };
    const fileStaging: DesktopChatFileStaging = {
      capabilities: [DESKTOP_CHAT_FILE_CAPABILITY],
      transport: { stage: vi.fn(), cancel: vi.fn(async () => true), remove: vi.fn(async () => true) },
    };
    const s = session();
    capturedPromptProps.calls.length = 0;
    const transport = desktopChatTransportFrom(
      handlers({ prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok({ prompt, screenRevision: "rev-1" })) }),
    );
    render(
      <DesktopChatPane session={s} visible onViewTerminal={vi.fn()} transport={transport}
        identity={{ hostId: "local" }} pollMs={0} fileStaging={fileStaging} />,
    );
    await waitFor(() => expect(capturedPromptProps.calls.length).toBeGreaterThan(0));
    // The same key the remote surface uses as `chatTargetKey`: prompt ids are content hashes,
    // so two panes asking one question would otherwise share a single answer set.
    expect(capturedPromptProps.calls.at(-1)?.answerScope).toBe(
      `local|owner|1|${s.backendSessionId}`,
    );
  });

  it("prunes the pane's staged answer sets once its prompt request resolves", async () => {
    const { referenceAnswerAttachmentStore } = await import(
      "../remote/chat/promptAttach/referencePromptAttach"
    );
    const s = session();
    const scope = `local|owner|1|${s.backendSessionId}`;
    // A file staged for this pane's question: its key is `<scope>\0<promptId>`.
    referenceAnswerAttachmentStore.add(`${scope}\u0000p-clear`, [
      new File([new TextEncoder().encode("x")], "keep.log", { type: "text/plain" }),
    ]);
    expect(referenceAnswerAttachmentStore.get(`${scope}\u0000p-clear`)).toHaveLength(1);

    mount(
      s,
      handlers({ prompt: vi.fn<DesktopChatHandlers["prompt"]>(async () => ok({ prompt: null, screenRevision: null })) }),
    );
    // No prompt on the pane: the thread moved past the question, so its files go with it.
    await waitFor(() =>
      expect(referenceAnswerAttachmentStore.get(`${scope}\u0000p-clear`)).toHaveLength(0),
    );
  });
});
