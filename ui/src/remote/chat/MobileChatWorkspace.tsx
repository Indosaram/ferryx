import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  ChevronLeft,
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

  readonly sessionId?: string;
  readonly token?: string;
  readonly transportUrl?: string;
  readonly terminalTitle?: string;
  readonly isAccountSession?: boolean;
  readonly createWebSocket?: (pathAndQuery: string) => any;

  readonly className?: string;
  readonly warnings?: readonly string[];
  readonly composerPlaceholder?: string;
  readonly disabled?: boolean;

  readonly headerTitle?: string;
  readonly headerSubtitle?: string;
  readonly onBack?: () => void;
  readonly headerActions?: React.ReactNode;
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
  sessionId,
  token,
  transportUrl,
  terminalTitle = "Raw PTY Terminal",
  isAccountSession,
  createWebSocket,
  className,
  warnings,
  composerPlaceholder,
  disabled = false,
  headerTitle,
  headerSubtitle,
  onBack,
  headerActions,
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
        "relative flex flex-col w-full h-full min-h-0 bg-chat-screen text-chat-foreground overflow-hidden select-none",
        className
      )}
    >
      <header
        data-testid="mobile-chat-header"
        className="flex items-center justify-between px-3 py-2 bg-chat-screen border-b border-chat-border backdrop-blur-md shrink-0 z-10"
      >
        <div className="flex items-center gap-2 min-w-0">
          {onBack && (
            <button
              type="button"
              data-testid="thread-header-back"
              aria-label="Back"
              onClick={onBack}
              className="flex size-7 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <ChevronLeft className="size-4" aria-hidden="true" />
            </button>
          )}
          <div className="flex flex-col min-w-0">
            <span className="text-base font-medium text-chat-foreground tracking-tight">
              {headerTitle ?? "Agent Workspace"}
            </span>
            {headerSubtitle ? (
              <span
                data-testid="chat-header-subtitle"
                className="font-mono text-xs text-chat-foreground-secondary truncate"
              >
                {headerSubtitle}
              </span>
            ) : workspaceLabel ? (
              <span
                data-testid="chat-header-subtitle"
                className="font-mono text-xs text-chat-foreground-secondary truncate"
              >
                {workspaceLabel}
                {worktreeLabel ? ` · ${worktreeLabel}` : ""}
              </span>
            ) : (
              <div className="flex items-center gap-1.5">
                <span className="inline-block size-1.5 rounded-full bg-status-success shrink-0" />
                <span className="font-mono text-xs text-chat-foreground-secondary truncate">
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
              aria-label={isTerminalOpen ? "Hide terminal" : "Show terminal"}
              aria-pressed={isTerminalOpen}
              onClick={() => setIsTerminalOpen((prev) => !prev)}
              className={cn(
                "flex size-7 shrink-0 items-center justify-center rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                isTerminalOpen
                  ? "bg-chat-surface-raised text-chat-foreground border-chat-border shadow-xs"
                  : "bg-chat-surface/90 text-chat-foreground-secondary border-chat-border hover:bg-chat-surface-hover hover:text-chat-foreground"
              )}
            >
              <TerminalIcon className="size-3.5" aria-hidden="true" />
            </button>
          )}

          {headerActions}
        </div>
      </header>

      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        data-testid="chat-message-stream"
        aria-busy={isRunning}
        className="flex-1 min-h-0 overflow-y-auto px-5 py-4 space-y-3.5 scroll-smooth overscroll-contain select-text"
      >
        {warnings && warnings.length > 0 ? (
          <div
            data-testid="chat-history-warning"
            role="status"
            className="mb-3 rounded-md border border-chat-border bg-chat-surface px-3 py-2 font-mono text-xs text-chat-foreground-secondary"
          >
            {warnings.join(" ")}
          </div>
        ) : null}
        {!hasMessages ? (
          <div
            data-testid="chat-empty-state"
            className="flex flex-col items-start justify-center min-h-[40vh] max-w-md mx-auto w-full px-2 my-auto select-none"
          >
            <div className="w-full rounded-lg border border-chat-border bg-chat-surface px-3 py-2.5 space-y-1">
              <div
                data-testid="chat-empty-context"
                className="font-mono text-xs text-chat-foreground-secondary truncate"
              >
                {workspaceLabel
                  ? `${workspaceLabel}${worktreeLabel ? ` · ${worktreeLabel}` : ""}`
                  : "ferryx remote"}
              </div>
              <p className="font-mono text-xs text-chat-foreground-secondary">
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

      <footer className="relative z-20 shrink-0">
        {canShowTerminal && (
          <div
            data-testid="terminal-drawer"
            aria-hidden={!isTerminalOpen}
            {...(!isTerminalOpen ? { inert: "" } : {})}
            className={cn(
              "absolute inset-x-0 bottom-full flex flex-col w-full bg-chat-screen/95 border-t border-chat-border backdrop-blur-xl shadow-2xl transition-[opacity,transform] duration-300 ease-out",
              isTerminalOpen
                ? "opacity-100 translate-y-0 pointer-events-auto"
                : "opacity-0 translate-y-3 pointer-events-none",
              isTerminalExpanded ? "h-[85vh]" : "h-[45vh]"
            )}
          >
            <div className="flex items-center justify-between px-3 py-2 bg-chat-surface/90 border-b border-chat-border select-none">
              <div className="flex items-center gap-2">
                <TerminalIcon className="size-3.5 text-status-success" />
                <span className="text-xs font-mono font-semibold text-chat-foreground">
                  {terminalTitle}
                </span>
              </div>

              <div className="flex items-center gap-1">
                <button
                  type="button"
                  data-testid="terminal-expand-button"
                  aria-label={isTerminalExpanded ? "Collapse terminal" : "Expand terminal"}
                  onClick={() => setIsTerminalExpanded((prev) => !prev)}
                  className="p-1.5 rounded-md text-chat-foreground-secondary hover:text-chat-foreground hover:bg-chat-surface-raised/80 active:scale-95 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
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
                  aria-label="Close terminal"
                  onClick={() => setIsTerminalOpen(false)}
                  className="p-1.5 rounded-md text-chat-foreground-secondary hover:text-chat-foreground hover:bg-chat-surface-raised/80 active:scale-95 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  title="Close terminal"
                >
                  <X className="size-3.5" />
                </button>
              </div>
            </div>

            <div className="flex-1 min-h-0 w-full relative overflow-hidden bg-terminal">
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

        <div className="relative">
          {showScrollPill && !isTerminalOpen && (
            <div className="absolute -top-12 right-4 z-20">
              <button
                type="button"
                data-testid="scroll-to-latest-pill"
                onClick={() => {
                  setIsAtBottom(true);
                  scrollToBottom(true);
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-chat-surface-raised/95 hover:bg-chat-surface-hover text-chat-foreground text-xs font-medium shadow-lg border border-chat-border backdrop-blur-md active:scale-95 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <ArrowDown className="size-3.5 text-chat-code" />
                <span>Scroll to latest</span>
              </button>
            </div>
          )}
          <MobileChatComposer
            onSend={onSendMessage}
            onStop={onStopExecution}
            isRunning={isRunning}
            disabled={disabled}
            placeholder={composerPlaceholder}
          />
        </div>
      </footer>
    </div>
  );
};
