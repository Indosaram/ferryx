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

const QUEUED_OPEN =
  "Codex keeps working meanwhile. Answer here; the question holds the terminal's input until it is answered or closed.";
const QUEUED_COLLAPSED =
  "Codex keeps working meanwhile. Answer here; the message box still talks to Codex.";
const FALLBACK_HINT =
  "This pane is waiting and no reader knows its prompt: answer it in the terminal.";

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
  const cardRef = useRef<HTMLElement | null>(null);
  const confirmRef = useRef<HTMLDivElement | null>(null);
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

  // the question to confirm stays on the card's fold; a card scrolled past it comes back to it,
  // and nothing outside the card moves
  useEffect(() => {
    confirmRef.current?.scrollIntoView({ block: "nearest" });
  }, [typedAnswer]);

  const answer = useCallback(
    async (choice: ReferencePromptAnswer, press?: ReferencePromptPress): Promise<boolean> => {
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
        if (!shown.current || accepted === false) return false;
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
          setLocalError(cause instanceof Error ? cause.message : String(cause));
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
        <span className="flex items-baseline gap-1.5">
          <span className="break-words">{text}</span>
          {recommended && (
            <span className="shrink-0 rounded border border-chat-border px-1 text-[10px] text-chat-foreground-tertiary">
              Recommended
            </span>
          )}
        </span>
        {option?.description != null && (
          <span className="block text-[11px] text-chat-foreground-tertiary break-words">
            {option.description}
          </span>
        )}
      </span>
    );
  };
  const optionNumber = (index: number): React.ReactNode => (
    <span className="shrink-0 pt-0.5 font-mono text-[10px] text-chat-foreground-tertiary">
      <span aria-hidden="true">{index + 1}</span>
      <span className="sr-only">{index + 1}.</span>
    </span>
  );

  // The existing approval shell models one shape: two options, no custom row, a single pick.
  // Anything richer (Codex's three-option approval, a plan with a "tell Claude what to change"
  // row, a multi-select) is the ported option list below, which the shell cannot express.
  const twoOptionApproval =
    prompt.kind === "approval" &&
    !prompt.multiSelect &&
    customIndex === null &&
    selectableIndices.length === 2
      ? selectableIndices
      : null;
  const approvalQuestion = prompt.question === prompt.title ? null : prompt.question;
  const approvalDescription = approvalQuestion ?? prompt.body ?? prompt.title;
  const shownError = localError ?? error ?? null;

  return (
    <section
      ref={cardRef}
      role="region"
      aria-labelledby={titleId}
      aria-busy={pending}
      data-testid="reference-prompt-card"
      className={cn(
        "rounded-xl border border-status-warning/30 bg-chat-surface/95 shadow-md p-3.5 backdrop-blur-md my-2.5 space-y-3 select-none",
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
      <header className="flex items-start gap-2.5">
        <div className="p-1 rounded-lg bg-status-warning/10 border border-status-warning/20 text-status-warning shrink-0 mt-0.5">
          <AlertCircle className="w-4 h-4" aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1 space-y-0.5">
          <span className="sr-only">input needed</span>
          <h2
            id={titleId}
            className="text-xs font-semibold text-chat-foreground tracking-tight break-words"
          >
            {prompt.title}
          </h2>
        </div>
      </header>

      {prompt.steps != null && prompt.steps.length > 0 && (
        <ol className="space-y-1" aria-label="Questions">
          {prompt.steps.map((step, index) => (
            <li
              key={index}
              aria-current={step.current ? "step" : undefined}
              className={cn(
                "flex items-center gap-1.5 text-xs",
                step.answered ? "text-chat-foreground-secondary" : "text-chat-foreground",
                step.current && "font-medium",
              )}
            >
              <span
                aria-hidden="true"
                className="flex size-4 shrink-0 items-center justify-center rounded-full border border-chat-border font-mono text-[10px]"
              >
                {step.answered ? <Check className="size-2.5" /> : index + 1}
              </span>
              <span className="truncate">{step.label}</span>
              {step.answered && <span className="sr-only">(answered)</span>}
            </li>
          ))}
        </ol>
      )}

      {twoOptionApproval !== null ? (
        <>
          {/* the reference text (a command, a diff) is what the reader must see before approving */}
          {approvalQuestion !== null && prompt.body != null && prompt.body.length > 0 && (
            <pre className="rounded-lg border border-chat-border bg-chat-screen/60 p-2 font-mono text-[11px] text-chat-foreground-secondary whitespace-pre-wrap break-words max-h-60 overflow-y-auto select-text">
              {prompt.body}
            </pre>
          )}
          <ApprovalActionCard
            title={prompt.title}
            description={approvalDescription}
            confirmLabel={prompt.options[twoOptionApproval[0] ?? 0]?.label ?? "Approve"}
            declineLabel={prompt.options[twoOptionApproval[1] ?? 1]?.label ?? "Decline"}
            allowCustomFeedback={false}
            onAccept={() => void answer({ optionIndex: twoOptionApproval[0] ?? 0 })}
            onDecline={() => void answer({ optionIndex: twoOptionApproval[1] ?? 1 })}
            isSubmitting={pending}
          />
        </>
      ) : (
        <>
          {approvalQuestion !== null && (
            <p className="text-xs text-chat-foreground-secondary leading-relaxed break-words">
              {approvalQuestion}
            </p>
          )}
          {prompt.queued != null && (
            <p className="text-[11px] text-chat-foreground-tertiary leading-relaxed">
              {prompt.queued === "open" ? QUEUED_OPEN : QUEUED_COLLAPSED}
            </p>
          )}
          {prompt.body != null && prompt.body.length > 0 && (
            <pre className="rounded-lg border border-chat-border bg-chat-screen/60 p-2 font-mono text-[11px] text-chat-foreground-secondary whitespace-pre-wrap break-words max-h-60 overflow-y-auto select-text">
              {prompt.body}
            </pre>
          )}
          {hasChoices && (
            <div
              className="space-y-1"
              role={prompt.multiSelect ? "group" : undefined}
              aria-label={prompt.multiSelect ? prompt.question : undefined}
            >
              {prompt.options.map((option, index) => {
                if (index === customIndex) return null;
                if (prompt.multiSelect) {
                  const checked = selected.includes(index);
                  return (
                    <label
                      key={index}
                      data-testid={`reference-prompt-multi-${index}`}
                      className={cn(
                        "flex min-h-[32px] cursor-pointer items-start gap-2 rounded-lg border px-2.5 py-1.5 text-xs transition-colors",
                        checked
                          ? "border-status-warning/40 bg-status-warning/10 text-chat-foreground"
                          : "border-chat-foreground/10 bg-chat-surface/60 text-chat-foreground-secondary hover:bg-chat-surface-hover",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={pending}
                        onChange={() => toggle(index)}
                        className="mt-0.5 size-3.5 shrink-0 accent-chat-primary"
                      />
                      {optionNumber(index)}
                      {optionContent(index)}
                    </label>
                  );
                }
                return (
                  <button
                    key={index}
                    type="button"
                    data-testid={`reference-prompt-option-${index}`}
                    disabled={pending}
                    className={cn(
                      "flex w-full min-h-[32px] items-start gap-2 rounded-lg border px-2.5 py-1.5 text-left text-xs transition-colors disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      typedAnswer?.optionIndex === index
                        ? "border-status-warning/40 bg-status-warning/10 text-chat-foreground"
                        : "border-chat-foreground/10 bg-chat-surface/60 text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground",
                    )}
                    onClick={(event) => void answer({ optionIndex: index }, event)}
                  >
                    {optionNumber(index)}
                    {optionContent(index)}
                  </button>
                );
              })}
            </div>
          )}
          {prompt.multiSelect && (
            <button
              type="button"
              data-testid="reference-prompt-submit"
              disabled={pending || selected.length === 0}
              className={cn(
                "px-3 py-1.5 rounded-lg border text-xs font-medium transition-all disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                selected.length > 0
                  ? "border-status-warning/40 bg-status-warning/20 text-status-warning hover:text-chat-foreground"
                  : "border-chat-foreground/10 bg-chat-surface-raised text-chat-foreground-secondary",
              )}
              onClick={(event) =>
                void answer(
                  { optionIndices: [...selected].sort((a, b) => a - b) },
                  event,
                )
              }
            >
              {selected.length > 0 ? `Submit (${selected.length})` : "Submit"}
            </button>
          )}
          {customIndex !== null && (
            <div className="space-y-1.5">
              {hasChoices && (
                <span id={customLabelId} className="text-[11px] text-chat-foreground-tertiary">
                  Or type your own answer
                </span>
              )}
              <div className="flex items-center gap-2">
                <input
                  data-testid="reference-prompt-custom-input"
                  className="min-w-0 flex-1 rounded-lg border border-chat-foreground/10 bg-chat-screen/50 px-2.5 py-1.5 text-xs text-chat-foreground placeholder:text-chat-foreground-tertiary focus:outline-none focus:border-status-warning/50 focus:ring-1 focus:ring-status-warning/30 transition-all"
                  value={custom}
                  disabled={pending}
                  placeholder={prompt.options[customIndex]?.label ?? "Type an answer"}
                  aria-label={hasChoices ? undefined : "Custom answer"}
                  aria-labelledby={hasChoices ? customLabelId : undefined}
                  onChange={(event) => setCustom(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    // an IME's Enter commits the candidate; WebKit can send it after compositionend, as key code 229
                    if (
                      event.key === "Enter" &&
                      !event.nativeEvent.isComposing &&
                      event.nativeEvent.keyCode !== 229 &&
                      custom.trim().length > 0
                    ) {
                      void answer({ customText: custom.trim() });
                    }
                  }}
                />
                <button
                  type="button"
                  data-testid="reference-prompt-custom-send"
                  disabled={pending || custom.trim().length === 0}
                  className="flex shrink-0 items-center gap-1.5 rounded-lg border border-chat-foreground/10 bg-chat-surface-raised px-3 py-1.5 text-xs font-medium text-chat-foreground-secondary transition-all hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  onClick={(event) => void answer({ customText: custom.trim() }, event)}
                >
                  <Send className="size-3.5" aria-hidden="true" />
                  <span>Send</span>
                </button>
              </div>
            </div>
          )}
        </>
      )}

      {typedAnswer?.optionIndex != null && (
        <div
          ref={confirmRef}
          role="alert"
          data-testid="reference-prompt-confirm"
          className="space-y-2 rounded-lg border border-status-warning/30 bg-status-warning/10 p-2.5"
        >
          {/* an option's label can run to pages (a review to approve): it wraps and scrolls in
              itself, and the two buttons stay whole */}
          <span className="block max-h-24 overflow-y-auto text-xs text-chat-foreground break-words select-text">
            {`Send ${typedAnswer.optionIndex + 1}. ${prompt.options[typedAnswer.optionIndex]?.label ?? ""}?`}
          </span>
          <span className="flex items-center gap-2">
            <button
              type="button"
              data-testid="reference-prompt-confirm-send"
              disabled={pending}
              className="flex items-center gap-1.5 rounded-lg border border-status-warning/40 bg-status-warning/20 px-3 py-1.5 text-xs font-medium text-status-warning shadow-xs transition-all hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) =>
                void answer(typedAnswer, event).then((current) => {
                  if (current) onTypedAnswerDone?.();
                })
              }
            >
              {pending ? (
                <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />
              ) : (
                <Check className="size-3.5" aria-hidden="true" />
              )}
              <span>Confirm</span>
            </button>
            <button
              type="button"
              data-testid="reference-prompt-confirm-cancel"
              disabled={pending}
              className="rounded-lg border border-chat-foreground/10 bg-chat-surface-raised px-3 py-1.5 text-xs font-medium text-chat-foreground-secondary transition-all hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={() => onTypedAnswerDone?.()}
            >
              Cancel
            </button>
          </span>
        </div>
      )}

      {prompt.fallback === true && (
        <p data-testid="reference-prompt-fallback" className="text-[11px] text-chat-foreground-tertiary">
          {FALLBACK_HINT}
        </p>
      )}

      {shownError !== null && (
        <p
          role="alert"
          data-testid="reference-prompt-error"
          className="text-xs text-chat-danger leading-relaxed break-words"
        >
          {shownError}
        </p>
      )}
    </section>
  );
}
