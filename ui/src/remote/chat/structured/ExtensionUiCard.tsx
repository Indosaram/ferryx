/**
 * `ExtensionUiCard` (lane H5): one provider extension request drawn as a card the reader answers.
 *
 * The six interactive methods `mhc` sends (`select`, `confirm`, `input`, `editor`, `question`)
 * each name their own answer shape in the frozen `ExtensionUiResponseBody` contract, so each gets
 * its own control here: a `select` answers `value`, a `confirm` answers `confirmed`, an `input`
 * and an `editor` answer `value`, and a `question` answers `answers` keyed by question id.
 *
 * `set_editor_text` never reaches this card: it is a draft prefill, applied by the lane
 * (`structuredEditorPrefill`) and answered at once. A method this client does not know is drawn
 * as a dismissal only - the card never guesses an answer shape it cannot fill.
 */
import React, { useCallback, useMemo, useState } from "react";
import { Check, X } from "lucide-react";
import { cn } from "../../../lib/cn";
import type {
  StructuredExtensionUiRequest,
  StructuredExtensionUiResponse,
} from "../../../lib/structuredChatTypes";

/** One question of a `question` request, as the provider's `payload.questions` describes it. */
export interface StructuredExtensionQuestion {
  readonly id: string;
  readonly header: string;
  readonly question: string;
  readonly options: readonly { readonly label: string; readonly description?: string }[];
  readonly multiSelect: boolean;
}

