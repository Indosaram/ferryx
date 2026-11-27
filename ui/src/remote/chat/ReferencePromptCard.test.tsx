/**
 * Ported from the pinned upstream `src/components/PromptCard.tsx` behavior
 * (`devswha/herdr-web-ui` @ `54e5a1f6…`, MIT). Authored, NOT EXECUTED — execution override.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { userFacingError } from "../userFacingError";
import { ReferencePromptCard, type ReferencePromptCardProps } from "./ReferencePromptCard";
import { REFERENCE_PROMPT_STALE_MESSAGE } from "./referencePromptAnswer";
import { referenceAnswerAttachmentStore } from "./promptAttach/referencePromptAttach";
import type { ReferenceFileReceipt, ReferencePrompt, ReferencePromptAnswerPayload } from "./referenceTypes";

function prompt(overrides: Partial<ReferencePrompt> = {}): ReferencePrompt {
  return {
    id: "p1",
    agent: "claude",
    kind: "question",
    title: "Question",
    question: "Which model?",
    body: null,
    options: [
      { label: "LM-O (Recommended)", description: null },
      { label: "YCB-V", description: null },
    ],
    multiSelect: false,
    customOptionIndex: null,
    queued: null,
    steps: [],
    fallback: null,
    ...overrides,
  };
}

function accepted() {
  return vi.fn<(payload: ReferencePromptAnswerPayload) => Promise<boolean>>(
    async () => true,
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderCard(overrides: Partial<ReferencePromptCardProps> = {}) {
  const onAnswer = vi.mocked(overrides.onAnswer ?? accepted());
  const props: ReferencePromptCardProps = {
    prompt: prompt(),
    screenRevision: "rev-7",
    onAnswer,
    // the owner always names its pane; a test card names its own so the answer-set key is
    // stable and clearable between cases
    answerScope: "card-test",
    ...overrides,
  };
  const view = render(<ReferencePromptCard {...props} />);
  return { view, onAnswer, props };
}

/** Pick an option (a tap only selects it) and return the button that sends it. */
function pick(index: number): HTMLElement {
  fireEvent.click(screen.getByTestId(`reference-prompt-option-${index}`), { detail: 1 });
  return screen.getByTestId("reference-prompt-submit");
}

