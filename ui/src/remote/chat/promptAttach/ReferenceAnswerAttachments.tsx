/**
 * The Attach files row of a prompt's answer field (lane G6).
 *
 * Parity target: t3code `docs/user/question-attachments.md` — a question that accepts a
 * custom answer offers an attachment button and lists its uploads with their state, so a
 * failed one can be retried or removed before the answer goes out. Rendered only where
 * the card shows a custom answer field; a question with only predefined choices never
 * sees this row.
 *
 * Presentational: the store and the upload calls stay in the card.
 */
import { useId, useRef } from "react";
import { Loader2, Paperclip, RotateCcw, X } from "lucide-react";
import { cn } from "../../../lib/cn";
import type { ReferenceAnswerAttachment } from "./referencePromptAttach";

export interface ReferenceAnswerAttachmentsProps {
  readonly attachments: readonly ReferenceAnswerAttachment[];
  readonly disabled: boolean;
  readonly onPickFiles: (files: readonly File[]) => void;
  readonly onRemove: (id: string) => void;
  readonly onRetry: (id: string) => void;
  readonly className?: string;
}

export function ReferenceAnswerAttachments({
  attachments,
  disabled,
  onPickFiles,
  onRemove,
  onRetry,
  className,
}: ReferenceAnswerAttachmentsProps) {
  const inputId = useId();
  const inputRef = useRef<HTMLInputElement | null>(null);

  const pick = (list: FileList | null): void => {
    if (list === null || list.length === 0) return;
    onPickFiles([...list]);
    // the same file picked again must fire a change event: reset the input
    if (inputRef.current !== null) inputRef.current.value = "";
  };

  return (
    <div className={cn("space-y-1.5", className)}>
      <button
        type="button"
        data-testid="reference-answer-attach"
        disabled={disabled}
        className="flex min-h-9 items-center gap-1.5 rounded-lg border border-chat-border bg-chat-surface-raised px-3 text-xs font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={() => inputRef.current?.click()}
      >
        <Paperclip className="size-3.5" aria-hidden="true" />
        <span>Attach files</span>
      </button>
      <input
        ref={inputRef}
        id={inputId}
        data-testid="reference-answer-attach-input"
        type="file"
        multiple
        // `hidden`, as t3code has it: the button opens the picker, so the input itself must
        // not be a tab stop a keyboard user lands on and finds unlabeled
        hidden
        tabIndex={-1}
        onChange={(event) => pick(event.currentTarget.files)}
      />
      <span aria-live="polite" className="block">
        {attachments.map((each) => (
          <span
            key={each.id}
            data-testid={`reference-answer-attachment-${each.id}`}
            data-status={each.status}
            className={cn(
              "flex items-center gap-2 rounded-lg border border-chat-border px-2.5 py-1.5 text-xs",
              each.status === "failed" ? "border-status-warning" : "",
            )}
          >
            {each.status === "uploading" ? (
              <Loader2 aria-hidden="true" className="size-3.5 shrink-0 animate-spin motion-reduce:animate-none" />
            ) : (
              <Paperclip aria-hidden="true" className="size-3.5 shrink-0" />
            )}
            <span className="min-w-0 flex-1 truncate text-chat-foreground">
              {each.displayName}
              <span className="sr-only">
                {each.status === "uploading"
                  ? " (uploading)"
                  : each.status === "failed"
                    ? " (failed)"
                    : " (uploaded)"}
              </span>
            </span>
            {each.status === "failed" && each.error !== null && (
              <span role="alert" className="shrink-0 text-status-warning">
                {each.error}
              </span>
            )}
            {each.status === "failed" && (
              <button
                type="button"
                data-testid={`reference-answer-attachment-retry-${each.id}`}
                aria-label={`Retry uploading ${each.displayName}`}
                disabled={disabled}
                className="shrink-0 rounded p-1 text-chat-foreground-secondary hover:text-chat-foreground disabled:opacity-50"
                onClick={() => onRetry(each.id)}
              >
                <RotateCcw className="size-3.5" aria-hidden="true" />
              </button>
            )}
            <button
              type="button"
              data-testid={`reference-answer-attachment-remove-${each.id}`}
              aria-label={`Remove ${each.displayName}`}
              disabled={disabled}
              className="shrink-0 rounded p-1 text-chat-foreground-secondary hover:text-chat-foreground disabled:opacity-50"
              onClick={() => onRemove(each.id)}
            >
              <X className="size-3.5" aria-hidden="true" />
            </button>
          </span>
        ))}
      </span>
    </div>
  );
}