function isRecord(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * The questions of a `question` request, read defensively out of its free-form `payload`.
 *
 * `payload` is `serde_json::Value` on the wire, so a gateway that ships a shape this client does
 * not know must not crash the transcript. A row missing its `id` or `question` is dropped: an
 * answer keyed by an id the card invented could not be matched by the provider.
 */
export function structuredExtensionQuestions(
  request: StructuredExtensionUiRequest,
): readonly StructuredExtensionQuestion[] {
  const rows = isRecord(request.payload) ? request.payload.questions : undefined;
  if (!Array.isArray(rows)) return [];
  const questions: StructuredExtensionQuestion[] = [];
  for (const row of rows) {
    if (!isRecord(row)) continue;
    const id = typeof row.id === "string" && row.id.length > 0 ? row.id : null;
    const question = typeof row.question === "string" ? row.question : null;
    if (id === null || question === null) continue;
    const options: { label: string; description?: string }[] = [];
    const listed = row.options;
    if (Array.isArray(listed)) {
      for (const option of listed) {
        if (typeof option === "string" && option.length > 0) {
          options.push({ label: option });
          continue;
        }
        if (!isRecord(option)) continue;
        const label = typeof option.label === "string" ? option.label : null;
        if (label === null) continue;
        const description = typeof option.description === "string" ? option.description : undefined;
        options.push({ label, ...(description === undefined ? {} : { description }) });
      }
    }
    questions.push({
      id,
      header: typeof row.header === "string" ? row.header : "",
      question,
      options,
      multiSelect: row.multiSelect === true,
    });
  }
  return questions;
}

/** The heading a request draws: its own title, else the method, never an empty box. */
function heading(request: StructuredExtensionUiRequest): string {
  return request.title ?? request.message ?? request.method;
}

const BUTTON =
  "flex min-h-9 items-center gap-1 rounded-md px-3 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40";
const PRIMARY = cn(BUTTON, "bg-chat-surface-hover text-chat-foreground hover:bg-chat-border");
const SECONDARY = cn(BUTTON, "text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground");

export interface ExtensionUiCardProps {
  readonly request: StructuredExtensionUiRequest;
  /** Answers one request; the lane clears the card when the answer lands. */
  readonly onAnswer: (requestId: string, response: StructuredExtensionUiResponse) => Promise<boolean>;
  /** A stop is settling the turn, so the answer would race it. */
  readonly disabled?: boolean;
}

/**
 * The card. Each method keeps its own local state and submits one response shape, so a partial
 * answer never goes out: a `select` with nothing picked and a `question` with an unanswered row
 * both leave the submit button disabled rather than sending a value the reader never chose.
 */
export const ExtensionUiCard: React.FC<ExtensionUiCardProps> = ({ request, onAnswer, disabled = false }) => {
  const [pending, setPending] = useState(false);
  const [value, setValue] = useState(request.prefill ?? "");
  const [selected, setSelected] = useState<string | null>(null);
  const [answers, setAnswers] = useState<Readonly<Record<string, readonly string[]>>>({});

  const questions = useMemo(() => structuredExtensionQuestions(request), [request]);

  const answer = useCallback(
    async (response: StructuredExtensionUiResponse): Promise<void> => {
      setPending(true);
      try {
        await onAnswer(request.requestId, response);
      } finally {
        setPending(false);
      }
    },
    [onAnswer, request.requestId],
  );

  const cancel = useCallback(() => void answer({ cancelled: true }), [answer]);
  const busy = pending || disabled;

  const header = (
    <div className="flex items-start justify-between gap-2">
      <div className="min-w-0 flex flex-col gap-0.5">
        <p data-testid="extension-ui-title" className="text-sm font-medium text-chat-foreground">
          {heading(request)}
        </p>
        {request.message !== undefined && request.message !== request.title ? (
          <p data-testid="extension-ui-message" className="whitespace-pre-wrap text-xs text-chat-foreground-secondary">
            {request.message}
          </p>
        ) : null}
      </div>
      <button
        type="button"
        data-testid="extension-ui-cancel"
        aria-label="Dismiss this request"
        disabled={busy}
        onClick={cancel}
        className={cn(SECONDARY, "size-9 shrink-0 justify-center px-0")}
      >
        <X aria-hidden="true" className="size-4" />
      </button>
    </div>
  );

  const footer = (canSubmit: boolean, onSubmit: () => void) => (
    <div className="flex items-center justify-end gap-2 pt-1">
      <button type="button" data-testid="extension-ui-dismiss" disabled={busy} onClick={cancel} className={SECONDARY}>
        Cancel
      </button>
      <button
        type="button"
        data-testid="extension-ui-submit"
        disabled={busy || !canSubmit}
        onClick={onSubmit}
        className={PRIMARY}
      >
        <Check aria-hidden="true" className="size-3.5" />
        Send
      </button>
    </div>
  );

  const body = ((): React.ReactNode => {
    switch (request.method) {
      case "select":
        return (
          <>
            <ul data-testid="extension-ui-options" className="flex flex-col gap-1">
              {request.options.map((option, index) => (
                <li key={option}>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={selected === option}
                    data-testid={`extension-ui-option-${index}`}
                    disabled={busy}
                    onClick={() => setSelected(option)}
                    className={cn(
                      "flex w-full min-h-9 items-center rounded-md px-2 text-left text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      selected === option
                        ? "bg-chat-surface-hover text-chat-foreground"
                        : "text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground",
                    )}
                  >
                    {option}
                  </button>
                </li>
              ))}
            </ul>
            {footer(selected !== null, () => selected !== null && void answer({ value: selected }))}
          </>
        );
      case "confirm":
        return (
          <div className="flex items-center justify-end gap-2 pt-1">
            <button type="button" data-testid="extension-ui-decline" disabled={busy} onClick={cancel} className={SECONDARY}>
              Cancel
            </button>
            <button
              type="button"
              data-testid="extension-ui-submit"
              disabled={busy}
              onClick={() => void answer({ confirmed: true })}
              className={PRIMARY}
            >
              <Check aria-hidden="true" className="size-3.5" />
              Confirm
            </button>
          </div>
        );
      case "input":
        return (
          <>
            <input
              data-testid="extension-ui-input"
              aria-label={heading(request)}
              value={value}
              disabled={busy}
              placeholder={request.placeholder ?? ""}
              onChange={(event) => setValue(event.currentTarget.value)}
              className="min-h-9 w-full rounded-md border border-chat-border bg-chat-surface px-2 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            />
            {footer(value.trim().length > 0, () => void answer({ value }))}
          </>
        );
      case "editor":
        return (
          <>
            <textarea
              data-testid="extension-ui-editor"
              aria-label={heading(request)}
              rows={5}
              value={value}
              disabled={busy}
              placeholder={request.placeholder ?? ""}
              onChange={(event) => setValue(event.currentTarget.value)}
              className="w-full resize-y rounded-md border border-chat-border bg-chat-surface px-2 py-1.5 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            />
            {footer(true, () => void answer({ value }))}
          </>
        );
      case "question":
        // An empty form is a shape this client cannot answer: the provider named questions but
        // this card could not read one, so it offers the dismissal alone rather than a blank send.
        if (questions.length === 0) {
          return (
            <div className="flex items-center justify-end pt-1">
              <button type="button" data-testid="extension-ui-dismiss" disabled={busy} onClick={cancel} className={SECONDARY}>
                Dismiss
              </button>
            </div>
          );
        }
        return (
          <>
            <div className="flex flex-col gap-3">
              {questions.map((question) => {
                const picked = answers[question.id] ?? [];
                const toggle = (label: string): void => {
                  setAnswers((previous) => {
                    const current = previous[question.id] ?? [];
                    if (!question.multiSelect) return { ...previous, [question.id]: [label] };
                    const next = current.includes(label)
                      ? current.filter((row) => row !== label)
                      : [...current, label];
                    return { ...previous, [question.id]: next };
                  });
                };
                return (
                  <fieldset key={question.id} data-testid={`extension-ui-question-${question.id}`} className="flex flex-col gap-1">
                    <legend className="text-xs font-medium text-chat-foreground">
                      {question.header.length > 0 ? question.header : question.question}
                    </legend>
                    {question.header.length > 0 ? (
                      <p className="text-xs text-chat-foreground-secondary">{question.question}</p>
                    ) : null}
                    {question.options.map((option, index) => (
                      <button
                        key={option.label}
                        type="button"
                        role={question.multiSelect ? "checkbox" : "radio"}
                        aria-checked={picked.includes(option.label)}
                        data-testid={`extension-ui-question-${question.id}-option-${index}`}
                        disabled={busy}
                        onClick={() => toggle(option.label)}
                        className={cn(
                          "flex min-h-9 w-full flex-col items-start rounded-md px-2 text-left text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                          picked.includes(option.label)
                            ? "bg-chat-surface-hover text-chat-foreground"
                            : "text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground",
                        )}
                      >
                        <span>{option.label}</span>
                        {option.description !== undefined ? (
                          <span className="text-chat-foreground-secondary">{option.description}</span>
                        ) : null}
                      </button>
                    ))}
                  </fieldset>
                );
              })}
            </div>
            {footer(
              questions.every((question) => (answers[question.id] ?? []).length > 0),
              () =>
                void answer({
                  // The provider keys answers by question id and reads `selected` as a list, which
                  // is what `RpcQuestionAnswer` carries on the wire.
                  answers: Object.fromEntries(
                    questions.map((question) => [
                      question.id,
                      { selected: [...(answers[question.id] ?? [])] },
                    ]),
                  ),
                }),
            )}
          </>
        );
      default:
        // A method this client does not draw a control for: dismissing it is the only answer that
        // cannot be wrong. It still goes through `answerExtensionUi`, so the provider is not left
        // waiting on a card that will never send.
        return (
          <div className="flex items-center justify-end pt-1">
            <button type="button" data-testid="extension-ui-dismiss" disabled={busy} onClick={cancel} className={SECONDARY}>
              Dismiss
            </button>
          </div>
        );
    }
  })();

  return (
    <section
      data-testid="extension-ui-card"
      data-method={request.method}
      aria-label={heading(request)}
      className="mx-3 mt-2 flex flex-col gap-2 rounded-lg border border-chat-border bg-chat-surface-raised p-3"
    >
      {header}
      {body}
    </section>
  );
};