describe("ReferencePromptCard", () => {
  beforeEach(() => {
    referenceAnswerAttachmentStore.clearScope("card-test");
  });
  it("requires explicit confirmation before dismissing an ambiguous answer", async () => {
    const onDismissAmbiguous = vi.fn(async () => undefined);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    renderCard({
      prompt: prompt({ answerStatus: "outcomeUnknown" }),
      onDismissAmbiguous,
    });
    fireEvent.click(screen.getByTestId("reference-prompt-dismiss-ambiguous"));
    expect(onDismissAmbiguous).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByTestId("reference-prompt-dismiss-ambiguous"));
    await waitFor(() => expect(onDismissAmbiguous).toHaveBeenCalledTimes(1));
  });
  it("disables all answer choices for an ambiguous prompt", () => {
    renderCard({ prompt: prompt({ answerStatus: "outcomeUnknown" }) });
    expect(screen.getByTestId("reference-prompt-option-0")).toBeDisabled();
    expect(screen.getByTestId("reference-prompt-option-1")).toBeDisabled();
    expect(screen.getByTestId("reference-prompt-submit")).toBeDisabled();
  });
  beforeEach(cleanup);
  afterEach(cleanup);

  it("renders the prompt as a labelled region and numbers each option", () => {
    renderCard();
    const card = screen.getByTestId("reference-prompt-card");
    expect(card).toHaveAttribute("role", "region");
    expect(card).toHaveAttribute("aria-labelledby");
    expect(screen.getByText("Question")).toBeInTheDocument();
    expect(screen.getByText("Which model?")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-option-0")).toHaveTextContent("LM-O");
    expect(screen.getByTestId("reference-prompt-option-1")).toHaveTextContent("YCB-V");
    // the "Recommended" suffix becomes a tag, not part of the label
    expect(screen.getByTestId("reference-prompt-option-0")).toHaveTextContent("Recommended");
    expect(screen.queryByTestId("reference-prompt-custom-input")).not.toBeInTheDocument();
    expect(screen.queryByTestId("reference-prompt-confirm")).not.toBeInTheDocument();
  });

  it("only selects an option on tap: nothing is sent until Send answer", () => {
    const { onAnswer } = renderCard();
    const submit = screen.getByTestId("reference-prompt-submit");
    expect(submit).toBeDisabled();
    const option = screen.getByTestId("reference-prompt-option-1");
    expect(option).toHaveAttribute("role", "radio");
    expect(option).toHaveAttribute("aria-checked", "false");
    fireEvent.click(option, { detail: 1 });
    expect(option).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("reference-prompt-option-0")).toHaveAttribute("aria-checked", "false");
    expect(submit).not.toBeDisabled();
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("asks for Send answer before a standing permission (\"don't ask again\") goes out", () => {
    const { onAnswer } = renderCard({
      prompt: prompt({
        kind: "approval",
        options: [{ label: "Yes" }, { label: "Yes, and don't ask again" }, { label: "No" }],
      }),
    });
    fireEvent.click(screen.getByTestId("reference-prompt-option-1"), { detail: 1 });
    expect(onAnswer).not.toHaveBeenCalled();
    expect(screen.getByTestId("reference-prompt-option-1")).toHaveTextContent("Remembered for later requests");
  });

  it("sends the option the user picked, bound to the card's prompt and screen revision", async () => {
    const { onAnswer } = renderCard();
    fireEvent.click(pick(1), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]).toEqual({
      promptId: "p1",
      screenRevision: "rev-7",
      answer: { optionIndex: 1 },
    });
  });

  it("reaches its options by keyboard: each is a button, and a key press picks then sends", async () => {
    const { onAnswer } = renderCard();
    const option = screen.getByTestId("reference-prompt-option-0");
    expect(option.tagName).toBe("BUTTON");
    option.focus();
    fireEvent.keyDown(option, { key: "Enter" });
    fireEvent.click(option, { detail: 0 });
    expect(onAnswer).not.toHaveBeenCalled();
    const submit = screen.getByTestId("reference-prompt-submit");
    submit.focus();
    fireEvent.keyDown(submit, { key: "Enter" });
    fireEvent.click(submit, { detail: 0 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 0 });
  });

  it("shows the flight: the card is busy and its controls are disabled while the answer is out", async () => {
    let release: (value: boolean) => void = () => {};
    const onAnswer = vi.fn(
      () => new Promise<boolean>((resolve) => {
        release = resolve;
      }),
    );
    renderCard({ onAnswer });
    const option = screen.getByTestId("reference-prompt-option-0");
    fireEvent.click(pick(0), { detail: 1 });
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-card")).toHaveAttribute("aria-busy", "true"),
    );
    expect(option).toBeDisabled();
    release(true);
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-card")).toHaveAttribute("aria-busy", "false"),
    );
  });

  it("keeps the keyboard out of the message box after a touch press", async () => {
    const onAnswered = vi.fn();
    renderCard({ onAnswered });
    const submit = pick(0);
    const down = new MouseEvent("pointerdown", { bubbles: true, cancelable: true });
    Object.defineProperty(down, "pointerType", { value: "touch" });
    fireEvent(submit, down);
    fireEvent.click(submit, { detail: 1 });
    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(onAnswered).toHaveBeenCalledWith(false);
  });

  it("does not take the focus on when the press did not start in the card", async () => {
    const onAnswered = vi.fn();
    renderCard({ onAnswered });
    const submit = pick(0);
    fireEvent.keyDown(submit, { key: "Enter" });
    fireEvent.click(submit, { detail: 0 });
    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(onAnswered).toHaveBeenCalledWith(false);
  });

  it("reports nothing when the answer was dropped (a stale screen, a newer card)", async () => {
    const onAnswered = vi.fn();
    const onAnswer = vi.fn(async () => false);
    renderCard({ onAnswer, onAnswered });
    fireEvent.click(pick(0), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswered).not.toHaveBeenCalled();
  });

  it("lets the user answer again when a still-shown card's answer was dropped", async () => {
    const flight = deferred<boolean>();
    const onAnswer = vi.fn(() => flight.promise);
    renderCard({ onAnswer });
    fireEvent.click(pick(0), { detail: 1 });
    expect(screen.getByTestId("reference-prompt-card")).toHaveAttribute("aria-busy", "true");
    await act(async () => {
      flight.resolve(false);
      await flight.promise;
    });
    expect(screen.getByTestId("reference-prompt-card")).toHaveAttribute("aria-busy", "false");
    expect(screen.getByTestId("reference-prompt-option-0")).not.toBeDisabled();
    expect(screen.getByTestId("reference-prompt-option-1")).not.toBeDisabled();
  });

  it("says the screen moved and asks for a re-read when the answer is refused as stale", async () => {
    const onPromptChanged = vi.fn();
    const onAnswered = vi.fn();
    const onAnswer = vi.fn(async () => {
      throw { code: "REQUEST_CONFLICT", message: "staleScreen", retryable: true, details: null };
    });
    renderCard({ onAnswer, onPromptChanged, onAnswered });
    fireEvent.click(pick(0), { detail: 1 });
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent(
        REFERENCE_PROMPT_STALE_MESSAGE,
      ),
    );
    expect(onPromptChanged).toHaveBeenCalledTimes(1);
    expect(onAnswered).not.toHaveBeenCalled();
  });

  it("shows any other failure in plain language, never the raw transport text, without asking for a re-read", async () => {
    const onPromptChanged = vi.fn();
    const onAnswer = vi.fn(async () => {
      throw new Error("transport lost");
    });
    renderCard({ onAnswer, onPromptChanged });
    fireEvent.click(pick(0), { detail: 1 });
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent(
        userFacingError(new Error("transport lost"), { action: "send your answer" }),
      ),
    );
    expect(screen.getByTestId("reference-prompt-error")).not.toHaveTextContent("transport lost");
    expect(onPromptChanged).not.toHaveBeenCalled();
  });

  it("shows a caller-observed error on the card", () => {
    renderCard({ error: "refused: unsupported" });
    expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent("refused: unsupported");
  });
});

