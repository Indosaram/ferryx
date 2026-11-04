import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ReferencePartList } from "./MobileChatComponents";
import { MobileChatMessage } from "./MobileChatMessage";
import {
  REFERENCE_DIFF_INLINE_LIMIT,
  referencePartKey,
  referenceWorkGroupKey,
  summarizeReferenceDiff,
  type ReferenceWorkMember,
} from "./referenceWorkGroup";
import type { ReferencePart, ReferencePayloadRef, ReferenceTaskResult } from "./referenceTypes";

afterEach(cleanup);

function tool(overrides: Partial<Extract<ReferencePart, { kind: "tool" }>> = {}): Extract<ReferencePart, { kind: "tool" }> {
  return {
    kind: "tool",
    name: "read",
    summary: "Read src/app.ts",
    input: "src/app.ts",
    output: "file body",
    ...overrides,
  };
}

function task(overrides: Partial<ReferenceTaskResult> = {}): ReferenceTaskResult {
  return {
    id: "t1",
    title: "Explore the repository",
    status: "completed",
    result: "The parser lives in src-tauri.",
    ...overrides,
  };
}

function payload(overrides: Partial<ReferencePayloadRef> = {}): ReferencePayloadRef {
  return { ref: "payload-1", byteLength: 12, digest: "abc", truncated: false, ...overrides };
}

function diff(overrides: Partial<Extract<ReferencePart, { kind: "diff" }>> & { body?: string } = {}): ReferencePart {
  const { body, ...rest } = overrides;
  return {
    kind: "diff",
    patch: payload(),
    ...(body !== undefined ? { body } : {}),
    ...rest,
  } as ReferencePart;
}

const SMALL_PATCH = [
  "*** Begin Patch",
  "*** Update File: src/a.ts",
  "@@",
  "-old",
  "+new",
  "+newer",
  "*** End Patch",
].join("\n");

describe("work group keys", () => {
  it("keys a group by the first part id and falls back to index only when that id is absent", () => {
    const withIds: ReferenceWorkMember[] = [
      { part: tool({ id: "tool-a" }), index: 4 },
      { part: tool({ id: "tool-b" }), index: 5 },
    ];
    expect(referenceWorkGroupKey(withIds)).toBe("group:tool-a");
    expect(referencePartKey(withIds[1].part, 9)).toBe("part:tool-b");

    const rev2: ReferenceWorkMember[] = [
      { part: tool(), index: 2 },
      { part: tool(), index: 3 },
    ];
    expect(referenceWorkGroupKey(rev2)).toBe("group-index:2");
    expect(referencePartKey(rev2[0].part, 2)).toBe("index:2");
  });

  it("keeps group and part keys across append, replacement, and a window slide", () => {
    const first = tool({ id: "tool-a", status: "completed", output: "one" });
    const second = tool({ id: "tool-b", status: "completed", summary: "Read src/b.ts", output: "two" });
    const { rerender } = render(<ReferencePartList parts={[first, second]} />);

    expect(screen.getByTestId("reference-work-group")).toHaveAttribute("data-group-key", "group:tool-a");
    fireEvent.click(screen.getByTestId("reference-work-group-toggle"));
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "true");
    const rows = screen.getAllByTestId("work-row");
    fireEvent.click(rows[0]);
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("one");

    rerender(
      <ReferencePartList
        parts={[
          { kind: "text", text: "slid in", id: "text-0" },
          tool({ id: "tool-a", status: "completed", output: "replaced" }),
          second,
          tool({ id: "tool-c", status: "completed", summary: "Read src/c.ts" }),
        ]}
      />,
    );

    expect(screen.getByTestId("reference-work-group")).toHaveAttribute("data-group-key", "group:tool-a");
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByTestId("reference-tool-part")[0]).toHaveAttribute("data-part-key", "part:tool-a");
    expect(screen.getAllByTestId("reference-tool-part")[1]).toHaveAttribute("data-part-key", "part:tool-b");
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("replaced");
    expect(screen.getAllByTestId("reference-tool-part")).toHaveLength(3);
  });

  it("keeps a collapsed group collapsed, and its header height class, when a completed tool is appended", () => {
    const { rerender } = render(
      <ReferencePartList parts={[tool({ id: "tool-a", status: "completed" })]} />,
    );
    const toggle = screen.getByTestId("reference-work-group-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle.className).toContain("min-h-11");
    expect(screen.getByTestId("reference-work-group-body")).toHaveAttribute("hidden");

    rerender(
      <ReferencePartList
        parts={[
          tool({ id: "tool-a", status: "completed" }),
          tool({ id: "tool-b", status: "completed", summary: "Read src/b.ts" }),
        ]}
      />,
    );

    expect(screen.getByTestId("reference-work-group")).toHaveAttribute("data-group-key", "group:tool-a");
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("reference-work-group-body")).toHaveAttribute("hidden");
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveTextContent("2 tools");
  });
});

