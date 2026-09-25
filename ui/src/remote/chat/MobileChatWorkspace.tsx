import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  Maximize2,
  Minimize2,
  Terminal as TerminalIcon,
  X,
} from "lucide-react";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
} from "./MobileChatComponents";
import {
  MobileChatMessage,
  MobileChatMessageProps,
} from "./MobileChatMessage";
import {
  MobileChatComposer,
  ChatAttachment,
} from "./MobileChatComposer";
import {
  QuickActionItem,
} from "./MobileChatQuickActions";
import { RemoteTerminal } from "../RemoteTerminal";

export interface MobileChatWorkspaceProps {
  readonly messages: readonly MobileChatMessageProps[];
  readonly onSendMessage: (text: string, attachments: readonly ChatAttachment[]) => void;
  readonly onStopExecution?: () => void;
  readonly isRunning?: boolean;
  readonly activityState?: ActivityState;
  readonly activityLabel?: string;
  readonly workspaceLabel?: string;
  readonly worktreeLabel?: string;
  readonly quickActions?: readonly QuickActionItem[];
  readonly onSelectQuickAction?: (action: QuickActionItem) => void;

  readonly sessionId?: string;
  readonly token?: string;
  readonly transportUrl?: string;
  readonly terminalTitle?: string;
  readonly isAccountSession?: boolean;
  readonly createWebSocket?: (pathAndQuery: string) => any;

  readonly className?: string;
  readonly composerPlaceholder?: string;
  readonly disabled?: boolean;
}