describe("ReferencePromptCard typed approval confirmation", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  const typed = { optionIndex: 0 };

  it("sends the typed pick only on Confirm, and clears it afterwards", async () => {
    const onAnswer = accepted();
    const onTypedAnswerDone = vi.fn();
    renderCard({ onAnswer, typedAnswer: typed, onTypedAnswerDone });
    const confirm = screen.getByTestId("reference-prompt-confirm");
    expect(confirm).toHaveAttribute("role", "group");
    expect(document.activeElement).toBe(screen.getByTestId("reference-prompt-confirm-send"));
    expect(confirm).toHaveTextContent("Send 1. LM-O (Recommended)?");
    expect(onAnswer).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 0 });
    await waitFor(() => expect(onTypedAnswerDone).toHaveBeenCalledTimes(1));
  });

  it("cancels the typed pick without sending anything", () => {
    const onAnswer = accepted();
    const onTypedAnswerDone = vi.fn();
    renderCard({ onAnswer, typedAnswer: typed, onTypedAnswerDone });
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-cancel"));
    expect(onTypedAnswerDone).toHaveBeenCalledTimes(1);
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("marks the picked option as the typed one", () => {
    renderCard({ typedAnswer: typed });
    expect(screen.getByTestId("reference-prompt-option-0")).toHaveAttribute("aria-checked", "true");
  });

  it("does not clear the pick when the answer was dropped", async () => {
    const onTypedAnswerDone = vi.fn();
    const onAnswer = vi.fn(async () => false);
    renderCard({ onAnswer, typedAnswer: typed, onTypedAnswerDone });
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onTypedAnswerDone).not.toHaveBeenCalled();
  });
});

describe("ReferencePromptCard typed confirm failure", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("keeps the typed pick for a retry when Confirm fails on the transport", async () => {
    const flight = deferred<boolean>();
    const onAnswer = vi.fn(() => flight.promise);
    const onTypedAnswerDone = vi.fn();
    renderCard({ onAnswer, typedAnswer: { optionIndex: 0 }, onTypedAnswerDone });
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await act(async () => {
      flight.reject(new Error("host 503"));
      await flight.promise.catch(() => undefined);
    });
    expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent(/Couldn't send your answer/);
    expect(onTypedAnswerDone).not.toHaveBeenCalled();
    expect(screen.getByTestId("reference-prompt-confirm-send")).not.toBeDisabled();
  });

  it("still clears the typed pick when Confirm is refused as stale", async () => {
    const flight = deferred<boolean>();
    const onAnswer = vi.fn(() => flight.promise);
    const onTypedAnswerDone = vi.fn();
    renderCard({ onAnswer, typedAnswer: { optionIndex: 0 }, onTypedAnswerDone });
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await act(async () => {
      flight.reject({ code: "REQUEST_CONFLICT", message: "staleScreen", retryable: true, details: null });
      await flight.promise.catch(() => undefined);
    });
    expect(onTypedAnswerDone).toHaveBeenCalledTimes(1);
  });
});

describe("ReferencePromptCard multiple choice", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  const multi = prompt({
    multiSelect: true,
    question: "Pick some",
    options: [{ label: "A" }, { label: "B" }, { label: "C" }],
  });

  it("submits the chosen numbers in order, and refuses an empty choice", async () => {
    const onAnswer = accepted();
    renderCard({ prompt: multi, onAnswer });
    const submit = screen.getByTestId("reference-prompt-submit");
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent("Send answer");

    fireEvent.click(screen.getByTestId("reference-prompt-multi-2"));
    fireEvent.click(screen.getByTestId("reference-prompt-multi-0"));
    expect(submit).not.toBeDisabled();
    expect(submit).toHaveTextContent("Send answer (2)");

    fireEvent.click(submit, { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndices: [0, 2] });
  });

  it("unchecks a choice the user presses again", () => {
    renderCard({ prompt: multi });
    const second = screen.getByTestId("reference-prompt-multi-1");
    fireEvent.click(second);
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent("Send answer (1)");
    fireEvent.click(second);
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent(/^Send answer$/);
    expect(screen.getByTestId("reference-prompt-submit")).toBeDisabled();
  });
});