describe("work group expand state", () => {
  it("starts running groups expanded and completed groups collapsed", () => {
    const { rerender } = render(
      <ReferencePartList parts={[tool({ id: "run", status: "running", summary: "Reading" })]} />,
    );
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "true");

    rerender(<ReferencePartList parts={[tool({ id: "done", status: "completed" })]} />);
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "false");
  });

  it("expands a completed group when a later part is running, and does not collapse it when that part completes", () => {
    const { rerender } = render(
      <ReferencePartList parts={[tool({ id: "tool-a", status: "completed" })]} />,
    );
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "false");

    rerender(
      <ReferencePartList
        parts={[
          tool({ id: "tool-a", status: "completed" }),
          tool({ id: "tool-b", status: "running", summary: "Still reading" }),
        ]}
      />,
    );
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "true");

    rerender(
      <ReferencePartList
        parts={[
          tool({ id: "tool-a", status: "completed" }),
          tool({ id: "tool-b", status: "completed", summary: "Still reading" }),
        ]}
      />,
    );
    expect(screen.getByTestId("reference-work-group-toggle")).toHaveAttribute("aria-expanded", "true");
  });
});

describe("work status badges", () => {
  it("shows a badge only for a status the part recorded", () => {
    render(
      <ReferencePartList
        parts={[
          tool({ id: "a", status: "running", summary: "running tool" }),
          tool({ id: "b", status: "failed", summary: "failed tool", error: true }),
          tool({ id: "c", status: "unverifiable", summary: "unverifiable tool" }),
          tool({ id: "d", summary: "no status", error: true }),
        ]}
      />,
    );

    const badges = screen.getAllByTestId("reference-status-badge");
    expect(badges.map((badge) => badge.getAttribute("data-status"))).toEqual([
      "running",
      "failed",
      "unverifiable",
    ]);
    const plain = screen.getByText("no status").closest("[data-testid='reference-tool-part']");
    expect(plain?.querySelector("[data-testid='reference-status-badge']")).toBeNull();
    expect(plain).toHaveTextContent("failed");
  });

  it("shows cancelled and completed from the task record, not from a guess", () => {
    render(
      <ReferencePartList
        parts={[
          {
            kind: "taskResult",
            id: "tasks",
            tasks: [
              task({ id: "c", status: "cancelled", title: "stopped" }),
              task({ id: "u", status: "unverifiable", title: "unknown" }),
            ],
          },
        ]}
      />,
    );
    const badges = screen.getAllByTestId("reference-status-badge");
    expect(badges.map((badge) => badge.getAttribute("data-status"))).toEqual(["cancelled", "unverifiable"]);
    expect(screen.getByText("stopped").parentElement).toHaveTextContent("cancelled");
  });
});

describe("turn usage", () => {
  it("shows model and recorded token counts, and nothing when they are absent", () => {
    const { rerender } = render(
      <MobileChatMessage
        id="usage"
        role="assistant"
        content=""
        referenceMeta={{ provider: "claude", model: "opus", usage: { inputTokens: 3, cacheReadTokens: 1 } }}
        referenceParts={[{ kind: "text", text: "Answer." }]}
      />,
    );
    expect(screen.getByTestId("reference-turn-usage")).toHaveTextContent("opus · 3 in · 1 cache read");
    expect(screen.getByTestId("reference-turn-usage")).not.toHaveTextContent("out");

    rerender(
      <MobileChatMessage
        id="usage"
        role="assistant"
        content=""
        referenceMeta={{ provider: "claude" }}
        referenceParts={[{ kind: "text", text: "Answer." }]}
      />,
    );
    expect(screen.queryByTestId("reference-turn-usage")).not.toBeInTheDocument();

    rerender(
      <MobileChatMessage id="usage" role="assistant" content="" referenceParts={[{ kind: "text", text: "Answer." }]} />,
    );
    expect(screen.queryByTestId("reference-turn-usage")).not.toBeInTheDocument();
  });
});

