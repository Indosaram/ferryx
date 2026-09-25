import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowUp,
  Mic,
  Plus,
  Square,
  X,
  FileText,
} from "lucide-react";
import { cn } from "../../lib/cn";
import {
  MobileChatQuickActions,
  type QuickActionItem,
} from "./MobileChatQuickActions";

export interface ChatAttachment {
  readonly id: string;
  readonly name: string;
  readonly size: number;
  readonly type: string;
  readonly url?: string;
  readonly file?: File;
}

export interface MobileChatComposerHandle {
  clearDraft: () => void;
}

export interface MobileChatComposerProps {
  readonly onSend: (text: string, attachments: readonly ChatAttachment[]) => void;
  readonly onStop?: () => void;
  readonly onClearDraft?: () => void;
  readonly isRunning?: boolean;
  readonly disabled?: boolean;
  readonly placeholder?: string;
  readonly quickActions?: readonly QuickActionItem[];
  readonly onSelectQuickAction?: (action: QuickActionItem) => void;
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
  onSend,
  onStop,
  onClearDraft,
  isRunning = false,
  disabled = false,
  placeholder = "Ask the repo agent, or run a command...",
  quickActions,
  onSelectQuickAction,
  className,
}, ref) => {
  const [text, setText] = useState("");
  const [history, setHistory] = useState<readonly string[]>([]);
  const [historyIdx, setHistoryIdx] = useState<number>(-1);
  const [attachments, setAttachments] = useState<readonly ChatAttachment[]>([]);
  const objectUrlsRef = useRef<Set<string>>(new Set());
  const isComposingRef = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const revokeTrackedUrl = useCallback((url?: string) => {
    if (!url) return;
    if (objectUrlsRef.current.has(url)) {
      URL.revokeObjectURL(url);
      objectUrlsRef.current.delete(url);
    }
  }, []);

  const revokeAllTrackedUrls = useCallback(() => {
    objectUrlsRef.current.forEach((url) => {
      URL.revokeObjectURL(url);
    });
    objectUrlsRef.current.clear();
  }, []);

  useEffect(() => {
    return () => {
      revokeAllTrackedUrls();
    };
  }, [revokeAllTrackedUrls]);

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
    setAttachments([]);
    revokeAllTrackedUrls();
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
    onClearDraft?.();
  }, [revokeAllTrackedUrls, onClearDraft]);

  React.useImperativeHandle(ref, () => ({
    clearDraft,
  }), [clearDraft]);

  const handleSend = useCallback(() => {
    if (disabled) return;
    const trimmed = text.trim();
    if (!trimmed && attachments.length === 0) return;
    const pendingAttachments = attachments;
    if (trimmed) {
      setHistory((prev) => (prev[prev.length - 1] === trimmed ? prev : [...prev, trimmed]));
      setHistoryIdx(-1);
    }
    onSend(trimmed, pendingAttachments);
    setText("");
    setAttachments([]);
    revokeAllTrackedUrls();
    if (textareaRef.current) {
      textareaRef.current.style.height = "36px";
    }
  }, [disabled, text, attachments, onSend, revokeAllTrackedUrls]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        if (isComposingRef.current) return;
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

      const newAttachments: ChatAttachment[] = Array.from(files).map((file) => {
        const isImage = file.type.startsWith("image/");
        let url: string | undefined;
        if (isImage) {
          url = URL.createObjectURL(file);
          objectUrlsRef.current.add(url);
        }
        return {
          id: `${Date.now()}-${Math.random().toString(36).substring(2, 9)}`,
          name: file.name,
          size: file.size,
          type: file.type,
          url,
          file,
        };
      });

      setAttachments((prev) => [...prev, ...newAttachments]);
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    },
    []
  );

  const handleRemoveAttachment = useCallback(
    (id: string) => {
      setAttachments((prev) => {
        const target = prev.find((a) => a.id === id);
        if (target?.url) {
          revokeTrackedUrl(target.url);
        }
        return prev.filter((a) => a.id !== id);
      });
    },
    [revokeTrackedUrl]
  );

  const handleQuickAction = useCallback(
    (action: QuickActionItem) => {
      if (action.id === "stop" || action.isDestructive) {
        onStop?.();
        return;
      }
      if (onSelectQuickAction) {
        onSelectQuickAction(action);
      } else {
        setText(action.prompt);
        textareaRef.current?.focus();
      }
    },
    [onStop, onSelectQuickAction]
  );

  const insertTextAtCursor = useCallback((inserted: string) => {
    const el = textareaRef.current;
    if (!el) {
      setText((prev) => prev + inserted);
      return;
    }
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const next = el.value.substring(0, start) + inserted + el.value.substring(end);
    setText(next);
    setTimeout(() => {
      el.focus();
      el.setSelectionRange(start + inserted.length, start + inserted.length);
    }, 0);
  }, []);

  const handleAccessoryKey = useCallback(
    (keyType: "esc" | "tab" | "ctrl-c" | "up" | "down" | "clear") => {
      if (disabled) return;
      switch (keyType) {
        case "ctrl-c": {
          if (isRunning && onStop) {
            onStop();
          } else {
            onSend("\x03", []);
          }
          break;
        }
        case "esc": {
          if (text.length > 0) {
            setText("");
            if (textareaRef.current) {
              textareaRef.current.style.height = "36px";
            }
          } else {
            onSend("\x1b", []);
          }
          break;
        }
        case "tab": {
          insertTextAtCursor("  ");
          break;
        }
        case "clear": {
          onSend("clear", []);
          break;
        }
        case "up": {
          if (history.length === 0) break;
          const nextIdx = historyIdx === -1 ? history.length - 1 : Math.max(0, historyIdx - 1);
          setHistoryIdx(nextIdx);
          setText(history[nextIdx] ?? "");
          break;
        }
        case "down": {
          if (history.length === 0 || historyIdx === -1) break;
          const nextIdx = historyIdx + 1;
          if (nextIdx >= history.length) {
            setHistoryIdx(-1);
            setText("");
          } else {
            setHistoryIdx(nextIdx);
            setText(history[nextIdx] ?? "");
          }
          break;
        }
      }
    },
    [disabled, isRunning, onStop, onSend, text, insertTextAtCursor, history, historyIdx]
  );

  const canSubmit = (text.trim().length > 0 || attachments.length > 0) && !disabled;

  return (
    <div
      data-testid="mobile-chat-composer"
      className={cn(
        "flex flex-col w-full bg-zinc-950/95 border-t border-border/80 backdrop-blur-md pb-safe select-none",
        className
      )}
    >
      <MobileChatQuickActions
        onSelectAction={handleQuickAction}
        isRunning={isRunning}
        actions={quickActions}
        disabled={disabled}
      />

      <div
        data-testid="terminal-accessory-bar"
        className="flex items-center gap-1.5 px-3 py-1 overflow-x-auto no-scrollbar border-t border-border/40 bg-zinc-900/60"
      >
        <button
          type="button"
          data-testid="accessory-key-esc"
          disabled={disabled}
          onClick={() => handleAccessoryKey("esc")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          ESC
        </button>
        <button
          type="button"
          data-testid="accessory-key-tab"
          disabled={disabled}
          onClick={() => handleAccessoryKey("tab")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          Tab
        </button>
        <button
          type="button"
          data-testid="accessory-key-ctrl-c"
          disabled={disabled}
          onClick={() => handleAccessoryKey("ctrl-c")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          Ctrl+C
        </button>
        <button
          type="button"
          data-testid="accessory-key-up"
          disabled={disabled}
          onClick={() => handleAccessoryKey("up")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          ↑
        </button>
        <button
          type="button"
          data-testid="accessory-key-down"
          disabled={disabled}
          onClick={() => handleAccessoryKey("down")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          ↓
        </button>
        <button
          type="button"
          data-testid="accessory-key-clear"
          disabled={disabled}
          onClick={() => handleAccessoryKey("clear")}
          className="font-mono text-[10px] px-2 py-0.5 rounded border border-border/60 bg-secondary/40 text-muted-foreground active:bg-accent hover:text-foreground transition-colors disabled:opacity-40 shrink-0"
        >
          /clear
        </button>
      </div>

      {attachments.length > 0 && (
        <div
          data-testid="chat-composer-attachments"
          className="flex items-center gap-2 px-3 pt-2 pb-1 overflow-x-auto no-scrollbar"
        >
          {attachments.map((att) => {
            const isImage = att.type.startsWith("image/") && att.url;
            return (
              <div
                key={att.id}
                data-testid={`attachment-preview-${att.id}`}
                className="group relative flex items-center gap-2 rounded-lg bg-zinc-900/90 border border-border/80 p-1.5 pr-2.5 shrink-0 max-w-[200px] shadow-sm"
              >
                {isImage ? (
                  <img
                    src={att.url}
                    alt={att.name}
                    className="size-8 rounded object-cover bg-zinc-950 border border-border/60"
                  />
                ) : (
                  <div className="flex size-8 items-center justify-center rounded bg-secondary/40 text-muted-foreground border border-border/50">
                    <FileText className="size-4" />
                  </div>
                )}
                <div className="flex flex-col min-w-0 flex-1">
                  <span className="truncate text-xs font-mono font-medium text-foreground">
                    {att.name}
                  </span>
                  <span className="text-[10px] font-mono text-muted-foreground">
                    {formatFileSize(att.size)}
                  </span>
                </div>
                <button
                  type="button"
                  data-testid={`remove-attachment-${att.id}`}
                  onClick={() => handleRemoveAttachment(att.id)}
                  className="size-5 rounded-md bg-secondary/60 hover:bg-destructive/80 hover:text-destructive-foreground text-muted-foreground flex items-center justify-center shrink-0 transition-colors"
                >
                  <X className="size-3" />
                </button>
              </div>
            );
          })}
        </div>
      )}

      <div className="flex items-end gap-2 px-2.5 py-2">
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
          disabled={disabled}
          onClick={() => fileInputRef.current?.click()}
          className="flex size-9 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:text-foreground active:bg-secondary/60 transition-colors disabled:opacity-40 disabled:pointer-events-none"
        >
          <Plus className="size-4" />
        </button>

        <div className="relative flex min-h-[38px] flex-1 items-center rounded-xl bg-zinc-900/90 border border-border/80 focus-within:border-primary/80 focus-within:ring-1 focus-within:ring-primary/30 px-3 py-1.5 transition-all">
          <textarea
            ref={textareaRef}
            data-testid="chat-composer-textarea"
            rows={1}
            value={text}
            disabled={disabled}
            placeholder={placeholder}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onCompositionStart={handleCompositionStart}
            onCompositionEnd={handleCompositionEnd}
            className="w-full resize-none bg-transparent font-sans text-sm text-foreground placeholder:text-muted-foreground/60 focus:outline-none max-h-36 overflow-y-auto leading-relaxed scrollbar-thin"
          />
        </div>

        <button
          type="button"
          data-testid="mic-button"
          disabled={disabled}
          className="flex size-9 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:text-foreground active:bg-secondary/60 transition-colors disabled:opacity-40 disabled:pointer-events-none"
        >
          <Mic className="size-4" />
        </button>

        {isRunning ? (
          <button
            type="button"
            data-testid="stop-button"
            onClick={onStop}
            className="flex size-9 shrink-0 items-center justify-center rounded-full bg-red-600 text-white shadow-sm hover:bg-red-700 active:bg-red-800 active:scale-95 transition-all"
          >
            <Square className="size-4 fill-current" />
          </button>
        ) : (
          <button
            type="button"
            data-testid="send-button"
            disabled={!canSubmit}
            onClick={handleSend}
            className={cn(
              "flex size-9 shrink-0 items-center justify-center rounded-full transition-all active:scale-95 shadow-sm",
              canSubmit
                ? "bg-primary text-primary-foreground hover:brightness-110 active:brightness-95"
                : "bg-secondary/40 text-muted-foreground/50 border border-border/40 cursor-not-allowed"
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