describe("ReferencePromptCard custom answer", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  const withCustom = prompt({
    customOptionIndex: 2,
    options: [{ label: "LM-O" }, { label: "YCB-V" }, { label: "Type something" }],
  });

  it("keeps the custom row out of the options and sends trimmed text on Send", async () => {
    const onAnswer = accepted();
    renderCard({ prompt: withCustom, onAnswer });
    expect(screen.queryByTestId("reference-prompt-option-2")).not.toBeInTheDocument();
    expect(screen.getByText("Or type your own answer")).toBeInTheDocument();

    const send = screen.getByTestId("reference-prompt-custom-send");
    expect(send).toBeDisabled();
    const input = screen.getByTestId("reference-prompt-custom-input");
    fireEvent.change(input, { target: { value: "  use T-LESS instead  " } });
    expect(send).not.toBeDisabled();
    fireEvent.click(send, { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "use T-LESS instead" });
  });

  it("sends the typed reply on Enter, but not while an IME is composing", async () => {
    const onAnswer = accepted();
    renderCard({ prompt: withCustom, onAnswer });
    const input = screen.getByTestId("reference-prompt-custom-input");
    fireEvent.change(input, { target: { value: "안녕" } });

    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(onAnswer).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(onAnswer).not.toHaveBeenCalled();
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(onAnswer).not.toHaveBeenCalled();
    fireEvent.compositionEnd(input);

    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "안녕" });
  });

  it("answers a free-form prompt through the custom row alone", async () => {
    const onAnswer = accepted();
    renderCard({
      prompt: prompt({ options: [{ label: "Type your reply" }], customOptionIndex: 0 }),
      onAnswer,
    });
    expect(screen.queryByTestId("reference-prompt-option-0")).not.toBeInTheDocument();
    const input = screen.getByTestId("reference-prompt-custom-input");
    expect(input).toHaveAttribute("aria-label", "Your answer");
    fireEvent.change(input, { target: { value: "keep the logs" } });
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "keep the logs" });
  });
});

describe("ReferencePromptCard existing shells and disclosures", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("uses the existing approval shell for a two-option approval", async () => {
    const onAnswer = accepted();
    renderCard({
      prompt: prompt({
        kind: "approval",
        title: "Needs your approval",
        question: "Needs your approval",
        options: [{ label: "Approve" }, { label: "Decline" }],
      }),
      onAnswer,
    });
    // one card: the title is drawn once
    expect(screen.getAllByText("Needs your approval")).toHaveLength(1);
    expect(screen.queryByTestId("reference-prompt-option-0")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Approve/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 0 });
    fireEvent.click(screen.getByRole("button", { name: /Decline/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(2));
    expect(onAnswer.mock.calls[1]?.[0]?.answer).toEqual({ optionIndex: 1 });
  });

  it("keeps the keyboard out of the message box after a tap on the approval shell", async () => {
    const onAnswered = vi.fn();
    renderCard({
      prompt: prompt({
        kind: "approval",
        title: "Needs your approval",
        question: "Needs your approval",
        options: [{ label: "Approve" }, { label: "Decline" }],
      }),
      onAnswered,
    });
    const approve = screen.getByRole("button", { name: /Approve/ });
    const down = new MouseEvent("pointerdown", { bubbles: true, cancelable: true });
    Object.defineProperty(down, "pointerType", { value: "touch" });
    fireEvent(approve, down);
    // Android Chrome focuses the tapped button, so the press starts inside the card
    approve.focus();
    fireEvent.click(approve, { detail: 1 });
    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(onAnswered).toHaveBeenCalledWith(false);
  });

  it("lists the questions of a multi-step prompt and marks the answered ones", () => {
    renderCard({
      prompt: prompt({
        steps: [
          { label: "Which models?", answered: true, current: false },
          { label: "Which dataset?", answered: false, current: true },
        ],
      }),
    });
    expect(screen.getByLabelText("Questions")).toBeInTheDocument();
    expect(screen.getByText("Which models?")).toBeInTheDocument();
    expect(screen.getByText("Which dataset?")).toBeInTheDocument();
    expect(screen.getByText("(answered)")).toBeInTheDocument();
  });

  it("labels a queued question as still working with its own agent's name, and shows the reference body", () => {
    renderCard({
      prompt: prompt({ queued: "collapsed", body: "git push --force" }),
    });
    expect(screen.getByText(/the message box still talks to Claude/)).toBeInTheDocument();
    expect(screen.queryByText(/Codex/)).not.toBeInTheDocument();
    expect(screen.getByText("git push --force")).toBeInTheDocument();
  });

  it("says the pane is blocked when no reader knows its prompt", () => {
    renderCard({ prompt: prompt({ fallback: true, options: [] }) });
    expect(screen.getByTestId("reference-prompt-fallback")).toHaveTextContent(
      "Answer it in the terminal",
    );
  });
});

