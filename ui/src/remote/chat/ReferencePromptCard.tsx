/**
 * The prompt card for the Herdr reference chat (task 10).
 *
 * Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
 * (`src/components/PromptCard.tsx` with its `PromptCard.css`, MIT — see
 * `docs/chat/HERDR_LICENSE`), rendered with this repo's chat visual vocabulary (`cn` and
 * the `chat-*` / `status-*` tokens) and with the existing approval shell
 * (`ApprovalActionCard` in `./MobileChatComponents`) where the prompt is the two-option
 * accept/decline shape that component models.
 *
 * One card is one occurrence of a prompt on one pane: the owner mounts a new card (a `key`)
 * for the next prompt, the same question asked again included. So the card's picks, text,
 * scroll and an answer still on its way never pass to another prompt, and an answer that
 * comes back after the card is gone reports nothing.
 *
 * Presentational: the transport is the integration owner's (task 12). The card awaits
 * `onAnswer`, so the flight, the stale refusal and the focus hand-off stay its own.
 */
import React, {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { AlertCircle, Check, Loader2, Send } from "lucide-react";
import { cn } from "../../lib/cn";
import { haptic } from "../haptics";
import { userFacingError } from "../userFacingError";
import { isImeConfirmingKey } from "./imeKeys";
import { referenceMentionPathIsSafe, referenceMentionPathOf } from "./referenceFiles";
import type { ReferenceFileReceipt } from "./referenceTypes";
import { ReferenceAnswerAttachments } from "./promptAttach/ReferenceAnswerAttachments";
import {
  promptAttachmentDraftKey,
  promptAttachUploadScope,
  referenceAnswerAttachmentStore,
} from "./promptAttach/referencePromptAttach";
import { ApprovalActionCard } from "./MobileChatComponents";
import {
  referenceAnswerTextWithAttachments,
  REFERENCE_PROMPT_STALE_MESSAGE,
  referenceFocusFollowsAnswer,
  referencePressOrigin,
  referencePromptErrorIsStale,
  type ReferencePressOrigin,
  type ReferenceTypedAnswer,
} from "./referencePromptAnswer";
import {
  referenceSelectableIndices,
  type ReferencePrompt,
  type ReferencePromptAnswer,
  type ReferencePromptAnswerPayload,
} from "./referenceTypes";

/** The click that asked for an answer, as the card's own handlers see it. */
export interface ReferencePromptPress {
  readonly nativeEvent: Event;
  readonly currentTarget: EventTarget;
}

export interface ReferencePromptCardProps {
  readonly prompt: ReferencePrompt;
  /** The screen revision the card was rendered from; an answer names it, so a moved screen is refused. */
  readonly screenRevision: string;
  /** Send one answer. Resolves `false` when the answer was dropped (a stale screen, a newer card). */
  readonly onAnswer: (
    payload: ReferencePromptAnswerPayload,
    press?: ReferencePromptPress,
  ) => Promise<boolean>;
  /** The answer was refused because the screen moved: re-read the prompt now. */
  readonly onPromptChanged?: () => void;
  readonly onDismissAmbiguous?: () => Promise<void>;
  /** An answer went out; `toMessageBox`: the keyboard's focus was in the card and may go on to the message box. */
  readonly onAnswered?: (toMessageBox: boolean) => void;
  /**
   * Stage the picked files on the environment the thread runs on and return their staged
   * receipts — the same receipt type the composer's staged files use. Absent where the
   * server cannot take answer attachments (t3code: "older servers"): the card then shows
   * no attach control and takes no pasted images.
   */
  readonly onAttachAnswerFiles?: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>;
  /**
   * The pane's stable identity, for the answer-attachment set's key: the session id, or
   * `referenceTargetKey` of the remote target. Prompt ids are content hashes, so without it
   * the same question in another session or on another host shares one set of files.
   */
  readonly answerScope?: string;
  /** An option picked by a typed message, waiting in the card for Confirm. */
  readonly typedAnswer?: ReferenceTypedAnswer | null;
  readonly onTypedAnswerDone?: () => void;
  /** A failure the caller already observed for this card (a transport error, its own refusal). */
  readonly error?: string | null;
  readonly className?: string;
}

/** The agent by the name the user knows it by; anything unknown is just "the agent". */
function agentName(agent: string): string {
  switch (agent.toLowerCase()) {
    case "claude":
      return "Claude";
    case "codex":
      return "Codex";
    case "omo":
      return "OmO";
    default:
      return "The agent";
  }
}

function queuedCopy(agent: string, queued: "open" | "collapsed"): string {
  const name = agentName(agent);
  return queued === "open"
    ? `${name} keeps working meanwhile. Answer here; the question holds the terminal's input until it is answered or closed.`
    : `${name} keeps working meanwhile. Answer here; the message box still talks to ${name === "The agent" ? "the agent" : name}.`;
}

/** An option that turns a one-off answer into a standing permission. */
const REMEMBERS_CHOICE = /\b(always|don'?t ask again|for this session|remember)\b/i;

const FALLBACK_HINT =
  "This session is waiting, but Ferryx can't read its question. Answer it in the terminal.";

/**
 * Can a key pressed anywhere on the page be read as this card's? Only while the card is in
 * the pane the user is looking at: panes stay mounted while hidden (`DesktopChatPane`'s
 * `hidden={!visible}`, `TerminalSplitView`'s `hidden` tab containers), and a card in a
 * background pane must not take the digit the user meant for the visible one (t3code:
 * `!isCollapsed && !backgroundThread`).
 */
function cardIsOnScreen(card: HTMLElement | null): boolean {
  if (card === null || card.closest('[hidden], [inert], [aria-hidden="true"]') !== null) {
    return false;
  }
  const view = card.ownerDocument.defaultView;
  if (view === null) return true;
  // `hidden` as a class (`display: none`) and `invisible` (`visibility: hidden`) leave no
  // ancestor attribute to look for, so ask what the browser actually lays out.
  for (let node: HTMLElement | null = card; node !== null; node = node.parentElement) {
    const style = view.getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden") return false;
  }
  return true;
}

export function ReferencePromptCard({
  prompt,
  screenRevision,
  onAnswer,
  onPromptChanged,
  onDismissAmbiguous,
  onAnswered,
  typedAnswer = null,
  onTypedAnswerDone,
  onAttachAnswerFiles,
  answerScope,
  error = null,
  className,
}: ReferencePromptCardProps) {
  const [selected, setSelected] = useState<readonly number[]>([]);
  const [custom, setCustom] = useState("");
  const [pending, setPending] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  // the option a tap picked: nothing is sent until "Send answer"
  const [picked, setPicked] = useState<number | null>(null);
  const cardRef = useRef<HTMLElement | null>(null);
  const confirmButtonRef = useRef<HTMLButtonElement | null>(null);
  const composing = useRef(false);
  // the control Enter or Space last went down on: the click that follows on it is that key's
  const keyed = useRef<EventTarget | null>(null);
  // and the button a pointer last went down on, with what it was: a click can misname its pointer
  const down = useRef<{ target: EventTarget; pointerType: string } | null>(null);
  // false once the card is gone: its prompt was replaced, or its pane left
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);

  // a typed pick waits on Confirm: the focus goes there, so the reader is not scrolled away
  const typedIndex = typedAnswer?.optionIndex ?? null;
  useEffect(() => {
    if (typedIndex !== null) confirmButtonRef.current?.focus({ preventScroll: true });
  }, [typedIndex]);

  // Answer attachments (t3code question-attachments parity): one set per question, kept
  // while the user moves between questions, never touching the message box's draft. The key
  // is the pane's identity plus the question's: a prompt id is a content hash, so the same
  // question asked elsewhere must not find this pane's files. The fallback scope is pinned on
  // the card's first render, so an owner that re-creates its upload callback inline (a new
  // function each render) does not move the key out from under a set that is uploading.
  const [fallbackScope] = useState(() => promptAttachUploadScope(onAttachAnswerFiles));
  const attachKey = promptAttachmentDraftKey(
    prompt.id,
    answerScope != null && answerScope.length > 0 ? answerScope : fallbackScope,
  );
  const attachments = useSyncExternalStore(
    referenceAnswerAttachmentStore.subscribe,
    () => referenceAnswerAttachmentStore.get(attachKey),
  );
  const uploadsBusy = attachments.some((each) => each.status === "uploading");
  const uploadsFailed = attachments.some((each) => each.status === "failed");
  // An upload still in flight, or one that failed and was not retried or removed, holds
  // the answer back (t3code: "wait for uploads to finish before submitting").
  const answerHeldForUploads = attachments.length > 0 && (uploadsBusy || uploadsFailed);
  const attachmentsReady = attachments.length > 0 && !uploadsBusy && !uploadsFailed;

  const runUploads = useCallback(
    async (
      added: readonly { readonly id: string; readonly file: File }[],
      upload: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>,
      key: string,
    ) => {
      const ids = added.map((each) => each.id);
      // The answer set outlives the card (the user may move to another question while it
      // uploads), so the outcome always lands in the store, mounted card or not.
      try {
        const receipts = await upload(added.map((each) => each.file));
        referenceAnswerAttachmentStore.markReady(key, ids, receipts);
      } catch (cause) {
        referenceAnswerAttachmentStore.markFailed(
          key,
          ids,
          userFacingError(cause, { action: "upload these files" }),
        );
      }
    },
    [],
  );

  const addAnswerFiles = useCallback(
    (files: readonly File[]) => {
      if (onAttachAnswerFiles === undefined || files.length === 0) return;
      const { added, refusals } = referenceAnswerAttachmentStore.add(attachKey, files);
      if (refusals.length > 0) {
        setLocalError(refusals.map((each) => each.reason).join(" "));
      }
      if (added.length > 0) void runUploads(added, onAttachAnswerFiles, attachKey);
    },
    [attachKey, onAttachAnswerFiles, runUploads],
  );

  const retryAnswerUpload = useCallback(
    (id: string) => {
      if (onAttachAnswerFiles === undefined) return;
      const attachment = referenceAnswerAttachmentStore.get(attachKey).find((each) => each.id === id);
      if (attachment === undefined || attachment.status !== "failed") return;
      referenceAnswerAttachmentStore.markRetrying(attachKey, id);
      void runUploads([attachment], onAttachAnswerFiles, attachKey);
    },
    [attachKey, onAttachAnswerFiles, runUploads],
  );

  /**
   * The custom text an answer turns in when files are attached: the typed (or option
   * number) text with each uploaded file's saved path appended as an `@path ` mention.
   * A mention the staged receipt does not vouch for refuses the send and keeps the draft.
   */
  const composeAnswerWithAttachments = useCallback(
    (baseText: string): string | null => {
      const receipts = referenceAnswerAttachmentStore.readyReceipts(attachKey);
      const paths: string[] = [];
      for (const receipt of receipts) {
        const path = referenceMentionPathOf(receipt.mentionText);
        if (path === null || !referenceMentionPathIsSafe(path)) {
          setLocalError(
            "An attached file's saved path cannot be added to the answer. Remove it and attach the file again.",
          );
          return null;
        }
        paths.push(path);
      }
      return referenceAnswerTextWithAttachments(baseText, paths);
    },
    [attachKey],
  );

  const answer = useCallback(
    // `pressFelt`: the approval card already gave the press its own haptic
    async (
      choice: ReferencePromptAnswer,
      press?: ReferencePromptPress,
      pressFelt = false,
    ): Promise<boolean> => {
      // read now: the pressed control is disabled while the answer is on its way, and loses the focus
      const fromCard = cardRef.current?.contains(document.activeElement) === true;
      const click = press?.nativeEvent as Partial<PointerEvent> | undefined;
      const origin: ReferencePressOrigin = referencePressOrigin(
        press === undefined
          ? undefined
          : {
              pointerType: click?.pointerType,
              downType:
                down.current?.target === press.currentTarget ? down.current.pointerType : undefined,
              detail: click?.detail,
              keyed: keyed.current === press.currentTarget,
            },
      );
      keyed.current = null;
      down.current = null;
      // the device's own pointer, asked only for a press that does not say what made it
      const coarse = window.matchMedia?.("(pointer: coarse)").matches === true;
      setPending(true);
      setLocalError(null);
      try {
        const accepted = await onAnswer({ promptId: prompt.id, screenRevision, answer: choice }, press);
        if (!shown.current) return false;
        if (accepted === false) {
          // dropped, but the card is still up: it can be answered again
          setPending(false);
          return false;
        }
        if (!pressFelt) haptic("success");
        // the answer's attachments belong to it now: the next answer starts an empty set
        referenceAnswerAttachmentStore.clear(attachKey);
        // and read again: the answer took a moment, and the user may have gone on to something else
        const card = cardRef.current;
        const active = document.activeElement;
        onAnswered?.(
          referenceFocusFollowsAnswer({
            fromCard,
            origin,
            coarse,
            cardMounted: card !== null,
            inCard: card?.contains(active) === true,
            onPage: active === null || active === document.body,
          }),
        );
      } catch (cause) {
        if (!shown.current) return false;
        if (referencePromptErrorIsStale(cause)) {
          setLocalError(REFERENCE_PROMPT_STALE_MESSAGE);
          onPromptChanged?.();
        } else {
          setLocalError(userFacingError(cause, { action: "send your answer" }));
          // a transport failure keeps a typed pick for a retry
          setPending(false);
          return false;
        }
      }
      setPending(false);
      return true;
    },
    [onAnswer, onAnswered, onPromptChanged, prompt.id, screenRevision, attachKey],
  );

  const toggle = (index: number): void => {
    setSelected((current) =>
      current.includes(index) ? current.filter((each) => each !== index) : [...current, index],
    );
  };

  /** A picked option goes out on its own, or with the attachments' mentions as its text. */
  const submitChoice = useCallback(
    (index: number, press?: ReferencePromptPress, pressFelt = false): void => {
      if (attachmentsReady) {
        const composed = composeAnswerWithAttachments(String(index + 1));
        if (composed === null) return;
        void answer({ customText: composed }, press, pressFelt);
        return;
      }
      void answer({ optionIndex: index }, press, pressFelt);
    },
    [answer, attachmentsReady, composeAnswerWithAttachments],
  );


  const titleId = useId();
  const customLabelId = useId();
  const confirmLabelId = useId();
  const selectableIndices = referenceSelectableIndices(prompt);
  const customIndex = prompt.customOptionIndex != null ? prompt.customOptionIndex : null;
  const hasChoices = selectableIndices.length > 0;
  // Claude renders the menu's own pick as "Redis (Recommended)": a tag reads better than the suffix
  const labelOf = (label: string): { text: string; recommended: boolean } => {
    const text = label.replace(/\s*\(recommended\)$/i, "");
    return { text, recommended: text !== label };
  };
  const optionContent = (index: number): React.ReactNode => {
    const option = prompt.options[index];
    const { text, recommended } = labelOf(option?.label ?? "");
    return (
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-baseline gap-x-1.5">
          <span className="break-words">{text}</span>
          {recommended && (
            <span className="shrink-0 rounded border border-chat-border px-1 text-xs text-chat-foreground-secondary">
              Recommended
            </span>
          )}
        </span>
        {option?.description != null && (
          <span className="block text-xs text-chat-foreground-secondary break-words">
            {option.description}
          </span>
        )}
        {REMEMBERS_CHOICE.test(text) && (
          <span className="block text-xs text-status-warning">Remembered for later requests</span>
        )}
      </span>
    );
  };
  const optionNumber = (index: number): React.ReactNode => (
    <span className="shrink-0 pt-0.5 font-mono text-xs text-chat-foreground-secondary">
      <span aria-hidden="true">{index + 1}</span>
      <span className="sr-only">{index + 1}.</span>
    </span>
  );

  // The two-option approval keeps its own Approve / Decline pair. Anything richer (Codex's
  // three-option approval, a plan with a "tell Claude what to change" row, a multi-select) is the
  // option list below: a tap only picks, and "Send answer" sends.
  const twoOptionApproval =
    prompt.kind === "approval" &&
    !prompt.multiSelect &&
    customIndex === null &&
    selectableIndices.length === 2
      ? selectableIndices
      : null;
  const approvalQuestion = prompt.question === prompt.title ? null : prompt.question;
  const shownError = localError ?? error ?? null;
  const pickedIndex = picked ?? typedAnswer?.optionIndex ?? null;

  // Number-key answers, t3code parity (ComposerPendingUserInputPanel's digit shortcuts and
  // the approval card's Allow once / Allow for session / Decline): 1–9 pick an option while
  // the question is answerable. A single-select goes out right away, as t3code's digit
  // shortcut answers it; a multiple choice only toggles. Nothing fires from a text field
  // (type "1" into the answer freely), from an IME-confirming Enter, or under a modifier.
  useEffect(() => {
    if (
      pending ||
      prompt.answerStatus === "outcomeUnknown" ||
      prompt.fallback === true ||
      typedAnswer != null ||
      answerHeldForUploads
    ) {
      return;
    }
    const handler = (event: KeyboardEvent): void => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
      if (isImeConfirmingKey(event as unknown as React.KeyboardEvent, false)) return;
      const digit = Number.parseInt(event.key, 10);
      if (Number.isNaN(digit) || digit < 1 || digit > 9) return;
      const index = digit - 1;
      if (!selectableIndices.includes(index)) return;
      const target = event.target;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement
      ) {
        return;
      }
      if (
        target instanceof HTMLElement &&
        target.closest(
          '[contenteditable]:not([contenteditable="false"]), [role="dialog"], [aria-modal="true"], [role="menu"], [role="combobox"], [data-slot$="popup"]',
        )
      ) {
        return;
      }
      // A pane keeps its card mounted while it is hidden: only the card the user can see
      // may take a digit, or the background pane answers the question in front of them.
      if (!cardIsOnScreen(cardRef.current)) return;
      event.preventDefault();
      if (prompt.multiSelect) toggle(index);
      else submitChoice(index);
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- re-arms on any answerability change
  }, [
    pending,
    prompt.answerStatus,
    prompt.fallback,
    prompt.multiSelect,
    typedAnswer,
    answerHeldForUploads,
    selectableIndices,
    submitChoice,
  ]);

  const bodyBlock =
    prompt.body != null && prompt.body.length > 0 ? (
      <pre className="rounded-lg border border-chat-border bg-chat-screen p-3 font-mono text-[0.8125rem] leading-relaxed text-chat-foreground whitespace-pre-wrap break-words">
        {prompt.body}
      </pre>
    ) : null;

  return (
    <section
      ref={cardRef}
      role="region"
      aria-labelledby={titleId}
      aria-busy={pending}
      data-testid="reference-prompt-card"
      className={cn(
        "rounded-xl border border-chat-border bg-chat-surface p-3.5 my-2.5 space-y-3",
        className,
      )}
      onKeyDown={(event) => {
        down.current = null;
        keyed.current = event.key === "Enter" || event.key === " " ? event.target : null;
      }}
      onPointerDown={(event) => {
        keyed.current = null;
        const button = (event.target as Element).closest("button");
        down.current =
          button === null ? null : { target: button, pointerType: event.pointerType };
      }}
    >
      <header>
        <span className="sr-only">Input needed: </span>
        <h2
          id={titleId}
          className="flex items-start gap-2 text-base font-semibold text-chat-foreground break-words"
        >
          <AlertCircle aria-hidden="true" className="mt-1 size-4 shrink-0 text-status-warning" />
          <span className="min-w-0">{prompt.title}</span>
        </h2>
      </header>

      {prompt.steps != null && prompt.steps.length > 0 && (
        <ol className="space-y-1" aria-label="Questions">
          {prompt.steps.map((step, index) => (
            <li
              key={index}
              aria-current={step.current ? "step" : undefined}
              className={cn(
                "flex items-center gap-2 text-sm",
                step.answered ? "text-chat-foreground-secondary" : "text-chat-foreground",
                step.current && "font-medium",
              )}
            >
              <span
                aria-hidden="true"
                className="flex size-5 shrink-0 items-center justify-center rounded-full border border-chat-border font-mono text-xs"
              >
                {step.answered ? <Check className="size-3" /> : index + 1}
              </span>
              <span className="truncate">{step.label}</span>
              {step.answered && <span className="sr-only">(answered)</span>}
            </li>
          ))}
        </ol>
      )}

      {approvalQuestion !== null && (
        <p className="text-[0.9375rem] text-chat-foreground leading-relaxed break-words">
          {approvalQuestion}
        </p>
      )}
      {prompt.queued != null && (
        <p className="text-xs text-chat-foreground-secondary leading-relaxed">
          {queuedCopy(prompt.agent, prompt.queued)}
        </p>
      )}
      {bodyBlock}

      {prompt.answerStatus === "outcomeUnknown" && (
        <div role="alert" className="space-y-2 rounded-lg border border-status-warning p-3">
          <p className="text-sm">The previous answer may have reached the agent. Choose whether to dismiss this card; no answer will be resent.</p>
          <button type="button" data-testid="reference-prompt-dismiss-ambiguous" className="min-h-11 rounded border px-3" onClick={() => {
            if (window.confirm("Dismiss this ambiguous prompt? The previous answer may already have been applied. No answer will be resent.")) {
              void onDismissAmbiguous?.();
            }
          }}>Dismiss without retrying</button>
        </div>
      )}

      {twoOptionApproval !== null ? (
        <ApprovalActionCard
          bare
          description={approvalQuestion ?? prompt.body ?? prompt.title}
          confirmLabel={prompt.options[twoOptionApproval[0] ?? 0]?.label ?? "Approve"}
          declineLabel={prompt.options[twoOptionApproval[1] ?? 1]?.label ?? "Decline"}
          allowCustomFeedback={false}
          onAccept={(_feedback, event) =>
            prompt.answerStatus === "outcomeUnknown" || answerHeldForUploads
              ? undefined
              : void submitChoice(twoOptionApproval[0] ?? 0, event, true)
          }
          onDecline={(_feedback, event) =>
            prompt.answerStatus === "outcomeUnknown" || answerHeldForUploads
              ? undefined
              : void submitChoice(twoOptionApproval[1] ?? 1, event, true)
          }
          isSubmitting={pending || prompt.answerStatus === "outcomeUnknown"}
        />
      ) : (
        <>
          {hasChoices && prompt.multiSelect && (
            <div className="flex flex-col gap-2" role="group" aria-label={prompt.question}>
              {prompt.options.map((_, index) => {
                if (index === customIndex) return null;
                const checked = selected.includes(index);
                return (
                  <label
                    key={index}
                    data-testid={`reference-prompt-multi-${index}`}
                    className={cn(
                      "flex min-h-11 cursor-pointer items-start gap-2.5 rounded-lg border border-chat-border px-3 py-2.5 text-sm transition-colors has-[:focus-visible]:ring-1 has-[:focus-visible]:ring-ring",
                      checked
                        ? "bg-chat-surface-raised text-chat-foreground"
                        : "bg-chat-surface text-chat-foreground hover:bg-chat-surface-hover",
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={pending || prompt.answerStatus === "outcomeUnknown"}
                      onChange={() => toggle(index)}
                      className="sr-only"
                    />
                    <span
                      aria-hidden="true"
                      className={cn(
                        "mt-px flex size-5 shrink-0 items-center justify-center rounded border",
                        checked
                          ? "border-chat-foreground bg-chat-foreground text-chat-screen"
                          : "border-chat-foreground-secondary",
                      )}
                    >
                      {checked && <Check className="size-3.5" />}
                    </span>
                    {optionNumber(index)}
                    {optionContent(index)}
                  </label>
                );
              })}
            </div>
          )}
          {hasChoices && !prompt.multiSelect && (
            <div
              className="flex flex-col gap-2"
              role="radiogroup"
              aria-label={prompt.question}
            >
              {prompt.options.map((_, index) => {
                if (index === customIndex) return null;
                const checked = pickedIndex === index;
                return (
                  <button
                    key={index}
                    type="button"
                    role="radio"
                    aria-checked={checked}
                    data-testid={`reference-prompt-option-${index}`}
                    disabled={pending || prompt.answerStatus === "outcomeUnknown"}
                    className={cn(
                      "flex w-full min-h-11 items-start gap-2.5 rounded-lg border border-chat-border px-3 py-2.5 text-left text-sm text-chat-foreground transition-colors disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      checked ? "bg-chat-surface-raised" : "bg-chat-surface hover:bg-chat-surface-hover",
                    )}
                    onClick={() => setPicked(index)}
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        "mt-px flex size-5 shrink-0 items-center justify-center rounded-full border",
                        checked
                          ? "border-chat-foreground bg-chat-foreground text-chat-screen"
                          : "border-chat-foreground-secondary",
                      )}
                    >
                      {checked && <Check className="size-3.5" />}
                    </span>
                    {optionNumber(index)}
                    {optionContent(index)}
                  </button>
                );
              })}
            </div>
          )}
          {hasChoices && (
            <button
              type="button"
              data-testid="reference-prompt-submit"
              disabled={
                pending || prompt.answerStatus === "outcomeUnknown" || answerHeldForUploads || (prompt.multiSelect ? selected.length === 0 : pickedIndex === null)
              }
              className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground transition-colors hover:bg-chat-primary/90 disabled:bg-chat-surface-raised disabled:text-chat-foreground-secondary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                if (prompt.multiSelect) {
                  const indices = [...selected].sort((a, b) => a - b);
                  if (attachmentsReady) {
                    const composed = composeAnswerWithAttachments(
                      indices.map((each) => each + 1).join(" "),
                    );
                    if (composed !== null) void answer({ customText: composed }, event);
                    return;
                  }
                  void answer({ optionIndices: indices }, event);
                } else if (pickedIndex !== null) {
                  const send = (): Promise<boolean> =>
                    attachmentsReady
                      ? (() => {
                          const composed = composeAnswerWithAttachments(String(pickedIndex + 1));
                          return composed !== null
                            ? answer({ customText: composed }, event)
                            : Promise.resolve(false);
                        })()
                      : answer({ optionIndex: pickedIndex }, event);
                  void send().then((current) => {
                    if (current && typedAnswer != null) onTypedAnswerDone?.();
                  });
                }
              }}
            >
              {pending ? (
                <Loader2 aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
              ) : (
                <Send aria-hidden="true" className="size-4" />
              )}
              <span>
                {prompt.multiSelect && selected.length > 0
                  ? `Send answer (${selected.length})`
                  : "Send answer"}
              </span>
            </button>
          )}
          {customIndex !== null && (
            <div className="space-y-1.5">
              {hasChoices && (
                <span id={customLabelId} className="block text-xs text-chat-foreground-secondary">
                  Or type your own answer
                </span>
              )}
              <div className="flex items-center gap-2">
                <input
                  data-testid="reference-prompt-custom-input"
                  className="min-h-11 min-w-0 flex-1 rounded-lg border border-chat-border bg-chat-screen px-3 py-2 text-base text-chat-foreground placeholder:text-chat-foreground-tertiary transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  value={custom}
                  disabled={pending || prompt.answerStatus === "outcomeUnknown"}
                  enterKeyHint="send"
                  placeholder={prompt.options[customIndex]?.label ?? "Type an answer"}
                  aria-label={hasChoices ? undefined : "Your answer"}
                  aria-labelledby={hasChoices ? customLabelId : undefined}
                  onChange={(event) => setCustom(event.currentTarget.value)}
                  onPaste={(event) => {
                    // t3code: pasted images join the answer. Only images; other pasted files
                    // keep the browser's default (the text of a file path or nothing).
                    if (onAttachAnswerFiles === undefined) return;
                    const images = [...event.clipboardData.files].filter((each) =>
                      each.type.startsWith("image/"),
                    );
                    if (images.length === 0) return;
                    event.preventDefault();
                    addAnswerFiles(images);
                  }}
                  onCompositionStart={() => {
                    composing.current = true;
                  }}
                  onCompositionEnd={() => {
                    composing.current = false;
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" || isImeConfirmingKey(event, composing.current)) return;
                    event.preventDefault();
                    if (answerHeldForUploads) return;
                    const typed = custom.trim();
                    if (typed.length === 0 && attachments.length === 0) return;
                    const composed = attachmentsReady ? composeAnswerWithAttachments(typed) : typed;
                    if (composed === null || composed.length === 0) return;
                    void answer({ customText: composed });
                  }}
                />
                <button
                  type="button"
                  data-testid="reference-prompt-custom-send"
                  disabled={
                    pending ||
                    prompt.answerStatus === "outcomeUnknown" ||
                    answerHeldForUploads ||
                    (custom.trim().length === 0 && attachments.length === 0)
                  }
                  className="flex min-h-11 shrink-0 items-center gap-1.5 rounded-lg border border-chat-border bg-chat-surface-raised px-4 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  onClick={(event) => {
                    const typed = custom.trim();
                    const composed = attachmentsReady ? composeAnswerWithAttachments(typed) : typed;
                    if (composed === null || composed.length === 0) return;
                    void answer({ customText: composed }, event);
                  }}
                >
                  <Send className="size-4" aria-hidden="true" />
                  <span>Send</span>
                </button>
              </div>
              {onAttachAnswerFiles !== undefined && (
                <ReferenceAnswerAttachments
                  attachments={attachments}
                  disabled={pending || prompt.answerStatus === "outcomeUnknown"}
                  onPickFiles={addAnswerFiles}
                  onRemove={(id) => referenceAnswerAttachmentStore.remove(attachKey, id)}
                  onRetry={retryAnswerUpload}
                />
              )}
            </div>
          )}
        </>
      )}

      {typedIndex !== null && typedAnswer !== null && (
        <div
          role="group"
          aria-labelledby={confirmLabelId}
          data-testid="reference-prompt-confirm"
          className="space-y-3 rounded-lg bg-chat-surface-raised p-3"
        >
          {/* an option's label can run to pages (a review to approve): it wraps and scrolls in
              itself, and the two buttons stay whole */}
          <span
            id={confirmLabelId}
            aria-live="polite"
            className="block max-h-24 overflow-y-auto text-sm text-chat-foreground break-words"
          >
            {`Send ${typedIndex + 1}. ${prompt.options[typedIndex]?.label ?? ""}?`}
          </span>
          <span className="flex flex-col gap-3">
            <button
              ref={confirmButtonRef}
              type="button"
              data-testid="reference-prompt-confirm-send"
              disabled={
                pending || prompt.answerStatus === "outcomeUnknown" || answerHeldForUploads
              }
              className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground transition-colors hover:bg-chat-primary/90 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                // the ready attachments are part of this answer too: the option's number is
                // the text their mentions follow, exactly as a picked option sends it
                const composed = attachmentsReady
                  ? composeAnswerWithAttachments(String(typedIndex + 1))
                  : null;
                if (attachmentsReady && composed === null) return;
                void answer(
                  composed !== null ? { customText: composed } : { optionIndex: typedIndex },
                  event,
                ).then((current) => {
                  if (current) onTypedAnswerDone?.();
                });
              }}
            >
              {pending ? (
                <Loader2 className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              ) : (
                <Check className="size-4" aria-hidden="true" />
              )}
              <span>Confirm</span>
            </button>
            <button
              type="button"
              data-testid="reference-prompt-confirm-cancel"
              disabled={pending || prompt.answerStatus === "outcomeUnknown"}
              className="flex min-h-11 w-full items-center justify-center rounded-lg border border-chat-border bg-chat-surface px-4 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={() => onTypedAnswerDone?.()}
            >
              Cancel
            </button>
          </span>
        </div>
      )}

      {prompt.fallback === true && (
        <p data-testid="reference-prompt-fallback" className="text-sm text-chat-foreground-secondary">
          {FALLBACK_HINT}
        </p>
      )}

      {shownError !== null && (
        <p
          role="alert"
          data-testid="reference-prompt-error"
          className="text-sm text-chat-danger leading-relaxed break-words"
        >
          {shownError}
        </p>
      )}
    </section>
  );
}
