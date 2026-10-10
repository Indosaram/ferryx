import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
  REFERENCE_ABANDONED_NOTE,
  ReferenceAbandonedBranchDisclosure,
  ReferenceHistoryDisclosure,
  ReferenceImagePart,
  ReferenceOlderPageControl,
  ReferencePartList,
  formatReferenceTimestamp,
  referenceAbandonedSummary,
  referenceNoticeLabel,
  referenceOlderControlLabel,
  referenceSkillLabel,
  referenceTaskResultMeta,
  referenceTaskStatusWord,
  referenceTurnSkills,
} from "./MobileChatComponents";
import { MobileChatMessage } from "./MobileChatMessage";
import {
  REFERENCE_SCROLLBACK_DISCLOSURE,
  type ReferenceHistoryPage,
  type ReferencePart,
  type ReferenceTaskResult,
} from "./referenceTypes";
import * as clipboardModule from "../../lib/clipboard";

function page(overrides: Partial<ReferenceHistoryPage> = {}): ReferenceHistoryPage {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: [],
    cursor: null,
    hasMore: false,
    generation: "gen-1",
    unavailableReason: null,
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

describe("MobileChatMessage reference rich parts", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders every rich part kind of a native turn", () => {
    const parts: ReferencePart[] = [
      { kind: "text", text: "Here is the answer.", phase: "finalAnswer" },
      { kind: "thinking", text: "First thought\nSecond thought" },
      { kind: "tool", name: "read", summary: "Read src/app.ts", input: "src/app.ts", output: "file body" },
      { kind: "image", mediaType: "image/png", ref: "img-1" },
      { kind: "compact", text: "Earlier turns summarised." },
      { kind: "notice", text: "background job finished", source: "async-result" },
      { kind: "taskResult", tasks: [task()] },
    ];

    const { container } = render(
      <MobileChatMessage id="msg-rich" role="assistant" content="" referenceParts={parts} />,
    );

    expect(screen.getByTestId("assistant-reference-body")).toBeInTheDocument();
    expect(container.textContent).toContain("Here is the answer.");
    expect(screen.getByTestId("thinking-block")).toHaveTextContent("First thought");
    expect(screen.getByTestId("reference-tool-part")).toHaveTextContent("Read src/app.ts");
    expect(screen.getByTestId("reference-image-unresolved")).toHaveTextContent("img-1");
    expect(screen.getByTestId("reference-compaction")).toHaveTextContent("Conversation compacted");
    expect(container.textContent).toContain("Earlier turns summarised.");
    expect(screen.getByTestId("reference-notice")).toHaveTextContent("Background result delivered");
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("Background task ended");
    expect(container.textContent).toContain("Explore the repository");
  });

  it("takes the turn's prose from its text parts and copies exactly that", async () => {
    const copySpy = vi.spyOn(clipboardModule, "copyTextToClipboard").mockResolvedValue(true);

    render(
      <MobileChatMessage
        id="msg-rich-copy"
        role="assistant"
        content=""
        referenceParts={[
          { kind: "text", text: "First paragraph." },
          { kind: "text", text: "Second paragraph." },
        ]}
      />,
    );

    expect(screen.getByTestId("assistant-reference-body")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("message-copy-button"));
    expect(copySpy).toHaveBeenCalledWith("First paragraph.\n\nSecond paragraph.");
  });

  it("draws a user turn's non-text parts under its bubble without repeating its prose", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-user-rich"
        role="user"
        content="look at this"
        referenceParts={[
          { kind: "text", text: "look at this" },
          { kind: "image", mediaType: "image/png", ref: "img-2" },
        ]}
      />,
    );

    expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("look at this");
    expect(screen.getByTestId("reference-image-unresolved")).toHaveTextContent("img-2");
    expect(container.textContent?.match(/look at this/g)).toHaveLength(1);
  });

  it("does not fabricate a body for a turn whose rich parts are empty", () => {
    render(<MobileChatMessage id="msg-empty-rich" role="assistant" content="" referenceParts={[]} />);

    expect(screen.queryByTestId("assistant-reference-body")).not.toBeInTheDocument();
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
    expect(screen.queryByTestId("message-copy-button")).not.toBeInTheDocument();
  });

  it("keeps the legacy prose presentation when no reference parts are given", () => {
    render(<MobileChatMessage id="msg-legacy" role="assistant" content="Legacy prose." />);

    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("Legacy prose.");
    expect(screen.queryByTestId("assistant-reference-body")).not.toBeInTheDocument();
  });

  it("marks a turn nobody typed for assistive technology", () => {
    render(
      <MobileChatMessage
        id="msg-runtime"
        role="assistant"
        content=""
        referenceSource="runtime"
        referenceParts={[{ kind: "text", text: "Background job finished." }]}
      />,
    );

    expect(screen.getByText("Runtime message")).toBeInTheDocument();
  });
});