describe("ReferencePromptCard ambiguous dismissal without a handler", () => {
  it("renders no dismiss button when the caller passes no onDismissAmbiguous", () => {
    renderCard({ prompt: prompt({ answerStatus: "outcomeUnknown" }) });
    expect(screen.queryByTestId("reference-prompt-dismiss-ambiguous")).toBeNull();
  });
});

describe("ReferencePromptCard answer attachments", () => {
  const clearSets = () => {
    referenceAnswerAttachmentStore.clearScope("card-test");
  };
  beforeEach(() => {
    cleanup();
    clearSets();
  });
  afterEach(() => {
    cleanup();
    clearSets();
  });

  const customPrompt = (overrides: Partial<ReferencePrompt> = {}): ReferencePrompt =>
    prompt({
      options: [{ label: "Other" }, { label: "Add tests" }, { label: "Ship as is" }],
      customOptionIndex: 0,
      ...overrides,
    });

  function staged(name: string, bytes = 4, type = "text/plain"): File {
    return new File([new Uint8Array(bytes)], name, { type });
  }

  function stagedReceipt(name: string): ReferenceFileReceipt {
    return {
      receipt: {
        hostId: "h1",
        attachmentId: `att-${name}`,
        sha256: "ab",
        sizeBytes: 4,
        mediaType: "text/plain",
      },
      displayName: name,
      mentionText: `@uploads/${name} `,
    };
  }

  async function attachThrough(
    uploader: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>,
    files: File[],
  ): Promise<void> {
    const input = screen.getByTestId("reference-answer-attach-input");
    await act(async () => {
      fireEvent.change(input, { target: { files } });
    });
    await waitFor(() => expect(uploader).toHaveBeenCalledTimes(1));
  }

  it("shows no attach control without the upload prop (an older server)", () => {
    renderCard({ prompt: customPrompt() });
    expect(screen.queryByTestId("reference-answer-attach")).not.toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-custom-input")).toBeInTheDocument();
  });

  it("shows no attach control on a question with only predefined choices", () => {
    renderCard({ prompt: prompt(), onAttachAnswerFiles: vi.fn(async (files) => files.map(() => stagedReceipt("x"))) });
    expect(screen.queryByTestId("reference-answer-attach")).not.toBeInTheDocument();
  });

  it("uploads picked files before the answer can go out, then sends their paths as mentions", async () => {
    const gate = deferred<ReferenceFileReceipt[]>();
    const uploader = vi.fn(async () => gate.promise);
    const onAnswer = accepted();
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader, onAnswer });
    const input = screen.getByTestId("reference-prompt-custom-input");
    await act(async () => {
      fireEvent.change(input, { target: { value: "use these" } });
    });
    await attachThrough(uploader, [staged("a.txt"), staged("b.txt")]);
    // the upload is in flight: nothing may leave yet
    expect(uploader).toHaveBeenCalledWith([expect.any(File), expect.any(File)]);
    expect(screen.getByTestId("reference-prompt-custom-send")).toBeDisabled();
    await act(async () => {
      gate.resolve([stagedReceipt("a.txt"), stagedReceipt("b.txt")]);
    });
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({
      customText: "use these @uploads/a.txt @uploads/b.txt",
    });
  });

  it("takes an image pasted into the answer field", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader });
    const input = screen.getByTestId("reference-prompt-custom-input");
    const image = staged("shot.png", 4, "image/png");
    await act(async () => {
      fireEvent.paste(input, { clipboardData: { files: [image] } });
    });
    await waitFor(() => expect(uploader).toHaveBeenCalledTimes(1));
    expect(uploader).toHaveBeenCalledWith([image]);
  });

  it("holds a failed upload out of the send, and retry sends it again", async () => {
    const uploader = vi
      .fn<(files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>>()
      .mockRejectedValueOnce(new Error("host unreachable"))
      .mockResolvedValue([stagedReceipt("a.txt")]);
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader });
    await attachThrough(uploader, [staged("a.txt")]);
    await waitFor(() => expect(uploader).toHaveBeenCalledTimes(1));
    await waitFor(() => {
      const failed = screen
        .getAllByTestId(/^reference-answer-attachment-/)
        .find((each) => each.getAttribute("data-status") === "failed");
      expect(failed).toBeTruthy();
    });
    expect(screen.getByTestId("reference-prompt-custom-send")).toBeDisabled();
    const retry = screen
      .getAllByRole("button")
      .find((each) => each.getAttribute("data-testid")?.startsWith("reference-answer-attachment-retry-"));
    expect(retry).toBeTruthy();
    fireEvent.click(retry as HTMLElement);
    await waitFor(() => expect(uploader).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
  });

  it("lets the user remove a failed upload and answer without it", async () => {
    const uploader = vi.fn(async () => {
      throw new Error("host unreachable");
    });
    const onAnswer = accepted();
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader, onAnswer });
    await attachThrough(uploader, [staged("a.txt")]);
    const remove = await waitFor(() => {
      const button = screen
        .getAllByRole("button")
        .find((each) => each.getAttribute("data-testid")?.startsWith("reference-answer-attachment-remove-"));
      expect(button).toBeTruthy();
      return button as HTMLElement;
    });
    fireEvent.click(remove);
    const input = screen.getByTestId("reference-prompt-custom-input");
    await act(async () => {
      fireEvent.change(input, { target: { value: "no files" } });
    });
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "no files" });
  });

  it("keeps the typed answer and the files when a submit fails", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    const onAnswer = vi.fn<(payload: ReferencePromptAnswerPayload) => Promise<boolean>>(async () => {
      throw new Error("transport down");
    });
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader, onAnswer });
    await attachThrough(uploader, [staged("a.txt")]);
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    const input = screen.getByTestId("reference-prompt-custom-input");
    await act(async () => {
      fireEvent.change(input, { target: { value: "still here" } });
    });
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(screen.getByTestId("reference-prompt-error")).toBeInTheDocument());
    expect((input as HTMLInputElement).value).toBe("still here");
    expect(
      screen
        .getAllByTestId(/^reference-answer-attachment-/)
        .filter((each) => each.getAttribute("data-status") === "ready"),
    ).toHaveLength(1);
  });

  it("keeps each question's answer set: another prompt sees none of it, the same one still does", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    const { view } = renderCard({
      prompt: customPrompt({ id: "pq1" }),
      onAttachAnswerFiles: uploader,
    });
    await attachThrough(uploader, [staged("kept.txt")]);
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    view.unmount();
    const other = renderCard({ prompt: customPrompt({ id: "pq2" }), onAttachAnswerFiles: uploader });
    expect(screen.queryByTestId("reference-answer-attach")).toBeInTheDocument();
    // another question's answer set carries none of this one's files
    expect(
      other.view.container.querySelectorAll('[data-testid^="reference-answer-attachment-"][data-status]'),
    ).toHaveLength(0);
    other.view.unmount();
    const again = renderCard({ prompt: customPrompt({ id: "pq1" }), onAttachAnswerFiles: uploader });
    expect(
      again.view.container.querySelectorAll('[data-testid^="reference-answer-attachment-"][data-status]'),
    ).toHaveLength(1);
  });

  it("sends a file by itself as its saved path alone", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    const onAnswer = accepted();
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader, onAnswer });
    await attachThrough(uploader, [staged("only.txt")]);
    await waitFor(() => expect(screen.getByTestId("reference-prompt-custom-send")).toBeEnabled());
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "@uploads/only.txt" });
  });

  it("refuses a file over the composer's size limit without uploading it", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader });
    const input = screen.getByTestId("reference-answer-attach-input");
    const huge = staged("huge.png", 11 * 1024 * 1024, "image/png");
    await act(async () => {
      fireEvent.change(input, { target: { files: [huge] } });
    });
    expect(screen.getByTestId("reference-prompt-error")).toBeInTheDocument();
    expect(uploader).not.toHaveBeenCalled();
  });

  it("keeps an upload that finishes after the user moved to another question", async () => {
    const gate = deferred<ReferenceFileReceipt[]>();
    const uploader = vi.fn(async () => gate.promise);
    const { view } = renderCard({ prompt: customPrompt({ id: "pq1" }), onAttachAnswerFiles: uploader });
    await attachThrough(uploader, [staged("late.txt")]);
    view.unmount();
    await act(async () => {
      gate.resolve([stagedReceipt("late.txt")]);
    });
    const again = renderCard({ prompt: customPrompt({ id: "pq1" }), onAttachAnswerFiles: uploader });
    expect(
      again.view.container.querySelectorAll('[data-testid^="reference-answer-attachment-"][data-status="ready"]'),
    ).toHaveLength(1);
  });

  it("sends a picked option together with the attached files as one custom answer", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    const onAnswer = accepted();
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader, onAnswer });
    await attachThrough(uploader, [staged("a.txt")]);
    fireEvent.click(screen.getByTestId("reference-prompt-option-1"), { detail: 1 });
    await waitFor(() => expect(screen.getByTestId("reference-prompt-submit")).toBeEnabled());
    fireEvent.click(screen.getByTestId("reference-prompt-submit"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({
      customText: "2 @uploads/a.txt",
    });
  });

  it("keeps the same question's answer set apart between two panes", async () => {
    // a prompt id is a content hash: the identical question on another host/session is the
    // same id, and must not find the first pane's files
    const firstUpload = vi.fn(async (files: readonly File[]) =>
      files.map((each) => stagedReceipt(each.name)),
    );
    const first = renderCard({
      prompt: customPrompt({ id: "same" }),
      answerScope: "hostA|owner|epoch|session-1",
      onAttachAnswerFiles: firstUpload,
    });
    await attachThrough(firstUpload, [staged("secret.txt")]);
    await waitFor(() => expect(firstUpload).toHaveBeenCalledTimes(1));
    first.view.unmount();

    const secondUpload = vi.fn(async (files: readonly File[]) =>
      files.map((each) => stagedReceipt(each.name)),
    );
    const onAnswer = accepted();
    const second = renderCard({
      prompt: customPrompt({ id: "same" }),
      answerScope: "hostB|owner|epoch|session-2",
      onAttachAnswerFiles: secondUpload,
      onAnswer,
    });
    expect(second.view.container.querySelectorAll("[data-status]")).toHaveLength(0);
    await act(async () => {
      fireEvent.change(screen.getByTestId("reference-prompt-custom-input"), {
        target: { value: "x" },
      });
    });
    fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ customText: "x" });
    referenceAnswerAttachmentStore.clearScope("hostA|owner|epoch|session-1");
    referenceAnswerAttachmentStore.clearScope("hostB|owner|epoch|session-2");
  });

  it("holds the typed-answer Confirm while an upload is in flight", async () => {
    const gate = deferred<ReferenceFileReceipt[]>();
    const uploader = vi.fn(async () => gate.promise);
    const onAnswer = accepted();
    renderCard({
      prompt: customPrompt(),
      onAttachAnswerFiles: uploader,
      onAnswer,
      typedAnswer: { optionIndex: 1 },
    });
    await attachThrough(uploader, [staged("a.txt")]);
    expect(screen.getByTestId("reference-prompt-confirm-send")).toBeDisabled();
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    expect(onAnswer).not.toHaveBeenCalled();
    await act(async () => {
      gate.resolve([stagedReceipt("a.txt")]);
    });
    await waitFor(() => expect(screen.getByTestId("reference-prompt-confirm-send")).toBeEnabled());
  });

  it("sends the typed-answer Confirm's ready attachments instead of dropping them", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    const onAnswer = accepted();
    renderCard({
      prompt: customPrompt(),
      onAttachAnswerFiles: uploader,
      onAnswer,
      typedAnswer: { optionIndex: 1 },
    });
    await attachThrough(uploader, [staged("a.txt")]);
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-confirm-send")).toBeEnabled(),
    );
    fireEvent.click(screen.getByTestId("reference-prompt-confirm-send"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({
      customText: "2 @uploads/a.txt",
    });
  });

  it("offers no tab stop of its own for the file input", () => {
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: vi.fn(async () => []) });
    const input = screen.getByTestId("reference-answer-attach-input");
    expect(input).toHaveAttribute("hidden");
    expect((input as HTMLInputElement).tabIndex).toBe(-1);
  });

  it("accepts only as many files as the server stages in one turn", async () => {
    const uploader = vi.fn(async (files: readonly File[]) => files.map((each) => stagedReceipt(each.name)));
    renderCard({ prompt: customPrompt(), onAttachAnswerFiles: uploader });
    const input = screen.getByTestId("reference-answer-attach-input");
    const five = [0, 1, 2, 3, 4].map((index) => staged(`f${index}.txt`, 1));
    await act(async () => {
      fireEvent.change(input, { target: { files: five } });
    });
    expect(uploader).toHaveBeenCalledWith([expect.any(File), expect.any(File), expect.any(File), expect.any(File)]);
    expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent("at most 4");
    expect(document.querySelectorAll('[data-testid^="reference-answer-attachment-"][data-status]')).toHaveLength(4);
  });
});

