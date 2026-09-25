import "./worktree-disk-test-dom";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentHistoryEntry, AgentHistoryMessage, AgentHistoryPage } from "../lib/agentHistory";
import { AgentHistoryDialog, type AgentHistoryServices } from "./AgentHistoryDialog";

afterEach(cleanup);

const mockEntries: AgentHistoryEntry[] = [
  {
    entryKey: "entry-claude-1",
    provider: "claude",
    providerSession: {
      key: "session_id",
      id: "session-abc-123",
      transcriptPath: "/path/to/claude/session.jsonl",
    },
    cwd: "/repo/project-a",
    version: "1.0",
    parentId: null,
  },
  {
    entryKey: "entry-claude-2",
    provider: "claude",
    providerSession: {
      key: "session_id",
      id: "session-xyz-789",
      transcriptPath: null,
    },
    cwd: "/repo/project-b",
    version: null,
    parentId: "entry-claude-1",
  },
];

const mockMessages: AgentHistoryMessage[] = [
  {
    ordinal: 1,
    role: "user",
    text: "Can you help me fix the build?",
    id: "msg-1",
    parentId: null,
  },
  {
    ordinal: 2,
    role: "assistant",
    text: "Sure, let's look at the error log.",
    id: "msg-2",
    parentId: "msg-1",
  },
];

function createMockServices(overrides: Partial<AgentHistoryServices> = {}): {
  services: AgentHistoryServices;
} {
  const services: AgentHistoryServices = {
    search: vi.fn(async () => ({
      items: mockEntries,
      nextCursor: null,
      partial: false,
      warnings: [],
    })),
    read: vi.fn(async () => ({
      items: mockMessages,
      nextCursor: null,
      partial: false,
      warnings: [],
    })),
    ...overrides,
  };
  return { services };
}

async function renderSettled(element: Parameters<typeof render>[0]) {
  let view!: ReturnType<typeof render>;
  await act(async () => {
    view = render(element);
  });
  return view;
}

