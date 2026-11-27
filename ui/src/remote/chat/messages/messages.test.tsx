import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import { AttachmentList, chatTextHasMath } from "../MobileChatComponents";
import { ANNOUNCE_MESSAGE_MAX_CHARS, announcementText, shouldAnnounce } from "./announcer";
import { referenceActionCategoryOfTool, referenceWorkActivitySummary, type ReferenceWorkMember, type ReferenceWorkNesting } from "../referenceWorkGroup";
import { highlightLinesSync, isCodeBlockTooLarge, loadHighlight } from "./codeHighlight";
import type { ReferencePart } from "../referenceTypes";

afterEach(cleanup);

const emptyNesting: ReferenceWorkNesting = { tasksByCallId: new Map(), localTasks: new Map() };
const member = (part: ReferencePart, index: number): ReferenceWorkMember => ({ part, index });

describe("work group activity summary", () => {
  it("names up to two categories and counts the rest", () => {
    const bash = (id: string): ReferencePart => ({ kind: "tool", name: "bash", summary: "", input: "ls", output: "", id });
    const edit = (id: string): ReferencePart => ({ kind: "tool", name: "apply_patch", summary: "", input: "x", output: "", id });
    const read = (id: string): ReferencePart => ({ kind: "tool", name: "read", summary: "", input: "f", output: "", id });
    const members = [
      member(bash("a"), 0),
      member(bash("b"), 1),
      member(edit("c"), 2),
      member(edit("d"), 3),
      member(edit("e"), 4),
      member(read("f"), 5),
    ];
    expect(referenceWorkActivitySummary(members, emptyNesting)).toBe(
      "Ran 2 commands and edited 3 files and 1 more action",
    );
  });

  it("counts diff parts as file changes and tasks as orchestration", () => {
    const diff: ReferencePart = {
      kind: "diff",
      patch: { ref: "p", byteLength: 4, digest: "d", truncated: false },
    };
    const tasks: ReferencePart = {
      kind: "taskResult",
      tasks: [
        { id: "t1", title: "x", status: "completed", result: "r" },
        { id: "t2", title: "y", status: "completed", result: "r" },
      ],
    };
    expect(referenceWorkActivitySummary([member(diff, 0), member(tasks, 1)], emptyNesting)).toBe(
      "Edited 1 file and ran 2 tasks",
    );
  });

  it("returns null with nothing countable, and reads tools as reads", () => {
    const text: ReferencePart = { kind: "text", text: "x" };
    expect(referenceWorkActivitySummary([member(text, 0)], emptyNesting)).toBeNull();
    expect(referenceActionCategoryOfTool("grep")).toBe("reads");
    expect(referenceActionCategoryOfTool("bash")).toBe("commands");
    expect(referenceActionCategoryOfTool("mystery_tool")).toBe("other");
  });
});

describe("announcements", () => {
  it("levels decide what is read", () => {
    expect(shouldAnnounce("status-only", "status")).toBe(true);
    expect(shouldAnnounce("status-only", "message")).toBe(false);
    expect(shouldAnnounce("status-and-messages", "message")).toBe(true);
    expect(shouldAnnounce("status-and-messages", "step")).toBe(false);
    expect(shouldAnnounce("everything", "step")).toBe(true);
  });

  it("cuts a long message short and skips empty text", () => {
    const long = "a".repeat(ANNOUNCE_MESSAGE_MAX_CHARS + 50);
    const cut = announcementText("status-and-messages", "message", long)!;
    expect(cut.length).toBeLessThanOrEqual(ANNOUNCE_MESSAGE_MAX_CHARS);
    expect(cut.endsWith("…")).toBe(true);
    expect(announcementText("everything", "step", "  ")).toBeNull();
  });

  it("announces the running step of a live turn at the everything level", () => {
    const tool: ReferencePart = {
      kind: "tool",
      name: "bash",
      summary: "",
      input: "ls",
      output: "",
      status: "running",
      id: "t",
    };
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool]}
        activityState="running_tool"
        announceLevel="everything"
      />,
    );
    expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Working");
    expect(screen.getByTestId("thread-announcer")).toHaveTextContent("Running bash");
  });

  it("stays silent without a level or an idle turn", () => {
    const tool: ReferencePart = { kind: "tool", name: "bash", summary: "", input: "ls", output: "", id: "t" };
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="body"
        referenceParts={[tool]}
        activityState="running_tool"
      />,
    );
    expect(screen.queryByTestId("thread-announcer")).not.toBeInTheDocument();
  });

  it("reads an in-between message only from status-and-messages up, and never the final reply", () => {
    const { unmount } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="halfway note"
        activityState="running_tool"
        isTurnFinalReply={false}
        announceLevel="status-only"
      />,
    );
    expect(screen.getByTestId("thread-announcer")).not.toHaveTextContent("halfway note");
    unmount();

    const { unmount: second } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="halfway note"
        activityState="running_tool"
        isTurnFinalReply={false}
        announceLevel="status-and-messages"
      />,
    );
    expect(screen.getByTestId("thread-announcer")).toHaveTextContent("halfway note");
    second();

    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="final answer"
        activityState="running_tool"
        isTurnFinalReply={true}
        announceLevel="status-and-messages"
      />,
    );
    expect(screen.getByTestId("thread-announcer")).not.toHaveTextContent("final answer");
  });
});