describe("diff summary", () => {
  it("counts files and hunks in a codex patch and a unified diff", () => {
    expect(summarizeReferenceDiff(SMALL_PATCH)).toEqual({
      files: [{ path: "src/a.ts", additions: 2, deletions: 1 }],
      additions: 2,
      deletions: 1,
    });
    const unified = [
      "diff --git a/src/a.ts b/src/a.ts",
      "--- a/src/a.ts",
      "+++ b/src/a.ts",
      "@@ -1 +1 @@",
      "-old",
      "+new",
      "diff --git a/src/b.ts b/src/b.ts",
      "--- a/src/b.ts",
      "+++ b/src/b.ts",
      "@@ -1 +1 @@",
      "+only",
    ].join("\n");
    expect(summarizeReferenceDiff(unified).files).toEqual([
      { path: "src/a.ts", additions: 1, deletions: 1 },
      { path: "src/b.ts", additions: 1, deletions: 0 },
    ]);
  });

  it("shows file counts and a small inline body, and withholds a large body", () => {
    const { rerender } = render(<ReferencePartList parts={[diff({ id: "d1", body: SMALL_PATCH })]} />);
    expect(screen.getByTestId("reference-diff-summary")).toHaveTextContent("src/a.ts +2 -1");
    expect(screen.getByTestId("reference-diff-body")).toHaveTextContent("+newer");

    const large = `${SMALL_PATCH}\n${"x".repeat(REFERENCE_DIFF_INLINE_LIMIT)}`;
    rerender(<ReferencePartList parts={[diff({ id: "d1", body: large })]} />);
    expect(screen.getByTestId("reference-diff-summary")).toHaveTextContent("src/a.ts +2 -1");
    expect(screen.queryByTestId("reference-diff-body")).not.toBeInTheDocument();
    expect(screen.queryByText("+newer")).not.toBeInTheDocument();
  });

  it("renders a payload ref as load details and does not invent file counts", () => {
    const onLoadPayload = vi.fn();
    const part = diff({ id: "d2", patch: payload({ ref: "patch-9", byteLength: 12, truncated: true }) });
    render(<ReferencePartList parts={[part]} context={{ onLoadPayload }} />);

    expect(screen.getByTestId("reference-diff-summary")).toHaveTextContent("Diff · 12 B · truncated");
    expect(screen.getByTestId("reference-diff-summary")).not.toHaveTextContent("+");
    expect(screen.queryByTestId("reference-diff-files")).not.toBeInTheDocument();
    expect(onLoadPayload).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("reference-load-payload"));
    expect(onLoadPayload).toHaveBeenCalledTimes(1);
    expect(onLoadPayload).toHaveBeenCalledWith(payload({ ref: "patch-9", byteLength: 12, truncated: true }));
  });

  it("offers load details for a tool output payload and leaves the rev2 whole-output control alone", () => {
    const onLoadPayload = vi.fn();
    const onRequestWholeOutput = vi.fn();
    render(
      <ReferencePartList
        parts={[
          tool({
            id: "out",
            status: "completed",
            output: "head",
            outputRef: "rev2-ref",
            outputSize: 40,
            outputPayload: payload({ ref: "out-9", byteLength: 40 }),
          }),
        ]}
        context={{ onLoadPayload, onRequestWholeOutput }}
      />,
    );
    fireEvent.click(screen.getByTestId("reference-work-group-toggle"));
    fireEvent.click(screen.getByTestId("work-row"));
    fireEvent.click(screen.getByTestId("reference-load-payload"));
    expect(onLoadPayload).toHaveBeenCalledWith(payload({ ref: "out-9", byteLength: 40 }));
    fireEvent.click(screen.getByTestId("reference-tool-whole-output"));
    expect(onRequestWholeOutput).toHaveBeenCalledWith("rev2-ref");
  });
});

describe("subagent nesting", () => {
  it("nests a task under its parent call id and leaves an unknown parent in the task block", () => {
    render(
      <ReferencePartList
        parts={[
          tool({ id: "parent", name: "task", summary: "Explore", callId: "call-1", status: "completed", output: "" }),
          {
            kind: "taskResult",
            id: "results",
            tasks: [
              task({ id: "child", title: "child agent", result: "found it", parentCallId: "call-1" }),
              task({ id: "orphan", title: "orphan agent", status: "failed", result: "no parent", parentCallId: "missing" }),
            ],
          },
        ]}
      />,
    );

    const nested = screen.getByTestId("reference-nested-tasks");
    expect(nested).toHaveTextContent("found it");
    expect(nested.compareDocumentPosition(screen.getByTestId("reference-tool-part")) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("no parent");
    expect(screen.getByTestId("reference-task-results")).not.toHaveTextContent("found it");
  });

  it("does not nest a task that has no parent call id under a neighboring tool", () => {
    render(
      <ReferencePartList
        parts={[
          tool({ id: "tool-a", callId: "call-1", status: "completed" }),
          { kind: "taskResult", id: "results", tasks: [task({ title: "standalone" })] },
        ]}
      />,
    );
    expect(screen.queryByTestId("reference-nested-tasks")).not.toBeInTheDocument();
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("standalone");
  });
});

describe("rev2 reference page", () => {
  it("renders a page with no ids or meta as today", () => {
    render(
      <MobileChatMessage
        id="rev2"
        role="assistant"
        content=""
        referenceParts={[
          { kind: "text", text: "Here is the answer." },
          tool(),
          { kind: "taskResult", tasks: [task()] },
        ]}
      />,
    );

    expect(screen.queryByTestId("reference-work-group")).not.toBeInTheDocument();
    expect(screen.queryByTestId("reference-turn-usage")).not.toBeInTheDocument();
    expect(screen.queryByTestId("reference-load-payload")).not.toBeInTheDocument();
    const row = screen.getByTestId("reference-tool-part");
    expect(row.querySelector("[data-testid='reference-status-badge']")).toBeNull();
    expect(row).toHaveTextContent("Read src/app.ts");
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("Background task ended");
    expect(screen.getByText("Here is the answer.")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("file body");
  });
});