describe("reference compaction disclosure", () => {
  afterEach(cleanup);

  it("labels the compaction and shows the summary it left", () => {
    render(<MobileChatMessage id="msg-compact" role="assistant" content="" referenceParts={[{ kind: "compact", text: "Twelve turns of setup." }]} />);

    const block = screen.getByTestId("reference-compaction");
    expect(block).toHaveTextContent("Conversation compacted");
    expect(block).toHaveTextContent("Twelve turns of setup.");
  });
});

describe("reference abandoned branch disclosure", () => {
  afterEach(cleanup);

  it("says how many turns and how many branches were walked away from", () => {
    expect(referenceAbandonedSummary({ count: 1, branches: 1 })).toBe(
      "1 earlier turn on a branch you navigated away from",
    );
    expect(referenceAbandonedSummary({ count: 4, branches: 1 })).toBe(
      "4 earlier turns on a branch you navigated away from",
    );
    expect(referenceAbandonedSummary({ count: 1, branches: 3 })).toBe(
      "1 earlier turn on 3 branches you navigated away from",
    );
    expect(referenceAbandonedSummary({ count: 6, branches: 2 })).toBe(
      "6 earlier turns on 2 branches you navigated away from",
    );
  });

  it("discloses the branch with the summary the transcript kept", () => {
    render(<ReferenceAbandonedBranchDisclosure abandoned={{ count: 3, branches: 1, summary: "We tried the other approach first." }} />);

    const block = screen.getByTestId("reference-abandoned");
    expect(block).toHaveTextContent("3 earlier turns on a branch you navigated away from");
    expect(block).toHaveTextContent("We tried the other approach first.");
  });

  it("falls back to the terminal instruction when the branch carries no summary", () => {
    render(<ReferenceAbandonedBranchDisclosure abandoned={{ count: 2, branches: 1 }} />);

    expect(screen.getByTestId("reference-abandoned")).toHaveTextContent(REFERENCE_ABANDONED_NOTE);
  });

  it("discloses the branch on the turn that carries it", () => {
    render(
      <MobileChatMessage
        id="msg-abandoned"
        role="assistant"
        content=""
        referenceAbandoned={{ count: 2, branches: 1, summary: "Walked away." }}
        referenceParts={[{ kind: "text", text: "Continuing on the chosen branch." }]}
      />,
    );

    expect(screen.getByTestId("reference-abandoned")).toHaveTextContent("2 earlier turns on a branch");
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("Continuing on the chosen branch.");
  });
});

describe("reference same-pane output disclosure", () => {
  afterEach(cleanup);

  it("labels a scrollback page with the reference's own words", () => {
    const { container } = render(
      <ReferenceHistoryDisclosure
        page={page({ source: "scrollback", availability: "scrollback", unavailableReason: "no native reader" })}
      />,
    );

    const banner = container.querySelector("[data-testid='reference-disclosure']");
    expect(banner).not.toBeNull();
    expect(banner).toHaveTextContent(REFERENCE_SCROLLBACK_DISCLOSURE);
  });

  it("labels a session that has not written a turn differently from scrollback", () => {
    const { container } = render(
      <ReferenceHistoryDisclosure
        page={page({ source: "omo-transcript", availability: "notStarted", unavailableReason: "session_not_written" })}
      />,
    );

    const banner = container.querySelector("[data-testid='reference-disclosure']");
    expect(banner).not.toBeNull();
    expect(banner?.textContent).not.toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
    expect(banner?.textContent ?? "").not.toBe("");
  });

  it("discloses nothing for a native page", () => {
    const { container } = render(<ReferenceHistoryDisclosure page={page()} />);

    expect(container.querySelector("[data-testid='reference-disclosure']")).toBeNull();
  });
});

