/**
 * The independent falsifier's cases for lane G6, kept in the lane so the repairs stay pinned.
 * Each `describe` names the claim it falsified and each `F<n>` its verdict line in
 * `.evidence/chat-t3-parity-20261125/verify/g6/VERDICT.md`.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ReferencePromptCard, type ReferencePromptCardProps } from "../ReferencePromptCard";
import { referenceAnswerTextWithAttachments } from "../referencePromptAnswer";
import { planReferenceFileSend, REFERENCE_FILE_LIMITS } from "../referenceFiles";
import {
  PROMPT_ATTACH_MAX_FILES,
  ReferenceAnswerAttachmentStore,
  referenceAnswerAttachmentStore,
} from "./referencePromptAttach";
import type { ReferenceFileReceipt, ReferencePrompt, ReferencePromptAnswerPayload } from "../referenceTypes";

function prompt(overrides: Partial<ReferencePrompt> = {}): ReferencePrompt {
  return {
    id: "fz",
    agent: "claude",
    kind: "question",
    title: "Question",
    question: "Which?",
    body: null,
    options: [{ label: "Other" }, { label: "A" }, { label: "B" }],
    multiSelect: false,
    customOptionIndex: 0,
    queued: null,
    steps: [],
    fallback: null,
    ...overrides,
  };
}
const approval = (id: string): ReferencePrompt =>
  prompt({ id, kind: "approval", title: "Run?", question: "Run?", customOptionIndex: null,
    options: [{ label: "Allow once" }, { label: "Allow for session" }, { label: "Decline" }] });

function receiptFor(host: string, name: string): ReferenceFileReceipt {
  return {
    receipt: { hostId: host, attachmentId: `${host}-${name}`, sha256: "ab", sizeBytes: 4, mediaType: "text/plain" },
    displayName: name,
    mentionText: `@${host}/uploads/${name} `,
  };
}
const file = (name: string, bytes = 4, type = "text/plain") => new File([new Uint8Array(bytes)], name, { type });
const accepted = () => vi.fn<(p: ReferencePromptAnswerPayload) => Promise<boolean>>(async () => true);

function card(props: Partial<ReferencePromptCardProps> & { prompt: ReferencePrompt }) {
  return <ReferencePromptCard screenRevision="r1" onAnswer={accepted()} {...props} />;
}

async function attach(container: HTMLElement, files: File[]) {
  const input = within(container).getByTestId("reference-answer-attach-input");
  await act(async () => {
    fireEvent.change(input, { target: { files } });
  });
}

beforeEach(() => {
  cleanup();
  for (const id of ["fz", "same", "a1", "a2", "h1"]) referenceAnswerAttachmentStore.clear(id);
});
afterEach(cleanup);

describe("claim 5: digit shortcuts must not fire on the wrong card", () => {
  it("F1 one '1' keypress answers only one of two prompt cards on the page", async () => {
    const a = accepted();
    const b = accepted();
    render(<>{card({ prompt: approval("a1"), onAnswer: a })}{card({ prompt: approval("a2"), onAnswer: b })}</>);
    fireEvent.keyDown(document.body, { key: "1" });
    await act(async () => {});
    expect(a.mock.calls.length + b.mock.calls.length).toBe(1);
  });

  it("F2 a card in a hidden (background) pane does not take a digit", async () => {
    const onAnswer = accepted();
    render(<div hidden>{card({ prompt: approval("h1"), onAnswer })}</div>);
    fireEvent.keyDown(document.body, { key: "1" });
    await act(async () => {});
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("F3 modifiers, IME, defaultPrevented, contenteditable and the main composer textarea are ignored", async () => {
    const onAnswer = accepted();
    render(<>{card({ prompt: approval("a1"), onAnswer })}<textarea data-testid="composer" /><div data-testid="ce" contentEditable /></>);
    fireEvent.keyDown(document.body, { key: "1", metaKey: true });
    fireEvent.keyDown(document.body, { key: "1", ctrlKey: true });
    fireEvent.keyDown(document.body, { key: "1", altKey: true });
    fireEvent.keyDown(document.body, { key: "1", isComposing: true });
    fireEvent.keyDown(document.body, { key: "1", keyCode: 229 });
    fireEvent.keyDown(screen.getByTestId("composer"), { key: "1" });
    fireEvent.keyDown(screen.getByTestId("ce"), { key: "1" });
    const prevented = new KeyboardEvent("keydown", { key: "1", bubbles: true, cancelable: true });
    prevented.preventDefault();
    document.body.dispatchEvent(prevented);
    await act(async () => {});
    expect(onAnswer).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "3" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 2 });
  });
});

describe("claim 1: answer sets stay separate", () => {
  it("F4 the same prompt id in another session/host must not inherit the first session's files", async () => {
    const hostA = vi.fn(async (fs: readonly File[]) => fs.map((f) => receiptFor("hostA", f.name)));
    const first = render(card({ prompt: prompt({ id: "same" }), onAttachAnswerFiles: hostA }));
    await attach(first.container, [file("secret.txt")]);
    await waitFor(() => expect(hostA).toHaveBeenCalledTimes(1));
    first.unmount();
    // another pane: a different session on hostB showing the identical (content-hashed) prompt
    const hostB = vi.fn(async (fs: readonly File[]) => fs.map((f) => receiptFor("hostB", f.name)));
    const onAnswer = accepted();
    const second = render(card({ prompt: prompt({ id: "same" }), onAttachAnswerFiles: hostB, onAnswer }));
    expect(second.container.querySelectorAll('[data-status]')).toHaveLength(0);
    fireEvent.change(within(second.container).getByTestId("reference-prompt-custom-input"), { target: { value: "x" } });
    fireEvent.click(within(second.container).getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "x" });
  });
});

describe("claim 2: submit is held for uploads on every path", () => {
  it("F5 the typed-answer Confirm is blocked while an upload is in flight", async () => {
    let release!: (r: readonly ReferenceFileReceipt[]) => void;
    const uploader = vi.fn(() => new Promise<readonly ReferenceFileReceipt[]>((res) => { release = res; }));
    const onAnswer = accepted();
    const view = render(card({ prompt: prompt(), onAttachAnswerFiles: uploader, onAnswer, typedAnswer: { optionIndex: 1 } }));
    await attach(view.container, [file("a.txt")]);
    expect(screen.getByTestId("reference-prompt-confirm-send")).toBeDisabled();
    await act(async () => release([receiptFor("h", "a.txt")]));
  });

  it("F6 the typed-answer Confirm does not silently drop ready attachments", async () => {
    const uploader = vi.fn(async (fs: readonly File[]) => fs.map((f) => receiptFor("h", f.name)));
    const onAnswer = accepted();
    const view = render(card({ prompt: prompt(), onAttachAnswerFiles: uploader, onAnswer, typedAnswer: { optionIndex: 1 } }));
    await attach(view.container, [file("a.txt")]);
    await waitFor(() => expect(view.container.querySelector('[data-status="ready"]')).not.toBeNull());
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(JSON.stringify(onAnswer.mock.calls[0]?.[0]?.answer)).toContain("@h/uploads/a.txt");
  });

  it("F7 removing an in-flight upload is not resurrected when it resolves", async () => {
    let release!: (r: readonly ReferenceFileReceipt[]) => void;
    const uploader = vi.fn(() => new Promise<readonly ReferenceFileReceipt[]>((res) => { release = res; }));
    const view = render(card({ prompt: prompt(), onAttachAnswerFiles: uploader }));
    await attach(view.container, [file("a.txt")]);
    const remove = view.container.querySelector('[data-testid^="reference-answer-attachment-remove-"]') as HTMLElement;
    fireEvent.click(remove);
    await act(async () => release([receiptFor("h", "a.txt")]));
    expect(view.container.querySelectorAll("[data-status]")).toHaveLength(0);
    expect(screen.getByTestId("reference-prompt-custom-send")).toBeDisabled();
  });

  it("F8 a failed send keeps text and files; a dropped (false) answer keeps them too", async () => {
    const uploader = vi.fn(async (fs: readonly File[]) => fs.map((f) => receiptFor("h", f.name)));
    const onAnswer = vi.fn(async () => false);
    const view = render(card({ prompt: prompt(), onAttachAnswerFiles: uploader, onAnswer }));
    await attach(view.container, [file("a.txt")]);
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    fireEvent.change(screen.getByTestId("reference-prompt-custom-input"), { target: { value: "keep" } });
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect((screen.getByTestId("reference-prompt-custom-input") as HTMLInputElement).value).toBe("keep");
    expect(view.container.querySelectorAll('[data-status="ready"]')).toHaveLength(1);
  });
});

describe("claim 3: mention format equals planReferenceFileSend", () => {
  it("F9 same text as the composer for several drafts", () => {
    const receipts = [receiptFor("h", "a.txt"), receiptFor("h", "b[1].png"), receiptFor("h", "c(2).pdf")];
    const paths = receipts.map((r) => r.mentionText.slice(1).trim());
    for (const draft of ["", "use", "use ", "line\nnext"]) {
      expect(referenceAnswerTextWithAttachments(draft, paths)).toBe(planReferenceFileSend(draft, receipts).text.trim());
    }
  });
  it("F10 a mention with a space is refused like the composer refuses it", () => {
    expect(() => planReferenceFileSend("x", [{ ...receiptFor("h", "a"), mentionText: "@my file.txt " }])).toThrow();
    expect(referenceAnswerTextWithAttachments("x", ["my file.txt"])).toBeNull();
  });
});

describe("claim 4: limits", () => {
  it("F11 exactly the server's per-turn cap fits, the next file is refused", () => {
    const store = new ReferenceAnswerAttachmentStore();
    // the cap is the server's `ATTACHMENT_MAX_FILES_PER_TURN`, not t3code's docs figure:
    // `reference_file_bounds_check` refuses the fifth file of a turn
    expect(PROMPT_ATTACH_MAX_FILES).toBe(4);
    const files = Array.from({ length: PROMPT_ATTACH_MAX_FILES + 1 }, (_, i) => file(`f${i}.txt`, 1));
    const { added, refusals } = store.add("k", files, { ...REFERENCE_FILE_LIMITS, maxTurnBytes: 1e9 });
    expect(added).toHaveLength(PROMPT_ATTACH_MAX_FILES);
    expect(refusals).toHaveLength(1);
    expect(refusals[0]?.reason).toContain("at most " + PROMPT_ATTACH_MAX_FILES);
  });
  it("F12 exactly maxFileBytes and exactly maxTurnBytes are accepted; one byte more is refused", () => {
    const limits = { ...REFERENCE_FILE_LIMITS, maxFileBytes: 10, maxTurnBytes: 20 };
    const store = new ReferenceAnswerAttachmentStore();
    expect(store.add("k", [file("a.txt", 10)], limits).added).toHaveLength(1);
    expect(store.add("k", [file("b.txt", 11)], limits).refusals).toHaveLength(1);
    expect(store.add("k", [file("c.txt", 10)], limits).added).toHaveLength(1);
    expect(store.add("k", [file("d.txt", 1)], limits).refusals).toHaveLength(1);
    expect(store.add("j", [file("e.exe", 1, "application/x-msdownload")], limits).refusals).toHaveLength(1);
  });
  it("F13 a choice-only question and a multi-select show no attach control", () => {
    const up = vi.fn(async () => []);
    render(<>{card({ prompt: prompt({ id: "a1", customOptionIndex: null }), onAttachAnswerFiles: up })}
      {card({ prompt: prompt({ id: "a2", multiSelect: true, customOptionIndex: null }), onAttachAnswerFiles: up })}</>);
    expect(screen.queryByTestId("reference-answer-attach")).toBeNull();
  });
  it("F14 the hidden file input is not a separate unlabeled tab stop", () => {
    render(card({ prompt: prompt(), onAttachAnswerFiles: vi.fn(async () => []) }));
    const input = screen.getByTestId("reference-answer-attach-input");
    const hidden = input.tabIndex === -1 || input.getAttribute("aria-hidden") === "true";
    const named = input.hasAttribute("aria-label") || input.hasAttribute("aria-labelledby");
    expect(hidden || named).toBe(true);
  });
});

describe("evidence: option + files on a plan card", () => {
  it("E1 picking option 1 with a file sends custom text '2 @path', not the option", async () => {
    const uploader = vi.fn(async (fs: readonly File[]) => fs.map((f) => receiptFor("h", f.name)));
    const onAnswer = accepted();
    const view = render(card({ prompt: prompt({ kind: "plan" }), onAttachAnswerFiles: uploader, onAnswer }));
    await attach(view.container, [file("a.txt")]);
    await waitFor(() => expect(view.container.querySelector('[data-status="ready"]')).not.toBeNull());
    fireEvent.keyDown(document.body, { key: "2" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "2 @h/uploads/a.txt" });
  });
});
