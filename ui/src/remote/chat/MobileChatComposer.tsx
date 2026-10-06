import React, { useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowUp,
  FileText,
  Loader2,
  Mic,
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

export type { ChatAttachment };

export interface MobileChatComposerHandle {
  clearDraft: () => void;
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
  readonly heldSendingId?: string | null;
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
  heldSendingId = null,
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

  useImperativeHandle(ref, () => ({
    clearDraft,
  }), [clearDraft]);

  /** Where a mention goes: the caret the user left in the box. */
  const caret = useCallback((): number => {
    const el = textareaRef.current;
    return el !== null && typeof el.selectionStart === "number" ? el.selectionStart : text.length;
  }, [text.length]);

  const handleSend = useCallback(() => {
    // A running pane still takes typed text (that is how a TUI is driven), and a prompt waiting
    // on the original pane takes a typed answer: the owner decides what the text means.
    if (disabled) return;
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
  }, [disabled, text, onSend, setText]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        if (
          isComposingRef.current ||
          Boolean(e.nativeEvent?.isComposing) ||
          Boolean((e as unknown as { isComposing?: boolean }).isComposing) ||
          e.keyCode === 229 ||
          (e.nativeEvent as KeyboardEvent | undefined)?.keyCode === 229
        ) {
          return;
        }
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

  const canSubmit = text.trim().length > 0 && !disabled;
  // A prompt waiting on the original pane is a question this box answers, so the box keeps Send
  // beside Stop while one is up. Without a prompt, a running pane shows Stop alone.
  const answersWaitingPrompt = promptCard !== undefined && promptCard !== null;
  const showsSend = !isRunning || answersWaitingPrompt;

  return (
    <div
      data-testid="mobile-chat-composer"
      className={cn(
        "flex flex-col w-full bg-chat-composer-panel border-t border-chat-composer-border rounded-2xl backdrop-blur-md pb-safe select-none",
        className
      )}
    >
      {draftUnsaved && (
        <div
          data-testid="chat-composer-draft-unsaved"
          role="status"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-[11px] text-chat-danger"
        >
          <AlertCircle aria-hidden="true" className="size-3 shrink-0" />
          <span>This draft could not be saved on this device. It stays here until you send it.</span>
        </div>
      )}
      {warning !== null && (
        <div
          data-testid="chat-composer-warning"
          role="status"
          aria-live="polite"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-[11px] text-chat-foreground-secondary"
        >
          <span>{warning}</span>
        </div>
      )}
      {attachError !== null && (
        <div
          data-testid="chat-composer-attach-error"
          role="alert"
          className="flex items-center gap-2 px-3 pt-2 pb-1 text-[11px] text-chat-danger"
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
                className="group relative flex items-center gap-2 rounded-lg bg-chat-surface/90 border border-chat-border p-1.5 pr-2.5 shrink-0 max-w-[220px] shadow-sm"
              >
                <div className="flex size-8 items-center justify-center rounded bg-chat-surface-raised/60 text-chat-foreground-secondary border border-chat-border/50">
                  <FileText className="size-4" aria-hidden="true" />
                </div>
                <div className="flex flex-col min-w-0 flex-1">
                  <span className="truncate text-xs font-mono font-medium text-chat-foreground">
                    {att.displayName}
                  </span>
                  <span className="truncate text-[10px] font-mono text-chat-foreground-secondary">
                    {formatFileSize(att.receipt.sizeBytes)} · {att.mentionText.trim()}
                  </span>
                </div>
                <button
                  type="button"
                  data-testid={`remove-attachment-${id}`}
                  aria-label={"Remove staged file " + att.displayName}
                  onClick={() => onRemoveAttachment?.(id)}
                  className="size-5 rounded-md bg-chat-surface-raised hover:bg-chat-danger/80 hover:text-chat-screen text-chat-foreground-secondary flex items-center justify-center shrink-0 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <X className="size-3" aria-hidden="true" />
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
          <span className="text-[11px] text-chat-foreground-secondary">
            {heldUnsaved
              ? "Held here — this device could not save them. Send or remove one yourself."
              : "Held until you send them. Nothing is sent on its own."}
          </span>
          {heldMessages.map((message) => {
            const sending = heldSendingId === message.id;
            return (
              <div
                key={message.id}
                data-testid={`held-message-${message.id}`}
                className="flex items-start gap-2 rounded-lg border border-chat-border bg-chat-surface/70 px-2 py-1.5"
              >
                {editingHeldId === message.id ? (
                  <input
                    data-testid={`held-message-input-${message.id}`}
                    className="min-w-0 flex-1 rounded-md border border-chat-border bg-chat-screen/60 px-2 py-1 text-xs text-chat-foreground"
                    value={editingHeldText}
                    disabled={sending}
                    onChange={(event) => setEditingHeldText(event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                        event.preventDefault();
                        commitHeldEdit();
                      }
                    }}
                  />
                ) : (
                  <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-xs text-chat-foreground-secondary">
                    {message.text}
                  </span>
                )}
                <span className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    data-testid={`held-message-send-${message.id}`}
                    aria-label={"Send held message " + message.text}
                    disabled={sending || editingHeldId === message.id}
                    onClick={() => onSendHeld?.(message.id)}
                    className="flex size-6 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {sending ? (
                      <Loader2 aria-hidden="true" className="size-3 animate-spin motion-reduce:animate-none" />
                    ) : (
                      <ArrowUp aria-hidden="true" className="size-3" />
                    )}
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-edit-${message.id}`}
                    aria-label={"Edit held message " + message.text}
                    disabled={sending}
                    onClick={() => (editingHeldId === message.id ? commitHeldEdit() : beginHeldEdit(message))}
                    className="flex size-6 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-foreground disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Pencil aria-hidden="true" className="size-3" />
                  </button>
                  <button
                    type="button"
                    data-testid={`held-message-remove-${message.id}`}
                    aria-label={"Remove held message " + message.text}
                    disabled={sending}
                    onClick={() => onRemoveHeld?.(message.id)}
                    className="flex size-6 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-danger disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <Trash2 aria-hidden="true" className="size-3" />
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

      <div className="flex items-end gap-2 px-[12px] py-2">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={handleFileChange}
          data-testid="file-upload-input"
        />

        <button
          type="button"
          data-testid="attach-file-button"
          aria-label="Attach file"
          aria-busy={attaching || undefined}
          disabled={disabled || attaching}
          onClick={() => fileInputRef.current?.click()}
          className="flex size-9 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary hover:text-chat-foreground active:bg-chat-surface-raised transition-colors disabled:opacity-40 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <Plus className="size-4" />
        </button>

        <div className="relative flex min-h-[38px] flex-1 items-center rounded-xl bg-chat-composer-surface border border-chat-composer-border focus-within:border-chat-primary/80 focus-within:ring-1 focus-within:ring-chat-primary/30 px-[14px] pb-2.5 pt-1.5 transition-all">
          <textarea
            ref={textareaRef}
            data-testid="chat-composer-textarea"
            rows={1}
            value={text}
            disabled={disabled}
            placeholder={placeholder}
            aria-label="Ask the repo agent"
            enterKeyHint="send"
            autoComplete="off"
            inputMode="text"
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onCompositionStart={handleCompositionStart}
            onCompositionEnd={handleCompositionEnd}
            className="w-full resize-none bg-transparent font-sans text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring max-h-36 overflow-y-auto leading-relaxed scrollbar-sleek"
          />
        </div>

        <button
          type="button"
          data-testid="mic-button"
          disabled
          aria-label="Voice input is not supported"
          title="Voice input is not supported"
          className="flex size-9 shrink-0 items-center justify-center rounded-full text-chat-foreground-secondary hover:text-chat-foreground active:bg-chat-surface-raised transition-colors disabled:opacity-40 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <Mic className="size-4" />
        </button>

        {isRunning && (
          <button
            type="button"
            data-testid="stop-button"
            aria-label="Stop the running turn"
            onClick={onStop}
            className="flex size-9 shrink-0 items-center justify-center rounded-full bg-chat-danger text-chat-foreground shadow-sm hover:bg-chat-danger/90 active:bg-chat-danger/80 active:scale-95 transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <Square className="size-4 fill-current" />
          </button>
        )}
        {showsSend && (
          <button
            type="button"
            data-testid="send-button"
            aria-label="Send message"
            disabled={!canSubmit}
            onClick={handleSend}
            className={cn(
              "flex size-9 shrink-0 items-center justify-center rounded-full transition-all active:scale-95 shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
              canSubmit
                ? "bg-chat-primary text-chat-primary-foreground hover:brightness-110 active:brightness-95"
                : "bg-chat-surface-raised/40 text-chat-foreground-secondary/50 border border-chat-border/40 cursor-not-allowed"
            )}
          >
            <ArrowUp className="size-4 stroke-[2.5]" />
          </button>
        )}
      </div>
    </div>
  );
});

MobileChatComposer.displayName = "MobileChatComposer";