describe("reference older page control", () => {
  afterEach(cleanup);

  it("labels its three states", () => {
    expect(referenceOlderControlLabel("idle")).toBe("Earlier messages");
    expect(referenceOlderControlLabel("loading")).toBe("Loading earlier messages…");
    expect(referenceOlderControlLabel("failed")).toBe("Couldn't load earlier messages — retry");
  });

  it("offers the older page, disables itself while loading and asks on click", () => {
    const onLoadOlder = vi.fn();
    const { rerender } = render(
      <ReferenceOlderPageControl hasOlder loadedOlder={false} state="idle" onLoadOlder={onLoadOlder} />,
    );

    const button = screen.getByTestId("reference-older-button");
    expect(button).toHaveTextContent("Earlier messages");
    fireEvent.click(button);
    expect(onLoadOlder).toHaveBeenCalledTimes(1);

    rerender(<ReferenceOlderPageControl hasOlder loadedOlder={false} state="loading" onLoadOlder={onLoadOlder} />);
    expect(screen.getByTestId("reference-older-button")).toBeDisabled();

    rerender(<ReferenceOlderPageControl hasOlder loadedOlder={false} state="failed" onLoadOlder={onLoadOlder} />);
    expect(screen.getByTestId("reference-older-button")).toHaveTextContent("Couldn't load earlier messages — retry");
  });

  it("marks the beginning of the conversation once an older page was loaded", () => {
    const { container, rerender } = render(
      <ReferenceOlderPageControl hasOlder={false} loadedOlder={false} />,
    );
    expect(container.firstChild).toBeNull();

    rerender(<ReferenceOlderPageControl hasOlder={false} loadedOlder />);
    expect(screen.getByTestId("reference-older-endcap")).toHaveTextContent("Beginning of conversation");
    expect(screen.queryByTestId("reference-older-button")).not.toBeInTheDocument();
  });
});

