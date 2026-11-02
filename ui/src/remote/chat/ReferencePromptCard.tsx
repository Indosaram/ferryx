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
import React, { useCallback, useEffect, useId, useRef, useState } from "react";
import { AlertCircle, Check, Loader2, Send } from "lucide-react";
import { cn } from "../../lib/cn";
import { haptic } from "../haptics";
import { userFacingError } from "../userFacingError";
import { isImeConfirmingKey } from "./imeKeys";
import { ApprovalActionCard } from "./MobileChatComponents";
import {
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
  /** An answer went out; `toMessageBox`: the keyboard's focus was in the card and may go on to the message box. */
  readonly onAnswered?: (toMessageBox: boolean) => void;
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

export function ReferencePromptCard({
  prompt,
  screenRevision,
  onAnswer,
  onPromptChanged,
  onAnswered,
  typedAnswer = null,
  onTypedAnswerDone,
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
    [onAnswer, onAnswered, onPromptChanged, prompt.id, screenRevision],
  );

  const toggle = (index: number): void => {
    setSelected((current) =>
      current.includes(index) ? current.filter((each) => each !== index) : [...current, index],
    );
  };

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

      {twoOptionApproval !== null ? (
        <ApprovalActionCard
          bare
          description={approvalQuestion ?? prompt.body ?? prompt.title}
          confirmLabel={prompt.options[twoOptionApproval[0] ?? 0]?.label ?? "Approve"}
          declineLabel={prompt.options[twoOptionApproval[1] ?? 1]?.label ?? "Decline"}
          allowCustomFeedback={false}
          onAccept={(_feedback, event) =>
            void answer({ optionIndex: twoOptionApproval[0] ?? 0 }, event, true)
          }
          onDecline={(_feedback, event) =>
            void answer({ optionIndex: twoOptionApproval[1] ?? 1 }, event, true)
          }
          isSubmitting={pending}
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
                      disabled={pending}
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
                    disabled={pending}
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
                pending || (prompt.multiSelect ? selected.length === 0 : pickedIndex === null)
              }
              className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground transition-colors hover:bg-chat-primary/90 disabled:bg-chat-surface-raised disabled:text-chat-foreground-secondary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                if (prompt.multiSelect) {
                  void answer({ optionIndices: [...selected].sort((a, b) => a - b) }, event);
                } else if (pickedIndex !== null) {
                  void answer({ optionIndex: pickedIndex }, event).then((current) => {
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
                  disabled={pending}
                  enterKeyHint="send"
                  placeholder={prompt.options[customIndex]?.label ?? "Type an answer"}
                  aria-label={hasChoices ? undefined : "Your answer"}
                  aria-labelledby={hasChoices ? customLabelId : undefined}
                  onChange={(event) => setCustom(event.currentTarget.value)}
                  onCompositionStart={() => {
                    composing.current = true;
                  }}
                  onCompositionEnd={() => {
                    composing.current = false;
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" || isImeConfirmingKey(event, composing.current)) return;
                    event.preventDefault();
                    if (custom.trim().length > 0) void answer({ customText: custom.trim() });
                  }}
                />
                <button
                  type="button"
                  data-testid="reference-prompt-custom-send"
                  disabled={pending || custom.trim().length === 0}
                  className="flex min-h-11 shrink-0 items-center gap-1.5 rounded-lg border border-chat-border bg-chat-surface-raised px-4 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  onClick={(event) => void answer({ customText: custom.trim() }, event)}
                >
                  <Send className="size-4" aria-hidden="true" />
                  <span>Send</span>
                </button>
              </div>
            </div>
          )}
        </>
      )}

      {typedAnswer?.optionIndex != null && (
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
            {`Send ${typedAnswer.optionIndex + 1}. ${prompt.options[typedAnswer.optionIndex]?.label ?? ""}?`}
          </span>
          <span className="flex flex-col gap-3">
            <button
              ref={confirmButtonRef}
              type="button"
              data-testid="reference-prompt-confirm-send"
              disabled={pending}
              className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg bg-chat-primary px-4 text-sm font-medium text-chat-primary-foreground transition-colors hover:bg-chat-primary/90 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) =>
                void answer(typedAnswer, event).then((current) => {
                  if (current) onTypedAnswerDone?.();
                })
              }
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
              disabled={pending}
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
