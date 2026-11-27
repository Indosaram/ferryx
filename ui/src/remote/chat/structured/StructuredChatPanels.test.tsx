/**
 * Lane H5: the structured pane's own panels - the extension card, the status strip, the session
 * tree drawer and the media view.
 *
 * Each case presses what the reader presses and asserts the wire call it must produce, so the
 * assertions stay on the promise (`answer_extension_ui` answers this request, `navigate_tree`
 * goes to this entry) rather than on the component's internal state.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { ExtensionUiCard, structuredExtensionQuestions } from "./ExtensionUiCard";
import { ExtensionStatusStrip } from "./ExtensionStatusStrip";
import { MediaPartView, structuredMediaDataUrl } from "./MediaPartView";
import { SessionTreeDrawer, structuredTreeRows } from "./SessionTreeDrawer";
import type {
  StructuredCapabilities,
  StructuredExtensionUiRequest,
  StructuredExtensionUiResponse,
  StructuredMediaBlob,
  StructuredMediaRef,
  StructuredSessionSettings,
  StructuredSessionTree,
} from "../../../lib/structuredChatTypes";

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

function request(extra: Record<string, unknown>): StructuredExtensionUiRequest {
  return { requestId: "req-1", method: "select", options: [], seq: 1, tsMs: 1000, ...extra } as StructuredExtensionUiRequest;
}

describe("extension UI card", () => {
  it("answers a select with the picked value, not the first option", async () => {
    const answers: { requestId: string; response: StructuredExtensionUiResponse }[] = [];
    render(
      <ExtensionUiCard
        request={request({ method: "select", title: "Pick one", options: ["alpha", "beta"] })}
        onAnswer={async (requestId, response) => {
          answers.push({ requestId, response });
          return true;
        }}
      />,
    );
    // Submit stays shut until the reader actually picks: a select with no pick has no answer.
    expect(screen.getByTestId("extension-ui-submit")).toBeDisabled();
    fireEvent.click(screen.getByTestId("extension-ui-option-1"));
    fireEvent.click(screen.getByTestId("extension-ui-submit"));
    await waitFor(() => expect(answers).toEqual([{ requestId: "req-1", response: { value: "beta" } }]));
  });

  it("answers a confirm with confirmed and a dismissal with cancelled", async () => {
    const answers: StructuredExtensionUiResponse[] = [];
    const onAnswer = async (_id: string, response: StructuredExtensionUiResponse): Promise<boolean> => {
      answers.push(response);
      return true;
    };
    render(<ExtensionUiCard request={request({ method: "confirm", title: "Run it?" })} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByTestId("extension-ui-submit"));
    await waitFor(() => expect(answers).toEqual([{ confirmed: true }]));

    cleanup();
    render(<ExtensionUiCard request={request({ method: "confirm", title: "Run it?" })} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByTestId("extension-ui-cancel"));
    await waitFor(() => expect(answers).toEqual([{ confirmed: true }, { cancelled: true }]));
  });

  it("takes the prefill as an editable starting point for an editor", async () => {
    const answers: StructuredExtensionUiResponse[] = [];
    render(
      <ExtensionUiCard
        request={request({ method: "editor", title: "Edit the plan", prefill: "step one" })}
        onAnswer={async (_id, response) => {
          answers.push(response);
          return true;
        }}
      />,
    );
    expect(screen.getByTestId("extension-ui-editor")).toHaveValue("step one");
    fireEvent.change(screen.getByTestId("extension-ui-editor"), { target: { value: "step two" } });
    fireEvent.click(screen.getByTestId("extension-ui-submit"));
    await waitFor(() => expect(answers).toEqual([{ value: "step two" }]));
  });

  it("will not send an empty input", async () => {
    const answers: StructuredExtensionUiResponse[] = [];
    render(
      <ExtensionUiCard
        request={request({ method: "input", title: "Name it" })}
        onAnswer={async (_id, response) => {
          answers.push(response);
          return true;
        }}
      />,
    );
    expect(screen.getByTestId("extension-ui-submit")).toBeDisabled();
    fireEvent.change(screen.getByTestId("extension-ui-input"), { target: { value: "  " } });
    expect(screen.getByTestId("extension-ui-submit")).toBeDisabled();
    fireEvent.change(screen.getByTestId("extension-ui-input"), { target: { value: "release" } });
    fireEvent.click(screen.getByTestId("extension-ui-submit"));
    await waitFor(() => expect(answers).toEqual([{ value: "release" }]));
  });

  it("answers a multi-question form keyed by question id, and only when every row is answered", async () => {
    const answers: StructuredExtensionUiResponse[] = [];
    render(
      <ExtensionUiCard
        request={request({
          method: "question",
          title: "Two questions",
          payload: {
            questions: [
              { id: "q1", header: "Runtime", question: "Which runtime?", options: [{ label: "bun" }, { label: "node" }], multiSelect: false },
              { id: "q2", header: "Targets", question: "Which targets?", options: ["linux", "macos"], multiSelect: true },
            ],
          },
        })}
        onAnswer={async (_id, response) => {
          answers.push(response);
          return true;
        }}
      />,
    );
    fireEvent.click(screen.getByTestId("extension-ui-question-q1-option-0"));
    // One row is still open, so the form does not send a half answer.
    expect(screen.getByTestId("extension-ui-submit")).toBeDisabled();
    fireEvent.click(screen.getByTestId("extension-ui-question-q2-option-0"));
    fireEvent.click(screen.getByTestId("extension-ui-question-q2-option-1"));
    fireEvent.click(screen.getByTestId("extension-ui-submit"));
    await waitFor(() =>
      expect(answers).toEqual([
        {
          answers: {
            q1: { selected: ["bun"] },
            q2: { selected: ["linux", "macos"] },
          },
        },
      ]),
    );
  });

  it("reads a question payload defensively and never invents an id", () => {
    expect(
      structuredExtensionQuestions(
        request({
          method: "question",
          payload: { questions: [{ header: "no id", question: "?" }, { id: "q1", question: "real" }] },
        }),
      ),
    ).toEqual([{ id: "q1", header: "", question: "real", options: [], multiSelect: false }]);
    expect(structuredExtensionQuestions(request({ method: "question" }))).toEqual([]);
    expect(structuredExtensionQuestions(request({ method: "question", payload: { questions: "nope" } }))).toEqual([]);
  });

  it("offers only a dismissal for a method it does not draw a control for", async () => {
    const answers: StructuredExtensionUiResponse[] = [];
    render(
      <ExtensionUiCard
        request={request({ method: "custom_unsupported", title: "An extension" })}
        onAnswer={async (_id, response) => {
          answers.push(response);
          return true;
        }}
      />,
    );
    expect(screen.queryByTestId("extension-ui-submit")).toBeNull();
    fireEvent.click(screen.getByTestId("extension-ui-dismiss"));
    await waitFor(() => expect(answers).toEqual([{ cancelled: true }]));
  });
});

describe("extension status strip", () => {
  const EMPTY = { statuses: [], widgets: [], subagents: [], goals: [] } as const;

  it("draws nothing when the extension has reported nothing", () => {
    const { container } = render(<ExtensionStatusStrip {...EMPTY} />);
    expect(container.firstChild).toBeNull();
  });

  it("draws the latest value per key as a pill, a widget block, a subagent badge and a goal", () => {
    render(
      <ExtensionStatusStrip
        statuses={[{ statusKey: "websearch", statusText: "Searching", seq: 2 }]}
        widgets={[{ widgetKey: "todo", lines: ["one", "two"], seq: 3 }]}
        subagents={[{ taskId: "t1", name: "auditor", status: "completed", seq: 4 }]}
        goals={[{ goalId: "g1", title: "Ship the lane", status: "inProgress", progress: "2/5", seq: 5 }]}
      />,
    );
    expect(screen.getByTestId("extension-status-websearch")).toHaveTextContent("Searching");
    expect(screen.getByTestId("extension-widget-todo")).toHaveTextContent("one");
    expect(screen.getByTestId("extension-widget-todo")).toHaveTextContent("two");
    expect(screen.getByTestId("extension-subagent-t1")).toHaveAttribute("data-status", "done");
    expect(screen.getByTestId("extension-goal-g1")).toHaveTextContent("Ship the lane");
    expect(screen.getByTestId("extension-goal-g1")).toHaveTextContent("2/5");
  });

  it("marks a failed subagent as failed rather than as still running", () => {
    render(
      <ExtensionStatusStrip
        {...EMPTY}
        subagents={[{ taskId: "t1", name: "auditor", status: "failed", seq: 1 }]}
      />,
    );
    expect(screen.getByTestId("extension-subagent-t1")).toHaveAttribute("data-status", "failed");
  });

  it("draws a widget only where the provider placed it, and an unplaced one above the editor", () => {
    const widgets = [
      { widgetKey: "above", lines: ["a"], placement: "aboveEditor", seq: 1 },
      { widgetKey: "below", lines: ["b"], placement: "belowEditor", seq: 2 },
      { widgetKey: "unplaced", lines: ["c"], seq: 3 },
    ];
    render(<ExtensionStatusStrip {...EMPTY} widgets={widgets} placement="aboveEditor" />);
    expect(screen.getByTestId("extension-widget-above")).toBeInTheDocument();
    expect(screen.getByTestId("extension-widget-unplaced")).toBeInTheDocument();
    expect(screen.queryByTestId("extension-widget-below")).toBeNull();

    cleanup();
    render(<ExtensionStatusStrip {...EMPTY} widgets={widgets} placement="belowEditor" />);
    expect(screen.getByTestId("extension-widget-below")).toBeInTheDocument();
    expect(screen.queryByTestId("extension-widget-above")).toBeNull();
  });

  it("keeps a cleared widget's place instead of collapsing the strip", () => {
    render(<ExtensionStatusStrip {...EMPTY} widgets={[{ widgetKey: "todo", lines: [], seq: 1 }]} />);
    expect(screen.getByTestId("extension-widget-todo-empty")).toHaveTextContent("(empty)");
  });
});

describe("media part view", () => {
  const ref: StructuredMediaRef = { toolCallId: "call-1", contentIndex: 0, mimeType: "image/png", seq: 3 };
  const blob: StructuredMediaBlob = { mimeType: "image/png", dataBase64: "iVBORw0KGgo=" };

  it("builds a data URL only for an image with bytes", () => {
    expect(structuredMediaDataUrl(blob)).toBe("data:image/png;base64,iVBORw0KGgo=");
    expect(structuredMediaDataUrl({ mimeType: "text/plain", dataBase64: "aGk=" })).toBeNull();
    expect(structuredMediaDataUrl({ mimeType: "image/png", dataBase64: "" })).toBeNull();
    expect(structuredMediaDataUrl({ mimeType: "", dataBase64: "aGk=" })).toBeNull();
  });

  it("reads the item once and draws the picture", async () => {
    const fetchMedia = vi.fn(async () => blob);
    render(<MediaPartView mediaRef={ref} fetchMedia={fetchMedia} />);
    await waitFor(() => expect(screen.getByTestId("structured-media-thumbnail")).toBeInTheDocument());
    expect(fetchMedia).toHaveBeenCalledTimes(1);
    expect(fetchMedia).toHaveBeenCalledWith("call-1", 0);
  });

  it("does not re-read bytes the lane already holds", async () => {
    const fetchMedia = vi.fn(async () => blob);
    render(<MediaPartView mediaRef={ref} blob={blob} fetchMedia={fetchMedia} />);
    await waitFor(() => expect(screen.getByTestId("structured-media-thumbnail")).toBeInTheDocument());
    expect(fetchMedia).not.toHaveBeenCalled();
  });

  it("names a refusal instead of drawing an empty box", async () => {
    render(<MediaPartView mediaRef={ref} fetchMedia={async () => null} />);
    await waitFor(() =>
      expect(screen.getByTestId("structured-media-unavailable")).toHaveTextContent("Couldn't load this attachment"),
    );
  });

  it("says what an item is when the provider sent no image at all", async () => {
    const textRef: StructuredMediaRef = { ...ref, mimeType: "application/pdf" };
    render(<MediaPartView mediaRef={textRef} blob={{ mimeType: "application/pdf", dataBase64: "aGk=" }} fetchMedia={async () => null} />);
    await waitFor(() =>
      expect(screen.getByTestId("structured-media-unavailable")).toHaveTextContent("application/pdf"),
    );
  });
});

describe("session tree drawer", () => {
  const CAPABILITIES: StructuredCapabilities = {
    steer: true,
    rollback: true,
    permissionModes: [],
    models: [{ id: "gpt-5", label: "GPT-5", efforts: [] }],
    slashCommands: [],
    treeNavigation: true,
    fork: true,
    cloneSession: true,
    sessionRename: true,
    sessionStats: true,
    reload: true,
    compactCustomInstructions: true,
    favoriteModels: true,
    scopedModels: true,
    editUserMessage: true,
    editAssistantMessage: false,
  };

  const TREE: StructuredSessionTree = {
    currentLeafId: "n2",
    nodes: [
      { id: "n1", role: "user", preview: "first prompt", active: false },
      { id: "n2", parentId: "n1", role: "assistant", preview: "reply", active: true },
      { id: "n3", parentId: "n1", role: "assistant", preview: "other branch", active: false },
    ],
  };

  function renderDrawer(overrides: Partial<React.ComponentProps<typeof SessionTreeDrawer>> = {}) {
    const calls: { op: string; args: unknown[] }[] = [];
    const props: React.ComponentProps<typeof SessionTreeDrawer> = {
      tree: TREE,
      capabilities: CAPABILITIES,
      settings: {} as StructuredSessionSettings,
      stats: null,
      sessionName: null,
      running: false,
      models: CAPABILITIES.models,
      onClose: () => {},
      fetchTree: async () => {
        calls.push({ op: "tree", args: [] });
        return TREE;
      },
      navigateTree: async (targetId) => {
        calls.push({ op: "navigateTree", args: [targetId] });
        return true;
      },
      fork: async (entryId) => {
        calls.push({ op: "fork", args: [entryId] });
        return { sessionId: "forked-1" };
      },
      cloneSession: async () => {
        calls.push({ op: "clone", args: [] });
        return { sessionId: "cloned-1" };
      },
      renameSession: async (name) => {
        calls.push({ op: "rename", args: [name] });
        return true;
      },
      editMessage: async (entryId, role, text) => {
        calls.push({ op: "editMessage", args: [entryId, role, text] });
        return true;
      },
      fetchStats: async () => {
        calls.push({ op: "stats", args: [] });
        return { turns: 3, totalTokens: 1200, inputTokens: 800, outputTokens: 400, contextWindow: 128000 };
      },
      reloadSession: async () => {
        calls.push({ op: "reload", args: [] });
        return true;
      },
      clearQueue: async () => {
        calls.push({ op: "clearQueue", args: [] });
        return true;
      },
      setSettings: async (settings) => {
        calls.push({ op: "settings", args: [settings] });
        return true;
      },
      setModelPreferences: async (preferences) => {
        calls.push({ op: "modelPreferences", args: [preferences] });
        return true;
      },
      compact: async (instructions) => {
        calls.push({ op: "compact", args: [instructions] });
      },
      ...overrides,
    };
    render(<SessionTreeDrawer {...props} />);
    return { calls };
  }

  it("orders the tree parents first and indents children by depth", () => {
    expect(structuredTreeRows(TREE).map((row) => [row.node.id, row.depth])).toEqual([
      ["n1", 0],
      ["n2", 1],
      ["n3", 1],
    ]);
  });

  it("keeps a node whose parent the page left out, as a root", () => {
    const partial: StructuredSessionTree = {
      nodes: [
        { id: "a", parentId: "missing", role: "user", preview: "p", active: false },
        { id: "b", role: "assistant", preview: "q", active: true },
      ],
    };
    // Neither node has a parent the page included, so both are roots - and neither is lost.
    expect(structuredTreeRows(partial).map((row) => [row.node.id, row.depth])).toEqual([
      ["a", 0],
      ["b", 0],
    ]);
  });

  it("does not loop forever on a cycle", () => {
    const cyclic: StructuredSessionTree = {
      nodes: [
        { id: "a", parentId: "b", role: "user", preview: "p", active: false },
        { id: "b", parentId: "a", role: "assistant", preview: "q", active: true },
      ],
    };
    expect(structuredTreeRows(cyclic).map((row) => row.node.id).sort()).toEqual(["a", "b"]);
  });

  it("navigates to a branch by its entry id, and refuses the leaf it is already on", async () => {
    const { calls } = renderDrawer();
    await waitFor(() => expect(calls.filter((call) => call.op === "tree")).toHaveLength(1));
    // The current leaf is where the session already is, so its control is not offered.
    expect(screen.getByTestId("session-tree-go-n2")).toBeDisabled();
    fireEvent.click(screen.getByTestId("session-tree-go-n3"));
    await waitFor(() => expect(calls).toContainEqual({ op: "navigateTree", args: ["n3"] }));
  });

  it("forks at an entry and says which session it produced", async () => {
    const { calls } = renderDrawer();
    fireEvent.click(screen.getByTestId("session-tree-fork-n1"));
    await waitFor(() => expect(calls).toContainEqual({ op: "fork", args: ["n1"] }));
    expect(await screen.findByTestId("session-tree-forked")).toHaveTextContent("forked-1");
  });

  it("edits only the roles the agent can rewrite", async () => {
    const { calls } = renderDrawer();
    // `editAssistantMessage` is false, so the assistant node offers no pencil.
    expect(screen.getByTestId("session-tree-edit-n1")).toBeInTheDocument();
    expect(screen.queryByTestId("session-tree-edit-n2")).toBeNull();
    fireEvent.click(screen.getByTestId("session-tree-edit-n1"));
    fireEvent.change(screen.getByTestId("session-tree-edit-text"), { target: { value: "rewritten prompt" } });
    fireEvent.submit(screen.getByTestId("session-tree-edit-form"));
    await waitFor(() =>
      expect(calls).toContainEqual({ op: "editMessage", args: ["n1", "user", "rewritten prompt"] }),
    );
  });

  it("compacts with the reader's own instructions, and not while a turn is running", async () => {
    const { calls } = renderDrawer();
    fireEvent.change(screen.getByTestId("session-action-compact-instructions"), {
      target: { value: "keep the API decisions" },
    });
    fireEvent.submit(screen.getByTestId("session-action-compact-form"));
    await waitFor(() =>
      expect(calls).toContainEqual({ op: "compact", args: ["keep the API decisions"] }),
    );

    cleanup();
    renderDrawer({ running: true });
    expect(screen.getByTestId("session-action-compact")).toBeDisabled();
  });

  it("toggles auto-compaction and auto-retry as session settings", async () => {
    const { calls } = renderDrawer();
    fireEvent.click(screen.getByTestId("session-action-auto-compaction"));
    await waitFor(() =>
      expect(calls).toContainEqual({ op: "settings", args: [{ autoCompaction: true }] }),
    );
    fireEvent.click(screen.getByTestId("session-action-auto-retry"));
    await waitFor(() => expect(calls).toContainEqual({ op: "settings", args: [{ autoRetry: true }] }));
  });

  it("renames, reloads, clears the queue and clones from the session panel", async () => {
    const { calls } = renderDrawer({ sessionName: "old name" });
    expect(screen.getByTestId("session-action-rename-input")).toHaveValue("old name");
    fireEvent.change(screen.getByTestId("session-action-rename-input"), { target: { value: "new name" } });
    fireEvent.submit(screen.getByTestId("session-action-rename-form"));
    await waitFor(() => expect(calls).toContainEqual({ op: "rename", args: ["new name"] }));

    fireEvent.click(screen.getByTestId("session-action-reload"));
    await waitFor(() => expect(calls).toContainEqual({ op: "reload", args: [] }));

    fireEvent.click(screen.getByTestId("session-action-clear-queue"));
    await waitFor(() => expect(calls).toContainEqual({ op: "clearQueue", args: [] }));

    fireEvent.click(screen.getByTestId("session-action-clone"));
    await waitFor(() => expect(calls).toContainEqual({ op: "clone", args: [] }));
    expect(await screen.findByTestId("session-tree-forked")).toHaveTextContent("cloned-1");
  });

  it("shows the session's totals after reading them", async () => {
    const { calls } = renderDrawer();
    fireEvent.click(screen.getByTestId("session-action-stats"));
    await waitFor(() => expect(calls).toContainEqual({ op: "stats", args: [] }));
    expect(await screen.findByTestId("session-stats-turns")).toHaveTextContent("3");
    expect(screen.getByTestId("session-stats-context")).toHaveTextContent("128000");
  });

  it("saves only the model preferences the agent advertises", async () => {
    const { calls } = renderDrawer();
    fireEvent.click(screen.getByTestId("session-model-favorite-gpt-5"));
    fireEvent.click(screen.getByTestId("session-model-preferences-save"));
    await waitFor(() =>
      expect(calls).toContainEqual({ op: "modelPreferences", args: [{ favoriteModels: ["gpt-5"], scopedModels: [] }] }),
    );
  });

  it("draws no branch controls at all for an agent without a tree", () => {
    renderDrawer({ capabilities: { ...CAPABILITIES, treeNavigation: false, fork: false } });
    expect(screen.getByTestId("session-tree-unsupported")).toBeInTheDocument();
    expect(screen.queryByTestId("session-tree-go-n1")).toBeNull();
    expect(screen.queryByTestId("session-tree-fork-n1")).toBeNull();
  });
});