describe("ReferencePromptCard approval keyboard shortcuts", () => {
  beforeEach(() => {
    cleanup();
    referenceAnswerAttachmentStore.clearScope("card-test");
  });
  afterEach(() => {
    cleanup();
    referenceAnswerAttachmentStore.clearScope("card-test");
  });

  const approval = (overrides: Partial<ReferencePrompt> = {}): ReferencePrompt =>
    prompt({
      kind: "approval",
      title: "Run command?",
      question: "Run command?",
      options: [{ label: "Allow once" }, { label: "Allow for session" }, { label: "Decline" }],
      ...overrides,
    });

  it("answers a two-option approval with its number keys", async () => {
    const onAnswer = accepted();
    renderCard({
      prompt: approval({ options: [{ label: "Allow once" }, { label: "Decline" }] }),
      onAnswer,
    });
    fireEvent.keyDown(document, { key: "1" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 0 });
    fireEvent.keyDown(document, { key: "2" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(2));
    expect(onAnswer.mock.calls[1]?.[0]?.answer).toEqual({ optionIndex: 1 });
  });

  it("answers Allow once / Allow for session / Decline with 1 / 2 / 3", async () => {
    const onAnswer = accepted();
    renderCard({ prompt: approval(), onAnswer });
    for (const [digit, index] of [["1", 0], ["2", 1], ["3", 2]] as const) {
      fireEvent.keyDown(document, { key: digit });
      await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(index + 1));
      expect(onAnswer.mock.calls[index]?.[0]?.answer).toEqual({ optionIndex: index });
    }
  });

  it("never takes a number typed in the answer field as an option pick", async () => {
    const onAnswer = accepted();
    renderCard({
      prompt: approval({
        customOptionIndex: 2,
        options: [{ label: "Allow once" }, { label: "Decline" }, { label: "Other" }],
      }),
      onAnswer,
    });
    const input = screen.getByTestId("reference-prompt-custom-input");
    input.focus();
    fireEvent.keyDown(input, { key: "1" });
    fireEvent.keyDown(input, { key: "2" });
    expect(onAnswer).not.toHaveBeenCalled();
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("only toggles a multiple choice with the number keys, and ignores a number past the options", () => {
    const onAnswer = accepted();
    renderCard({
      prompt: prompt({ multiSelect: true, options: [{ label: "A" }, { label: "B" }] }),
      onAnswer,
    });
    fireEvent.keyDown(document, { key: "1" });
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent("Send answer (1)");
    fireEvent.keyDown(document, { key: "3" });
    expect(onAnswer).not.toHaveBeenCalled();
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent("Send answer (1)");
  });

  it("does not take a number for a card in a hidden pane", async () => {
    // panes stay mounted while hidden (`DesktopChatPane`'s `hidden={!visible}`): the digit
    // belongs to the pane the user is looking at, and this one is not it
    const onAnswer = accepted();
    render(
      <div hidden>
        <ReferencePromptCard
          prompt={approval()}
          screenRevision="rev-7"
          onAnswer={onAnswer}
        />
      </div>,
    );
    fireEvent.keyDown(document, { key: "1" });
    await act(async () => {});
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("answers only one of two cards on the page with one key press", async () => {
    // every mounted card hears the document's keydown: one digit must still answer once
    const first = accepted();
    const second = accepted();
    render(
      <>
        <ReferencePromptCard prompt={approval({ id: "c1" })} screenRevision="rev-7" onAnswer={first} />
        <ReferencePromptCard prompt={approval({ id: "c2" })} screenRevision="rev-7" onAnswer={second} />
      </>,
    );
    fireEvent.keyDown(document.body, { key: "1" });
    await act(async () => {});
    expect(first.mock.calls.length + second.mock.calls.length).toBe(1);
  });

  it("ignores a digit under a modifier, in an IME, in a text field and in editable content", async () => {
    const onAnswer = accepted();
    render(
      <>
        <ReferencePromptCard prompt={approval()} screenRevision="rev-7" onAnswer={onAnswer} />
        <textarea data-testid="composer-field" />
        <div data-testid="editable" contentEditable />
      </>,
    );
    fireEvent.keyDown(document.body, { key: "1", metaKey: true });
    fireEvent.keyDown(document.body, { key: "1", ctrlKey: true });
    fireEvent.keyDown(document.body, { key: "1", altKey: true });
    fireEvent.keyDown(document.body, { key: "1", isComposing: true });
    fireEvent.keyDown(document.body, { key: "1", keyCode: 229 });
    fireEvent.keyDown(screen.getByTestId("composer-field"), { key: "1" });
    fireEvent.keyDown(screen.getByTestId("editable"), { key: "1" });
    const prevented = new KeyboardEvent("keydown", { key: "1", bubbles: true, cancelable: true });
    prevented.preventDefault();
    document.body.dispatchEvent(prevented);
    await act(async () => {});
    expect(onAnswer).not.toHaveBeenCalled();
    // and the plain digit still answers
    fireEvent.keyDown(document.body, { key: "3" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 2 });
  });

  it("takes a number again once the pane is shown", async () => {
    const onAnswer = accepted();
    const { container } = render(
      <div hidden>
        <ReferencePromptCard
          prompt={approval()}
          screenRevision="rev-7"
          onAnswer={onAnswer}
        />
      </div>,
    );
    const wrapper = container.firstElementChild as HTMLElement;
    wrapper.hidden = false;
    fireEvent.keyDown(document, { key: "1" });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
  });
});