export const MobileChatWorkspace: React.FC<MobileChatWorkspaceProps> = ({
  messages,
  onSendMessage,
  onStopExecution,
  isRunning = false,
  activityState = "idle",
  activityLabel,
  workspaceLabel,
  worktreeLabel,
  quickActions,
  onSelectQuickAction,
  sessionId,
  token,
  transportUrl,
  terminalTitle = "Raw PTY Terminal",
  isAccountSession,
  createWebSocket,
  className,
  composerPlaceholder,
  disabled = false,
}) => {
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const [isAtBottom, setIsAtBottom] = useState(true);
  const [showScrollPill, setShowScrollPill] = useState(false);

  const [isTerminalOpen, setIsTerminalOpen] = useState(false);
  const [isTerminalExpanded, setIsTerminalExpanded] = useState(false);

  const scrollToBottom = useCallback((smooth = true) => {
    if (messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({
        behavior: smooth ? "smooth" : "auto",
        block: "end",
      });
    }
  }, []);

  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current;
    if (!el) return;

    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    const atBottom = distanceFromBottom < 48;

    setIsAtBottom(atBottom);
    setShowScrollPill(distanceFromBottom > 100);
  }, []);

  useEffect(() => {
    if (isAtBottom) {
      scrollToBottom(true);
    }
  }, [messages, isRunning, activityState, isAtBottom, scrollToBottom]);

  const hasMessages = messages.length > 0;
  const canShowTerminal = Boolean(sessionId && token);

  return (
    <div
      data-testid="mobile-chat-workspace"
      className={cn(
        "relative flex flex-col w-full h-full min-h-0 bg-zinc-950 text-zinc-100 overflow-hidden select-none",
        className
      )}
    >
      <header
        data-testid="mobile-chat-header"
        className="flex items-center justify-between px-3 py-2 bg-zinc-950/90 border-b border-border/60 backdrop-blur-md shrink-0 z-10"
      >
        <div className="flex items-center gap-2 min-w-0">
          <div className="flex size-7 items-center justify-center rounded-md bg-zinc-900 border border-border/70 text-zinc-300 shrink-0 shadow-xs">
            <TerminalIcon className="size-3.5 text-zinc-300" />
          </div>
          <div className="flex flex-col min-w-0">
            <span className="text-xs font-semibold tracking-tight text-foreground/90 font-mono">
              Agent Workspace
            </span>
            {workspaceLabel ? (
              <span
                data-testid="chat-header-subtitle"
                className="text-[10px] text-muted-foreground font-mono truncate"
              >
                {workspaceLabel}
                {worktreeLabel ? ` · ${worktreeLabel}` : ""}
              </span>
            ) : (
              <div className="flex items-center gap-1.5">
                <span className="inline-block size-1.5 rounded-full bg-emerald-500 shrink-0" />
                <span className="text-[10px] text-muted-foreground font-mono truncate">
                  Session: {sessionId ? sessionId.slice(0, 8) : "default"}
                </span>
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {activityState !== "idle" && (
            <ActivityIndicator
              state={activityState}
              label={activityLabel}
              className="scale-90 origin-right"
            />
          )}

          {canShowTerminal && (
            <button
              type="button"
              data-testid="terminal-toggle-button"
              onClick={() => setIsTerminalOpen((prev) => !prev)}
              className={cn(
                "flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-mono font-medium border transition-colors",
                isTerminalOpen
                  ? "bg-zinc-800 text-zinc-100 border-zinc-600 shadow-xs"
                  : "bg-zinc-900/90 text-zinc-400 border-border/80 hover:bg-zinc-800/80 hover:text-zinc-200"
              )}
            >
              <TerminalIcon className="size-3" />
              <span>&gt;_ PTY</span>
            </button>
          )}
        </div>
      </header>

      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        data-testid="chat-message-stream"
        className="flex-1 min-h-0 overflow-y-auto px-3.5 py-4 space-y-3.5 scroll-smooth overscroll-contain select-text"
      >
        {!hasMessages ? (
          <div
            data-testid="chat-empty-state"
            className="flex flex-col items-start justify-center min-h-[40vh] max-w-md mx-auto w-full px-2 my-auto select-none"
          >
            <div className="w-full rounded-lg border border-border/50 bg-zinc-900/40 px-3 py-2.5 space-y-1">
              <div
                data-testid="chat-empty-context"
                className="font-mono text-[11px] text-muted-foreground/80 truncate"
              >
                {workspaceLabel
                  ? `${workspaceLabel}${worktreeLabel ? ` · ${worktreeLabel}` : ""}`
                  : "ferryx remote"}
              </div>
              <p className="text-[11px] text-muted-foreground/60 font-mono">
                Prompts run against the focused terminal.
              </p>
            </div>
          </div>
        ) : (
          <>
            {messages.map((msg) => (
              <MobileChatMessage
                key={msg.id}
                id={msg.id}
                role={msg.role}
                content={msg.content}
                timestamp={msg.timestamp}
                avatarUrl={msg.avatarUrl}
                senderName={msg.senderName}
                attachments={msg.attachments}
                toolCalls={msg.toolCalls}
                approvalAction={msg.approvalAction}
                activityState={msg.activityState}
                durationLabel={msg.durationLabel}
              />
            ))}
          </>
        )}

        <div ref={messagesEndRef} className="h-2 w-full" />
      </div>

      {showScrollPill && (
        <div className="absolute bottom-24 right-4 z-20 transition-all duration-200">
          <button
            type="button"
            data-testid="scroll-to-latest-pill"
            onClick={() => {
              setIsAtBottom(true);
              scrollToBottom(true);
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-zinc-800/95 hover:bg-zinc-700 text-zinc-200 text-xs font-medium shadow-lg border border-zinc-700/80 backdrop-blur-md active:scale-95 transition-all"
          >
            <ArrowDown className="size-3.5 text-sky-400 animate-bounce" />
            <span>Scroll to latest</span>
          </button>
        </div>
      )}

      {canShowTerminal && (
        <div
          data-testid="terminal-drawer"
          className={cn(
            "absolute inset-x-0 bottom-0 z-30 flex flex-col bg-zinc-950/95 border-t border-border backdrop-blur-xl shadow-2xl transition-all duration-300 ease-in-out",
            !isTerminalOpen
              ? "translate-y-full pointer-events-none"
              : isTerminalExpanded
              ? "h-[85vh] translate-y-0"
              : "h-[45vh] translate-y-0"
          )}
        >
          <div className="flex items-center justify-between px-3 py-2 bg-zinc-900/90 border-b border-border select-none">
            <div className="flex items-center gap-2">
              <TerminalIcon className="size-3.5 text-emerald-400" />
              <span className="text-xs font-mono font-semibold text-zinc-200">
                {terminalTitle}
              </span>
            </div>

            <div className="flex items-center gap-1">
              <button
                type="button"
                data-testid="terminal-expand-button"
                onClick={() => setIsTerminalExpanded((prev) => !prev)}
                className="p-1.5 rounded-md text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/80 active:scale-95 transition-colors"
                title={isTerminalExpanded ? "Collapse" : "Expand"}
              >
                {isTerminalExpanded ? (
                  <Minimize2 className="size-3.5" />
                ) : (
                  <Maximize2 className="size-3.5" />
                )}
              </button>
              <button
                type="button"
                data-testid="terminal-close-button"
                onClick={() => setIsTerminalOpen(false)}
                className="p-1.5 rounded-md text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/80 active:scale-95 transition-colors"
                title="Close terminal"
              >
                <X className="size-3.5" />
              </button>
            </div>
          </div>

          <div className="flex-1 min-h-0 w-full relative overflow-hidden bg-black">
            {isTerminalOpen && sessionId && token && (
              <RemoteTerminal
                sessionId={sessionId}
                token={token}
                transportUrl={transportUrl}
                embedded={true}
                isAccountSession={isAccountSession}
                createWebSocket={createWebSocket}
              />
            )}
          </div>
        </div>
      )}

      <footer className="relative z-20 shrink-0">
        <MobileChatComposer
          onSend={onSendMessage}
          onStop={onStopExecution}
          isRunning={isRunning}
          disabled={disabled}
          placeholder={composerPlaceholder}
          quickActions={quickActions}
          onSelectQuickAction={onSelectQuickAction}
        />
      </footer>
    </div>
  );
};
