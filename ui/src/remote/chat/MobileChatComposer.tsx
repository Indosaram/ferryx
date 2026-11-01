import React, { useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowUp,
  FileText,
  Loader2,
  Pencil,
  Plus,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { cn } from "../../lib/cn";
import type { ChatAttachment } from "./MobileChatComponents";
import type { HeldMessage } from "./referenceQueue";
import type { ReferenceFileReceipt } from "./referenceTypes";
import { isImeConfirmingKey } from "./imeKeys";

export type { ChatAttachment };

export interface MobileChatComposerHandle {
  clearDraft: () => void;
  focus: () => void;
}

/**
 * The message box of the reference chat (plan task 12).
 *
 * The draft belongs to the target (owning host + backend session + daemon incarnation), so the
 * owner passes the text in and takes every edit back out; the composer keeps its own copy only
 * when it is used standalone. A file is staged on the owning host by the owner and comes back as
 * an editable mention inside the draft, never as an opaque attachment this component would have
 * to upload.
 *
 * Held rows are messages the reference chat could not deliver when the user asked; nothing here
 * sends one on its own.
 */
export interface MobileChatComposerProps {
  /** The owner-scoped draft. Absent means the composer keeps its own text. */
  readonly value?: string;
  readonly onValueChange?: (text: string) => void;
  /** The owning host refused to persist the draft; the draft is still here and the user is told. */
  readonly draftUnsaved?: boolean;
  /** Files already staged on the owning host for this target; the draft holds their mentions. */
  readonly attachments?: readonly ReferenceFileReceipt[];
  /** Stage files on the owning host; caret is where the mention is inserted. */
  readonly onAttachFiles?: (files: readonly File[], caret: number) => void;
  readonly onRemoveAttachment?: (attachmentId: string) => void;
  readonly attaching?: boolean;
  readonly attachError?: string | null;
  /** Held messages for this target, in the order the user wrote them. */
  readonly heldMessages?: readonly HeldMessage[];
  /** Held rows whose send is still on its way; each stays disabled until its own answer lands. */
  readonly heldSendingIds?: readonly string[];
  readonly heldUnsaved?: boolean;
  readonly onEditHeld?: (id: string, text: string) => void;
  readonly onSendHeld?: (id: string) => void;
  readonly onRemoveHeld?: (id: string) => void;
  /** A prompt waiting on the original pane, drawn directly above the message box. */
  readonly promptCard?: React.ReactNode;
  /** Something the user must know about this lane (a refused Stop, a held send, a lost host). */
  readonly warning?: string | null;
  /** Send the draft. `false` means the owner kept it (held, refused) and the box must not clear. */
  readonly onSend: (text: string) => boolean | void;
  readonly onStop?: () => void;
  readonly onClearDraft?: () => void;
  readonly isRunning?: boolean;
  readonly disabled?: boolean;
  readonly placeholder?: string;
  readonly className?: string;
}

/** How long a first tap on Stop stays armed on a touch screen before it needs arming again. */
const STOP_ARM_MS = 3000;

/** A held message named in a button's label: enough to tell rows apart, never a whole paragraph. */
function heldName(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > 40 ? `${flat.slice(0, 39)}…` : flat;
}

function coarsePointer(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(pointer: coarse)").matches === true;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const MobileChatComposer = React.forwardRef<
  MobileChatComposerHandle,
  MobileChatComposerProps
>(({
  value,
  onValueChange,
  draftUnsaved = false,
  attachments = [],
  onAttachFiles,
  onRemoveAttachment,
  attaching = false,
  attachError = null,
  heldMessages = [],
  heldSendingIds = [],
  heldUnsaved = false,
  onEditHeld,
  onSendHeld,
  onRemoveHeld,
  promptCard,
  warning = null,
  onSend,
  onStop,
  onClearDraft,
  isRunning = false,
  disabled = false,
  placeholder = "Ask the agent…",
  className,
}, ref) => {
  const isControlled = value !== undefined;
  const [internalText, setInternalText] = useState("");
  const text = isControlled ? value : internalText;
  const [editingHeldId, setEditingHeldId] = useState<string | null>(null);
  const [editingHeldText, setEditingHeldText] = useState("");
  const isComposingRef = useRef(false);
  // On a touch screen Stop takes two taps: the first arms it, so a slip never ends a turn.
  const [stopArmed, setStopArmed] = useState(false);
  const stopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
  }, []);
  useEffect(() => {
    if (!isRunning) setStopArmed(false);
  }, [isRunning]);
  const handleStop = useCallback(() => {
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
    if (!coarsePointer() || stopArmed) {
      setStopArmed(false);
      onStop?.();
      return;
    }
    setStopArmed(true);
    stopTimerRef.current = setTimeout(() => setStopArmed(false), STOP_ARM_MS);
  }, [onStop, stopArmed]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const setText = useCallback(
    (next: string) => {
      if (!isControlled) setInternalText(next);
      onValueChange?.(next);
    },
    [isControlled, onValueChange],
  );

  const adjustHeight = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const nextHeight = Math.min(el.scrollHeight, 144);
    el.style.height = `${Math.max(nextHeight, 36)}px`;
  }, []);

  useEffect(() => {
    adjustHeight();
  }, [text, adjustHeight]);

  const clearDraft = useCallback(() => {
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
    onClearDraft?.();
  }, [setText, onClearDraft]);

  const focus = useCallback(() => {
    textareaRef.current?.focus();
  }, []);

  useImperativeHandle(ref, () => ({
    clearDraft,
    focus,
  }), [clearDraft, focus]);

  /** Where a mention goes: the caret the user left in the box. */
  const caret = useCallback((): number => {
    const el = textareaRef.current;
    return el !== null && typeof el.selectionStart === "number" ? el.selectionStart : text.length;
  }, [text.length]);

  const handleSend = useCallback(() => {
    // A running pane still takes typed text (that is how a TUI is driven), and a prompt waiting
    // on the original pane takes a typed answer: the owner decides what the text means.
    // a file still staging has no mention yet: sending now would leave it behind
    if (disabled || attaching) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    const consumed = onSend(trimmed);
    if (consumed === false) return;
    // The owner keeps the sent text until the host acknowledges it, so a failed send puts it
    // back; the field itself clears the moment the send goes out.
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
  }, [disabled, attaching, text, onSend, setText]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        if (isImeConfirmingKey(e, isComposingRef.current)) return;
        // a soft keyboard has no Shift+Enter: there Enter is a new line and the Send button sends
        if (coarsePointer()) return;
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  const handleCompositionStart = useCallback(() => {
    isComposingRef.current = true;
  }, []);

  const handleCompositionEnd = useCallback(() => {
    isComposingRef.current = false;
  }, []);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (!files || files.length === 0) return;
      const chosen = Array.from(files);
      // read the caret before the input takes the focus
      const at = caret();
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
      onAttachFiles?.(chosen, at);
    },
    [caret, onAttachFiles],
  );

  const beginHeldEdit = useCallback((message: HeldMessage) => {
    setEditingHeldId(message.id);
    setEditingHeldText(message.text);
  }, []);

  const commitHeldEdit = useCallback(() => {
    if (editingHeldId === null) return;
    const trimmed = editingHeldText.trim();
    if (trimmed.length > 0) onEditHeld?.(editingHeldId, trimmed);
    setEditingHeldId(null);
    setEditingHeldText("");
  }, [editingHeldId, editingHeldText, onEditHeld]);

  const canSubmit = text.trim().length > 0 && !disabled && !attaching;
  // A prompt waiting on the original pane is a question this box answers, so the box keeps Send
  // beside Stop while one is up. Without a prompt, a running pane shows Stop alone.
  const answersWaitingPrompt = promptCard !== undefined && promptCard !== null;
  const showsSend = !isRunning || answersWaitingPrompt;
  const textareaLabel = placeholder.replace(/…$/, "");
  const hasExtras =
    draftUnsaved ||
    warning !== null ||
    attachError !== null ||
    attachments.length > 0 ||
    heldMessages.length > 0 ||
    answersWaitingPrompt;

  return (
    <div
      data-testid="mobile-chat-composer"
      className={cn(
        "flex flex-col w-full bg-chat-composer-panel border-t border-chat-composer-border rounded-t-2xl pb-safe select-none",
        className
      )}
    >
      {hasExtras && (
      // Banners, files, held rows and the prompt scroll on their own, so the message box always
      // stays on screen above the keyboard.
      <div
        data-testid="chat-composer-extras"
        className="max-h-[45dvh] overflow-y-auto overscroll-contain scrollbar-sleek"
      >
      {draftUnsaved && (
        <div
          data-testid="chat-composer-draft-unsaved"
          role="status"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-danger"
        >
          <AlertCircle aria-hidden="true" className="size-3.5 shrink-0" />
          <span>This draft could not be saved on this device. It stays here until you send it.</span>
        </div>
      )}
      {warning !== null && (
        <div
          data-testid="chat-composer-warning"
          role="status"
          aria-live="polite"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-foreground-secondary"
        >
          <span>{warning}</span>
        </div>
      )}
      {attachError !== null && (
        <div
          data-testid="chat-composer-attach-error"
          role="alert"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-xs text-chat-danger"
        >
          <span>{attachError}</span>
        </div>
      )}
      {attachments.length > 0 && (
        <div
          data-testid="chat-composer-attachments"
          className="flex items-center gap-2 px-3 pt-2 pb-1 overflow-x-auto scrollbar-none"
        >
          {attachments.map((att) => {
            const id = att.receipt.attachmentId;
            return (
              <div
                key={id}
                data-testid={`attachment-preview-${id}`}
                className="group relative flex items-center gap-2 rounded-lg bg-chat-surface border border-chat-border p-1.5 pr-1 shrink-0 max-w-[15rem]"
              >
                <div className="flex size-8 items-center justify-center rounded bg-chat-surface-raised text-chat-foreground-secondary">
                  <FileText className="size-4" aria-hidden="true" />
                </div>
                <div className="flex flex-col min-w-0 flex-1">
                  <span className="truncate text-xs font-mono font-medium text-chat-foreground">
                    {att.displayName}
                  </span>
                  <span className="truncate text-xs font-mono text-chat-foreground-secondary">
                    {formatFileSize(att.receipt.sizeBytes)} · {att.mentionText.trim()}
                  </span>
                </div>
                <button
                  type="button"
                  data-testid={`remove-attachment-${id}`}
                  aria-label={"Remove staged file " + att.displayName}
                  onClick={() => onRemoveAttachment?.(id)}
                  className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <X className="size-4" aria-hidden="true" />
                </button>
              </div>
            );
          })}
        </div>
      )}
      {heldMessages.length > 0 && (
        <div
          data-testid="chat-composer-held"
          role="group"
          aria-label="Held messages"
          className="flex flex-col gap-1 px-3 pt-2 pb-1 border-b border-chat-composer-border/60"
        >
          <span className="text-xs text-chat-foreground-secondary">
            {heldUnsaved
              ? "Held here — this device could not save them. Send or remove one yourself."
              : "Held until you send them. Nothing is sent on its own."}
          </span>
          {heldMessages.map((message) => {
            const sending = heldSendingIds.includes(message.id);
            return (
              <div
                key={message.id}
                data-testid={`held-message-${message.id}`}
                className="flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface py-0.5 pl-3 pr-0.5"
              >
                {editingHeldId === message.id ? (
                  <input
                    data-testid={`held-message-input-${message.id}`}
                    aria-label={"Edit held message " + heldName(message.text)}
                    className="min-h-11 min-w-0 flex-1 rounded-md border border-chat-border bg-chat-screen px-2 py-1 text-base text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    value={editingHeldText}
                    disabled={sending}
                    onChange={(event) => setEditingHeldText(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !isImeConfirmingKey(event, false)) {
                        event.preventDefault();
                        commitHeldEdit();
                      }
                    }}
                  />
                ) : (
                  <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-sm text-chat-foreground">
                    {message.text}
                  </span>
                )}
                <span className="flex shrink-0 items-center gap-0.5">
                  <button
                    type="button"
                    data-testid={`held-message-send-${message.id}`}
                    aria-label={"Send held message " + heldName(message.text)}
                    disabled={sending || editingHeldId === message.id}
                    onClick={() => onSendHeld?.(message.id)}
                    className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {sending ? (
                      <Loader2 aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
                    ) : (
                      <ArrowUp aria-hidden="true" className="size-4" />
                    )}
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-edit-${message.id}`}
                    aria-label={"Edit held message " + heldName(message.text)}
                    disabled={sending}
                    onClick={() => (editingHeldId === message.id ? commitHeldEdit() : beginHeldEdit(message))}
                    className="flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Pencil aria-hidden="true" className="size-4" />
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-remove-${message.id}`}
                    aria-label={"Remove held message " + heldName(message.text)}
                    disabled={sending}
                    onClick={() => onRemoveHeld?.(message.id)}
                    className="ml-2 flex size-11 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-danger disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Trash2 aria-hidden="true" className="size-4" />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      )}
      {promptCard !== undefined && promptCard !== null && (
        <div data-testid="chat-composer-prompt" className="px-3 pt-2">
          {promptCard}
        </div>
      )}
      </div>
      )}

      {/* One card, text first under it: single-surface wrapper + control row ported from paseo composer/input/input.tsx (Apache-2.0) and t3code ThreadComposer.tsx (MIT). */}
      <div className="flex flex-col px-3 py-2">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={handleFileChange}
          data-testid="file-upload-input"
        />

        <div className="rounded-[20px] border border-chat-composer-border bg-chat-composer-surface transition-colors focus-within:border-chat-foreground-secondary/40">
          <textarea
            ref={textareaRef}
            data-testid="chat-composer-textarea"
            rows={1}
            value={text}
            disabled={disabled}
            placeholder={placeholder}
            aria-label={textareaLabel}
            enterKeyHint={coarsePointer() ? "enter" : "send"}
            autoComplete="off"
            inputMode="text"
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onCompositionStart={handleCompositionStart}
            onCompositionEnd={handleCompositionEnd}
            className="w-full resize-none bg-transparent px-3.5 pt-2.5 font-sans text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring max-h-36 overflow-y-auto leading-relaxed scrollbar-sleek"
          />

          {/* Stop sits at the left edge, Send at the right: a slip never turns an answer into a stop */}
          <div className="flex items-center justify-between gap-3 px-1 pb-1">
            <div className="flex items-center gap-1">
              <button
                type="button"
                data-testid="attach-file-button"
                aria-label="Attach file"
                title="Attach file"
                aria-busy={attaching || undefined}
                disabled={disabled || attaching}
                onClick={() => fileInputRef.current?.click()}
                className="flex size-11 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary hover:text-chat-foreground active:bg-chat-surface-raised transition-colors disabled:opacity-40 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <Plus aria-hidden="true" className="size-5" />
              </button>

              {isRunning && (
                <button
                  type="button"
                  data-testid="stop-button"
                  aria-label={stopArmed ? "Tap again to stop the running turn" : "Stop the running turn"}
                  title="Stop the running turn"
                  onClick={handleStop}
                  className="flex h-11 shrink-0 items-center gap-1.5 rounded-full bg-chat-danger px-4 text-sm font-medium text-chat-screen hover:bg-chat-danger/90 active:bg-chat-danger/80 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Square aria-hidden="true" className="size-3.5 fill-current" />
                  <span aria-hidden="true">{stopArmed ? "Tap to confirm" : "Stop"}</span>
                </button>
              )}
            </div>

            {showsSend && (
              <button
                type="button"
                data-testid="send-button"
                aria-label="Send message"
                title="Send message"
                disabled={!canSubmit}
                onClick={handleSend}
                className={cn(
                  "flex size-11 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                  canSubmit
                    ? "bg-chat-primary text-chat-primary-foreground hover:bg-chat-primary/90"
                    : "bg-chat-surface-raised text-chat-foreground-secondary cursor-not-allowed"
                )}
              >
                <ArrowUp aria-hidden="true" className="size-5 stroke-[2.5]" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});

MobileChatComposer.displayName = "MobileChatComposer";