describe("math detection", () => {
  it("reads a formula but not a price or a pair of shell variables", () => {
    expect(chatTextHasMath("Result: $x^2$")).toBe(true);
    expect(chatTextHasMath("$$\\int_0^1 x$$")).toBe(true);
    expect(chatTextHasMath("It costs $5 and $10 total.")).toBe(false);
    expect(chatTextHasMath("echo $PATH and $HOME")).toBe(false);
    expect(chatTextHasMath("no math here")).toBe(false);
  });
});

describe("code block highlighting", () => {
  it("maps fence languages to the registered highlight.js grammars", async () => {
    await loadHighlight();
    expect(highlightLinesSync("const x = 1;", "ts").lines).not.toBeNull();
    expect(highlightLinesSync("def f(): pass", "python").lines).not.toBeNull();
    // The punctuation t3code keeps in a fence language still names a grammar.
    expect(highlightLinesSync("int main() {}", "c++").lines).not.toBeNull();
    expect(highlightLinesSync("public class A {}", "c#").lines).not.toBeNull();
    // Unknown grammar falls back to plain code.
    expect(highlightLinesSync("plain", "notalanguage").lines).toBeNull();
    expect(highlightLinesSync("plain", "text").lines).toBeNull();
  });

  it("skips highlighting for a huge block and keeps copy and wrap on the plain code", () => {
    // The cap is a line count and a byte count: either one skips the lazy load.
    expect(isCodeBlockTooLarge(Array.from({ length: 20_001 }, () => "x").join("\n"))).toBe(true);
    expect(isCodeBlockTooLarge("x".repeat(512 * 1024 + 1))).toBe(true);
    expect(isCodeBlockTooLarge("const x = 1;")).toBe(false);

    const { container } = render(
      <MobileChatMessage id="plain" role="assistant" content={"```notalanguage\nconst x = 1;\n```"} />,
    );
    const lines = screen.getByTestId("highlighted-code-lines");
    expect(lines).toHaveAttribute("data-highlighted", "false");
    expect(lines).toHaveTextContent("const x = 1;");
    expect(container.querySelector('[data-highlighted="true"]')).toBeNull();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Wrap" })).toBeInTheDocument();
  });
});

describe("message copy button readability", () => {
  it("copies the turn's prose markdown, not internal structure", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[
          { kind: "text", text: "First paragraph", id: "a" },
          { kind: "text", text: "Second **bold**", id: "b" },
        ]}
      />,
    );
    fireEvent.click(screen.getByTestId("message-copy-button"));
    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("First paragraph\n\nSecond **bold**");
    });
  });

  it("offers the same copy button on a user message", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<MobileChatMessage id="u" role="user" content="fix the **parser**" />);
    fireEvent.click(screen.getByTestId("message-copy-button"));
    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("fix the **parser**");
    });
  });
});

describe("message images", () => {
  it("opens a pasted attachment in the lightbox and closes it with Escape", () => {
    render(
      <AttachmentList
        attachments={[
          { id: "1", name: "shot.png", type: "image", url: "data:image/png;base64,AAAA" },
        ]}
      />,
    );
    fireEvent.click(screen.getByTestId("message-image-thumbnail"));
    expect(screen.getByTestId("expanded-image-dialog")).toBeInTheDocument();
    // The overlay label must not swallow the tap that opens the picture.
    expect(screen.getByTestId("message-image-thumbnail").closest("button")).not.toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("expanded-image-dialog")).not.toBeInTheDocument();
  });
});

describe("open diff", () => {
  it("offers Open diff only when the pane wires a handler, and hands it the part", () => {
    const part: ReferencePart = {
      kind: "diff",
      patch: { ref: "p", byteLength: 4, digest: "d", truncated: false },
      body: "diff --git a/src/a.ts b/src/a.ts\n+new",
    };
    const { unmount } = render(
      <MobileChatMessage id="m" role="assistant" content="" referenceParts={[part]} />,
    );
    expect(screen.queryByTestId("reference-open-diff")).not.toBeInTheDocument();
    unmount();

    const onOpenDiff = vi.fn();
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[part]}
        referenceContext={{ onOpenDiff }}
      />,
    );
    fireEvent.click(screen.getByTestId("reference-open-diff"));
    expect(onOpenDiff).toHaveBeenCalledWith(part);
  });
});