describe("AgentHistoryDialog", () => {
  it("searching renders exactly the returned entries", async () => {
    const { services } = createMockServices();

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const input = view.getByLabelText("Search past conversations");
    const searchButton = view.getByRole("button", { name: /^Search$/i });

    await act(async () => {
      fireEvent.change(input, { target: { value: "build error" } });
    });

    await act(async () => {
      fireEvent.click(searchButton);
    });

    expect(services.search).toHaveBeenCalledWith({
      provider: "claude",
      cwd: "/repo/project-a",
      query: "build error",
      cursor: null,
      limit: 20,
    });

    expect(view.getByText("session-abc-123")).toBeTruthy();
    expect(view.getByText("session-xyz-789")).toBeTruthy();
    // The dialog header renders its own cwd prop, so the first entry's cwd is deliberately
    // not unique; the second entry's cwd appears only in the result row.
    expect(view.getAllByText("/repo/project-a").length).toBeGreaterThanOrEqual(1);
    expect(view.getAllByText("/repo/project-b").length).toBe(1);
  });

  it("empty result renders the empty-state text", async () => {
    const { services } = createMockServices({
      search: vi.fn(async () => ({
        items: [],
        nextCursor: null,
        partial: false,
        warnings: [],
      })),
    });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const searchButton = view.getByRole("button", { name: /^Search$/i });

    await act(async () => {
      fireEvent.click(searchButton);
    });

    expect(view.getByText("No conversations found")).toBeTruthy();
  });

  it("clicking Resume passes the entry to onResume", async () => {
    const { services } = createMockServices();
    const handleResume = vi.fn();

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={handleResume}
        services={services}
      />,
    );

    const searchButton = view.getByRole("button", { name: /^Search$/i });
    await act(async () => {
      fireEvent.click(searchButton);
    });

    const resumeButtons = view.getAllByRole("button", { name: /Resume/i });
    expect(resumeButtons.length).toBe(2);

    await act(async () => {
      fireEvent.click(resumeButtons[0]);
    });

    expect(handleResume).toHaveBeenCalledTimes(1);
    expect(handleResume).toHaveBeenCalledWith(mockEntries[0]);
  });

  it("an error from search renders inside a role=\"alert\"", async () => {
    const { services } = createMockServices({
      search: vi.fn(async () => {
        throw new Error("Failed to scan past conversations");
      }),
    });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const searchButton = view.getByRole("button", { name: /^Search$/i });
    await act(async () => {
      fireEvent.click(searchButton);
    });

    const alertElement = view.getByRole("alert");
    expect(alertElement).toBeTruthy();
    expect(alertElement.textContent).toContain("Failed to scan past conversations");
  });

  it("a stale entry-read response must not survive a newer search", async () => {
    type ReadDeferred = {
      resolve: (value: AgentHistoryPage<AgentHistoryMessage>) => void;
      reject: (err: unknown) => void;
    };
    const readDeferreds: ReadDeferred[] = [];
    const readMock = vi.fn(
      () =>
        new Promise<AgentHistoryPage<AgentHistoryMessage>>((resolve, reject) => {
          readDeferreds.push({ resolve, reject });
        }),
    );

    const { services } = createMockServices({ read: readMock });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const input = view.getByLabelText("Search past conversations");
    const searchButton = view.getByRole("button", { name: /^Search$/i });

    // 2. Click Search -> the two entries render
    await act(async () => {
      fireEvent.click(searchButton);
    });

    expect(view.getByText("session-abc-123")).toBeTruthy();
    expect(view.getByText("session-xyz-789")).toBeTruthy();

    // 3. Click the entry row session-abc-123 -> a read deferred is created (assert exactly one)
    const entryRow = view.getByText("session-abc-123");
    await act(async () => {
      fireEvent.click(entryRow);
    });

    expect(readDeferreds.length).toBe(1);

    // 4. WITHOUT resolving that read, change query and click Search again
    await act(async () => {
      fireEvent.change(input, { target: { value: "different query" } });
    });
    await act(async () => {
      fireEvent.click(searchButton);
    });

    // Assert the entry list is re-rendered from the new search
    expect(view.getByText("session-abc-123")).toBeTruthy();
    expect(view.getByText("session-xyz-789")).toBeTruthy();

    // 5. Now resolve the STALE read deferred with sentinel text
    await act(async () => {
      readDeferreds[0].resolve({
        items: [
          {
            ordinal: 1,
            role: "user",
            text: "stale-conversation-sentinel",
            id: "msg-stale-1",
            parentId: null,
          },
        ],
        nextCursor: null,
        partial: false,
        warnings: [],
      });
    });

    // 6. Assert sentinel is NOT in document, and message area shows no leftover from aborted read
    expect(view.queryByText("stale-conversation-sentinel")).toBeNull();
    expect(view.getByText("Select a conversation to view its messages.")).toBeTruthy();

    // 7. Finally assert entry list still shows the entries from the second search
    expect(view.getByText("session-abc-123")).toBeTruthy();
    expect(view.getByText("session-xyz-789")).toBeTruthy();
  });

  it("clears previous conversation messages while a new entry read is still pending", async () => {
    type ReadDeferred = {
      resolve: (value: AgentHistoryPage<AgentHistoryMessage>) => void;
      reject: (err: unknown) => void;
    };
    const readDeferreds: ReadDeferred[] = [];
    const readMock = vi.fn(
      () =>
        new Promise<AgentHistoryPage<AgentHistoryMessage>>((resolve, reject) => {
          readDeferreds.push({ resolve, reject });
        }),
    );

    const { services } = createMockServices({ read: readMock });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const searchButton = view.getByRole("button", { name: /^Search$/i });
    await act(async () => {
      fireEvent.click(searchButton);
    });

    // Select first entry
    const entry1Button = view.getByText("session-abc-123");
    await act(async () => {
      fireEvent.click(entry1Button);
    });

    expect(readDeferreds.length).toBe(1);

    // Resolve first entry read
    await act(async () => {
      readDeferreds[0].resolve({
        items: [
          {
            ordinal: 1,
            role: "user",
            text: "First conversation message text",
            id: "msg-first-1",
            parentId: null,
          },
        ],
        nextCursor: null,
        partial: false,
        warnings: [],
      });
    });

    expect(view.getByText("First conversation message text")).toBeTruthy();

    // Now select second entry while read will be pending
    const entry2Button = view.getByText("session-xyz-789");
    await act(async () => {
      fireEvent.click(entry2Button);
    });

    expect(readDeferreds.length).toBe(2);

    // While second read is still pending, the previous conversation's message must be cleared immediately
    expect(view.queryByText("First conversation message text")).toBeNull();

    // Resolve second read
    await act(async () => {
      readDeferreds[1].resolve({
        items: [
          {
            ordinal: 1,
            role: "user",
            text: "Second conversation message text",
            id: "msg-second-1",
            parentId: null,
          },
        ],
        nextCursor: null,
        partial: false,
        warnings: [],
      });
    });

    expect(view.getByText("Second conversation message text")).toBeTruthy();
    expect(view.queryByText("First conversation message text")).toBeNull();
  });

  it("a stale read cannot restore messages after the selection is cleared", async () => {
    const readDeferreds: Array<{ resolve: (value: AgentHistoryPage<AgentHistoryMessage>) => void }> = [];
    const readMock = vi.fn(
      () =>
        new Promise<AgentHistoryPage<AgentHistoryMessage>>((resolve) => {
          readDeferreds.push({ resolve });
        }),
    );
    const { services } = createMockServices({ read: readMock });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    const searchButton = view.getByRole("button", { name: /^Search$/i });
    await act(async () => {
      fireEvent.click(searchButton);
    });

    await act(async () => {
      fireEvent.click(view.getByText("session-abc-123"));
    });
    expect(readDeferreds.length).toBe(1);

    await act(async () => {
      fireEvent.click(searchButton);
    });

    await act(async () => {
      readDeferreds[0].resolve({
        items: [
          {
            ordinal: 1,
            role: "user",
            text: "stale-read-sentinel",
            id: "msg-stale",
            parentId: null,
          },
        ],
        nextCursor: null,
        partial: false,
        warnings: [],
      });
    });

    expect(view.queryByText("stale-read-sentinel")).toBeNull();
    expect(view.getByText("Select a conversation to view its messages.")).toBeTruthy();
  });

  it("a superseded load-more read does not disable later pagination", async () => {
    const readCalls: Array<{ resolve: (value: AgentHistoryPage<AgentHistoryMessage>) => void; cursor: string | null }> = [];
    const readMock = vi.fn((request: { entryKey: string; cursor: string | null; limit: number }) => {
      void request;
      return new Promise<AgentHistoryPage<AgentHistoryMessage>>((resolve) => {
        readCalls.push({ resolve, cursor: null });
      });
    });
    const { services } = createMockServices({ read: readMock as unknown as AgentHistoryServices["read"] });

    const view = await renderSettled(
      <AgentHistoryDialog
        workspaceId="ws-1"
        projectName="Test Project"
        cwd="/repo/project-a"
        onClose={vi.fn()}
        onResume={vi.fn()}
        services={services}
      />,
    );

    await act(async () => {
      fireEvent.click(view.getByRole("button", { name: /^Search$/i }));
    });

    await act(async () => {
      fireEvent.click(view.getByText("session-abc-123"));
    });
    expect(readCalls.length).toBe(1);

    await act(async () => {
      readCalls[0].resolve({
        items: [{ ordinal: 1, role: "user", text: "first page", id: "m1", parentId: null }],
        nextCursor: "cursor-1",
        partial: false,
        warnings: [],
      });
    });

    await act(async () => {
      fireEvent.click(view.getByRole("button", { name: /load more/i }));
    });
    expect(readCalls.length).toBe(2);

    await act(async () => {
      fireEvent.click(view.getByText("session-xyz-789"));
    });
    expect(readCalls.length).toBe(3);

    await act(async () => {
      readCalls[2].resolve({
        items: [{ ordinal: 1, role: "user", text: "second entry", id: "m2", parentId: null }],
        nextCursor: "cursor-2",
        partial: false,
        warnings: [],
      });
    });

    await act(async () => {
      fireEvent.click(view.getByRole("button", { name: /load more/i }));
    });
    expect(readCalls.length).toBe(4);
  });
});
