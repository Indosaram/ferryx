/**
 * Ported from the pinned upstream `src/components/PromptCard.tsx` behavior
 * (`devswha/herdr-web-ui` @ `54e5a1f6…`, MIT). Authored, NOT EXECUTED — execution override.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ReferencePromptCard, type ReferencePromptCardProps } from "./ReferencePromptCard";
import type { ReferencePrompt, ReferencePromptAnswerPayload } from "./referenceTypes";

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

function renderCard(overrides: Partial<ReferencePromptCardProps> = {}) {
  const onAnswer = overrides.onAnswer ?? accepted();
  const props: ReferencePromptCardProps = {
    prompt: prompt(),
    screenRevision: "rev-7",
    onAnswer,
    ...overrides,
  };
  const view = render(<ReferencePromptCard {...props} />);
  return { view, onAnswer, props };
}

describe("ReferencePromptCard", () => {
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

  it("sends the option the user pressed, bound to the card's prompt and screen revision", async () => {
    const { onAnswer } = renderCard();
    fireEvent.click(screen.getByTestId("reference-prompt-option-1"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]).toEqual({
      promptId: "p1",
      screenRevision: "rev-7",
      answer: { optionIndex: 1 },
    });
  });

  it("reaches its options by keyboard: each is a button, and a key press answers it", async () => {
    const { onAnswer } = renderCard();
    const option = screen.getByTestId("reference-prompt-option-0");
    expect(option.tagName).toBe("BUTTON");
    option.focus();
    fireEvent.keyDown(option, { key: "Enter" });
    fireEvent.click(option, { detail: 0 });
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
    fireEvent.click(option, { detail: 1 });
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
    const option = screen.getByTestId("reference-prompt-option-0");
    const down = new MouseEvent("pointerdown", { bubbles: true, cancelable: true });
    Object.defineProperty(down, "pointerType", { value: "touch" });
    fireEvent(option, down);
    fireEvent.click(option, { detail: 1 });
    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(onAnswered).toHaveBeenCalledWith(false);
  });

  it("does not take the focus on when the press did not start in the card", async () => {
    const onAnswered = vi.fn();
    renderCard({ onAnswered });
    const option = screen.getByTestId("reference-prompt-option-0");
    fireEvent.keyDown(option, { key: "Enter" });
    fireEvent.click(option, { detail: 0 });
    await waitFor(() => expect(onAnswered).toHaveBeenCalledTimes(1));
    expect(onAnswered).toHaveBeenCalledWith(false);
  });

  it("reports nothing when the answer was dropped (a stale screen, a newer card)", async () => {
    const onAnswered = vi.fn();
    const onAnswer = vi.fn(async () => false);
    renderCard({ onAnswer, onAnswered });
    fireEvent.click(screen.getByTestId("reference-prompt-option-0"), { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswered).not.toHaveBeenCalled();
  });

  it("says the screen moved and asks for a re-read when the answer is refused as stale", async () => {
    const onPromptChanged = vi.fn();
    const onAnswered = vi.fn();
    const onAnswer = vi.fn(async () => {
      throw { code: "REQUEST_CONFLICT", message: "staleScreen", retryable: true, details: null };
    });
    renderCard({ onAnswer, onPromptChanged, onAnswered });
    fireEvent.click(screen.getByTestId("reference-prompt-option-0"), { detail: 1 });
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent(
        "the prompt changed — re-read",
      ),
    );
    expect(onPromptChanged).toHaveBeenCalledTimes(1);
    expect(onAnswered).not.toHaveBeenCalled();
  });

  it("shows any other failure as its message, without asking for a re-read", async () => {
    const onPromptChanged = vi.fn();
    const onAnswer = vi.fn(async () => {
      throw new Error("transport lost");
    });
    renderCard({ onAnswer, onPromptChanged });
    fireEvent.click(screen.getByTestId("reference-prompt-option-0"), { detail: 1 });
    await waitFor(() =>
      expect(screen.getByTestId("reference-prompt-error")).toHaveTextContent("transport lost"),
    );
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
    expect(confirm).toHaveAttribute("role", "alert");
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
    expect(screen.getByTestId("reference-prompt-option-0").className).toContain("status-warning");
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
    expect(submit).toHaveTextContent("Submit");

    fireEvent.click(screen.getByTestId("reference-prompt-multi-2"));
    fireEvent.click(screen.getByTestId("reference-prompt-multi-0"));
    expect(submit).not.toBeDisabled();
    expect(submit).toHaveTextContent("Submit (2)");

    fireEvent.click(submit, { detail: 1 });
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndices: [0, 2] });
  });

  it("unchecks a choice the user presses again", () => {
    renderCard({ prompt: multi });
    const second = screen.getByTestId("reference-prompt-multi-1");
    fireEvent.click(second);
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent("Submit (1)");
    fireEvent.click(second);
    expect(screen.getByTestId("reference-prompt-submit")).toHaveTextContent("Submit");
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
    expect(input).toHaveAttribute("aria-label", "Custom answer");
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
        title: "Action Required",
        question: "Action Required",
        options: [{ label: "Approve" }, { label: "Decline" }],
      }),
      onAnswer,
    });
    expect(screen.queryByTestId("reference-prompt-option-0")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Approve/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
    expect(onAnswer.mock.calls[0]?.[0]?.answer).toEqual({ optionIndex: 0 });
    fireEvent.click(screen.getByRole("button", { name: /Decline/ }));
    await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(2));
    expect(onAnswer.mock.calls[1]?.[0]?.answer).toEqual({ optionIndex: 1 });
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

  it("labels a queued question as still working, and shows the reference body", () => {
    renderCard({
      prompt: prompt({ queued: "collapsed", body: "git push --force" }),
    });
    expect(screen.getByText(/the message box still talks to Codex/)).toBeInTheDocument();
    expect(screen.getByText("git push --force")).toBeInTheDocument();
  });

  it("says the pane is blocked when no reader knows its prompt", () => {
    renderCard({ prompt: prompt({ fallback: true, options: [] }) });
    expect(screen.getByTestId("reference-prompt-fallback")).toHaveTextContent(
      "answer it in the terminal",
    );
  });
});