describe("reference tool part", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("opens onto the call's input, output, failure and cut-output request", () => {
    const onRequestWholeOutput = vi.fn();
    const { container } = render(
      <MobileChatMessage
        id="msg-tool"
        role="assistant"
        content=""
        referenceContext={{ onRequestWholeOutput, loadingOutputRef: null }}
        referenceParts={[
          {
            kind: "tool",
            name: "bash",
            summary: "",
            input: "cargo test",
            output: "test failed",
            error: true,
            outputRef: "out-1",
            outputSize: 4096,
          },
        ]}
      />,
    );

    const row = screen.getByTestId("reference-tool-part");
    expect(row).toHaveTextContent("Ran bash");
    expect(row).toHaveTextContent("cargo test");
    expect(row).toHaveTextContent("failed");
    expect(screen.queryByTestId("reference-tool-output")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.getByTestId("reference-tool-input")).toHaveTextContent("cargo test");
    expect(screen.getByTestId("reference-tool-output")).toHaveTextContent("test failed");

    const whole = screen.getByTestId("reference-tool-whole-output");
    expect(whole).toHaveTextContent("4096 characters");
    fireEvent.click(whole);
    expect(onRequestWholeOutput).toHaveBeenCalledWith("out-1");
    expect(container.textContent).toContain("failed");
  });

  it("reads as busy while the whole output is being fetched", () => {
    render(
      <MobileChatMessage
        id="msg-tool-loading"
        role="assistant"
        content=""
        referenceContext={{ loadingOutputRef: "out-2" }}
        referenceParts={[{ kind: "tool", name: "read", summary: "", input: "", output: "", outputRef: "out-2" }]}
      />,
    );

    fireEvent.click(screen.getByTestId("work-row"));
    const whole = screen.getByTestId("reference-tool-whole-output");
    expect(whole).toBeDisabled();
    expect(whole).toHaveTextContent("Loading the whole output…");
  });

  it("states what the recorded skill evidence does and does not prove", () => {
    expect(referenceSkillLabel({ name: "s", evidence: "invocation", status: "loaded" })).toBe("Skill invoked");
    expect(referenceSkillLabel({ name: "s", evidence: "invocation", status: "failed" })).toBe("Skill invocation failed");
    expect(referenceSkillLabel({ name: "s", evidence: "instructions", status: "loaded" })).toBe("Skill instructions loaded");
    expect(referenceSkillLabel({ name: "s", evidence: "instructions", status: "failed" })).toBe("Skill read failed");

    render(
      <MobileChatMessage
        id="msg-skill"
        role="assistant"
        content=""
        referenceParts={[
          {
            kind: "tool",
            name: "read",
            summary: "debugging",
            input: "/s/SKILL.md",
            output: "loaded",
            skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
          },
        ]}
      />,
    );

    const skill = screen.getByTestId("reference-skill");
    expect(skill).toHaveTextContent("debugging");
    expect(skill).toHaveTextContent("Skill instructions loaded");
    expect(skill).toHaveTextContent("does not confirm every step was followed");
  });

  it("lists a turn's skills without opening the tool row they were recorded on", () => {
    render(
      <MobileChatMessage
        id="msg-skill-folded"
        role="assistant"
        content=""
        durationLabel="3s"
        referenceParts={[
          {
            kind: "tool",
            name: "read",
            summary: "debugging",
            input: "/s/SKILL.md",
            output: "loaded",
            skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
          },
        ]}
      />,
    );

    expect(screen.getByTestId("worked-for-toggle")).toHaveAttribute("aria-expanded", "false");
    const row = screen.getByTestId("reference-tool-part").querySelector("[data-testid='work-row']");
    expect(row).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("reference-tool-input")).not.toBeInTheDocument();
    expect(screen.getByTestId("reference-turn-skills")).toHaveTextContent("debugging");
  });

  it("draws a standalone skill part in the turn's skill list and not inside the part list", () => {
    render(
      <MobileChatMessage
        id="msg-skill-part"
        role="assistant"
        content=""
        durationLabel="3s"
        referenceParts={[
          { kind: "text", text: "Loading the skill." },
          {
            kind: "skill",
            skill: { name: "lonely", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
          },
        ]}
      />,
    );

    // the chip is turn level: it stays visible while the work section is folded
    expect(screen.getByTestId("worked-for-toggle")).toHaveAttribute("aria-expanded", "false");
    const skills = screen.getByTestId("reference-turn-skills");
    expect(skills).toHaveTextContent("lonely");
    expect(skills).toHaveTextContent("Skill instructions loaded");
    expect(skills).toHaveTextContent("/s/SKILL.md");
    // the reference filters `skill` out of the parts it renders in order (ChatView.tsx:322)
    const partList = screen.getByTestId("reference-parts");
    expect(partList).toHaveTextContent("Loading the skill.");
    expect(partList).not.toHaveTextContent("lonely");
    expect(partList.querySelector("[data-testid='reference-skill']")).toBeNull();
  });

  it("resolves the images a tool returned through the owning host", () => {
    render(
      <MobileChatMessage
        id="msg-tool-images"
        role="assistant"
        content=""
        referenceContext={{ resolveImageUrl: (image) => `https://host/img/${image.ref}` }}
        referenceParts={[
          {
            kind: "tool",
            name: "read",
            summary: "Read the screenshot",
            input: "shot.png",
            output: "ok",
            images: [{ mediaType: "image/png", ref: "shot-1" }],
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByTestId("work-row"));
    const img = screen.getByTestId("reference-image");
    expect(img).toHaveAttribute("src", "https://host/img/shot-1");
    expect(img).toHaveAttribute("alt", "image/png");
  });
});

describe("reference turn skill list", () => {
  afterEach(cleanup);

  it("deduplicates a skill recorded twice on the same document, keeping the latest attempt", () => {
    const skills = referenceTurnSkills([
      {
        kind: "tool",
        name: "read",
        summary: "debugging",
        input: "a",
        output: "",
        skill: { name: "debugging", evidence: "instructions", status: "requested", path: "/s/SKILL.md" },
      },
      {
        kind: "tool",
        name: "read",
        summary: "debugging",
        input: "b",
        output: "",
        skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
      },
    ]);

    expect(skills).toHaveLength(1);
    expect(skills[0]?.status).toBe("loaded");
  });

  it("keeps an invocation and an instruction read of the same skill apart", () => {
    const skills = referenceTurnSkills([
      {
        kind: "tool",
        name: "Skill",
        summary: "debugging",
        input: "",
        output: "",
        skill: { name: "debugging", evidence: "invocation", status: "requested" },
      },
      {
        kind: "tool",
        name: "read",
        summary: "debugging",
        input: "",
        output: "",
        skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
      },
    ]);

    expect(skills.map((skill) => skill.evidence)).toEqual(["invocation", "instructions"]);
  });

  it("deduplicates a standalone skill part against the tool call that read the same document", () => {
    const skills = referenceTurnSkills([
      {
        kind: "tool",
        name: "read",
        summary: "debugging",
        input: "a",
        output: "",
        skill: { name: "debugging", evidence: "instructions", status: "requested", path: "/s/SKILL.md" },
      },
      {
        kind: "skill",
        skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
      },
    ]);

    expect(skills).toHaveLength(1);
    expect(skills[0]?.status).toBe("loaded");
  });

  it("lists a skill a user turn's parts recorded", () => {
    render(
      <MobileChatMessage
        id="msg-user-skill"
        role="user"
        content="use the debugging skill"
        referenceParts={[
          { kind: "text", text: "use the debugging skill" },
          {
            kind: "tool",
            name: "Skill",
            summary: "debugging",
            input: "",
            output: "",
            skill: { name: "debugging", evidence: "invocation", status: "requested" },
          },
        ]}
      />,
    );

    expect(screen.getByTestId("reference-turn-skills")).toHaveTextContent("Skill requested");
  });

  it("lists a standalone skill part on a user turn without drawing it in the bubble", () => {
    render(
      <MobileChatMessage
        id="msg-user-skill-part"
        role="user"
        content="/skill:debugging"
        referenceParts={[
          { kind: "text", text: "/skill:debugging" },
          {
            kind: "skill",
            skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
          },
        ]}
      />,
    );

    expect(screen.getByTestId("reference-turn-skills")).toHaveTextContent("debugging");
    // the bubble holds the prose and no extra part list, because the chip is drawn above it
    expect(screen.queryByTestId("reference-parts")).not.toBeInTheDocument();
  });

  it("lists nothing for a turn that recorded no skill", () => {
    render(
      <MobileChatMessage
        id="msg-no-skill"
        role="assistant"
        content=""
        referenceParts={[{ kind: "text", text: "No skills here." }]}
      />,
    );

    expect(screen.queryByTestId("reference-turn-skills")).not.toBeInTheDocument();
  });
});

describe("reference image part", () => {
  afterEach(cleanup);

  it("shows a labelled chip rather than guessing a URL", () => {
    const { container } = render(<ReferenceImagePart part={{ kind: "image", mediaType: "image/png", ref: "img-9" }} />);

    expect(screen.getByTestId("reference-image-unresolved")).toHaveTextContent("img-9");
    expect(screen.getByTestId("reference-image-unresolved")).toHaveTextContent("image/png");
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders the image the owning host resolved", () => {
    render(
      <ReferenceImagePart
        part={{ kind: "image", mediaType: "image/jpeg", ref: "img-10" }}
        context={{ resolveImageUrl: (image) => `https://host/img/${image.ref}` }}
      />,
    );

    expect(screen.getByTestId("reference-image")).toHaveAttribute("src", "https://host/img/img-10");
  });
});

describe("reference background task results", () => {
  afterEach(cleanup);

  it("summarises what a finished task reported about itself", () => {
    expect(
      referenceTaskResultMeta(
        task({ agent: "explore", model: "smol", durationMs: 2400, turns: 3, toolCalls: 4, tokens: 1200 }),
      ),
    ).toBe("explore · smol · 2.4s · 3 turns · 4 tool calls · 1200 tokens");

    expect(referenceTaskResultMeta(task({ turns: 1, toolCalls: 1, tokens: 0 }))).toBe("1 turn · 1 tool call");
    expect(referenceTaskStatusWord("completed")).toBe("done");
    expect(referenceTaskStatusWord("failed")).toBe("failed");
    expect(referenceTaskStatusWord("cancelled")).toBe("cancelled");
  });

  it("heads one ended task differently from several and shows each result", () => {
    const { rerender } = render(
      <ReferencePartList parts={[{ kind: "taskResult", tasks: [task()] }]} />,
    );
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("Background task ended");
    expect(screen.getByTestId("reference-task-result")).toHaveTextContent("The parser lives in src-tauri.");
    expect(screen.getByTestId("reference-task-result")).toHaveTextContent("done");

    rerender(
      <ReferencePartList
        parts={[
          {
            kind: "taskResult",
            tasks: [
              task({ id: "a", title: "First task" }),
              task({ id: "b", title: "Second task", status: "failed", result: "it broke", resultCut: true }),
            ],
          },
        ]}
      />,
    );

    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("2 background tasks ended");
    expect(screen.getAllByTestId("reference-task-result")).toHaveLength(2);
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent("it broke");
    expect(screen.getByTestId("reference-task-results")).toHaveTextContent(
      "This is the first part of a longer result.",
    );
  });

  it("says a task that reported nothing reported nothing", () => {
    render(<ReferencePartList parts={[{ kind: "taskResult", tasks: [task({ result: "   " })] }]} />);

    expect(screen.getByTestId("reference-task-result")).toHaveTextContent("The task reported no result.");
  });
});

describe("reference runtime notice", () => {
  afterEach(cleanup);

  it("labels a runtime notice by its own first line unless it is a background result", () => {
    expect(referenceNoticeLabel({ text: "job finished", source: "async-result" })).toBe(
      "Background result delivered",
    );
    expect(referenceNoticeLabel({ text: "job finished" })).toBe("Background result delivered");
    expect(referenceNoticeLabel({ text: "first line here\nsecond line", source: "codex" })).toBe("first line here");
  });

  it("cuts a long first line rather than letting it fill the row", () => {
    const label = referenceNoticeLabel({ text: `${"x".repeat(120)}\nrest`, source: "codex" });
    expect(label).toHaveLength(96);
    expect(label.endsWith("…")).toBe(true);
  });

  it("keeps the runtime's own words behind the disclosure", () => {
    render(
      <MobileChatMessage
        id="msg-notice"
        role="assistant"
        content=""
        referenceParts={[{ kind: "notice", text: "first line here\nsecond line", source: "codex" }]}
      />,
    );

    const notice = screen.getByTestId("reference-notice");
    expect(notice).toHaveTextContent("first line here");
    expect(notice.textContent).toContain("second line");
  });
});

describe("reference timestamp formatting", () => {
  it("prints an ISO instant and a numeric instant as a time, and nothing for nothing", () => {
    expect(formatReferenceTimestamp(null)).toBeNull();
    expect(formatReferenceTimestamp(undefined)).toBeNull();
    expect(formatReferenceTimestamp("")).toBeNull();
    expect(formatReferenceTimestamp("2026-10-06T00:00:00.000Z")).not.toBeNull();
    expect(formatReferenceTimestamp(1_700_000_000_000)).not.toBeNull();
  });

  it("leaves text it cannot read as a date alone", () => {
    expect(formatReferenceTimestamp("not a date")).toBe("not a date");
  });
});
