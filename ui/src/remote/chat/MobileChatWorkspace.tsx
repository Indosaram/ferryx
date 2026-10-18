import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowDown, ChevronLeft, Terminal as TerminalIcon } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
  ReferenceAbandonedBranchDisclosure,
  ReferenceDisclosureBanner,
  ReferenceOlderPageControl,
  ReferenceOlderState,
  ReferencePartRenderContext,
} from "./MobileChatComponents";
import { MobileChatMessage, MobileChatMessageProps } from "./MobileChatMessage";
import { MobileChatComposer, MobileChatComposerHandle } from "./MobileChatComposer";
import type { HeldMessage } from "./referenceQueue";
import type {
  ReferenceAbandonedBranch,
  ReferenceFileReceipt,
} from "./referenceTypes";

/**
 * The chat lens over the original pane (plan task 12).
 *
 * One thing is deliberately absent: this component never mounts a terminal. The chat is the
 * default at every width and the terminal is an explicit mode the owner switches to, so a
 * nested drawer here would be a second owner of the same session and a second place for the
 * geometry to be decided. `onOpenTerminal` asks the owner to switch modes instead.
 *
 * Abandoned branches arrive on individual turns in the frozen DTO, but the reference discloses
 * them once for the page: they are composed here and never forwarded to a turn.
 */
export interface MobileChatWorkspaceProps {
  readonly messages: readonly MobileChatMessageProps[];
  /** `false` means the owner kept the draft (held, refused); the box must not clear it. */
  readonly onSendMessage: (text: string) => boolean | void;
  readonly onStopExecution?: () => void;
  readonly isRunning?: boolean;
  readonly activityState?: ActivityState;
  readonly activityLabel?: string;
  readonly workspaceLabel?: string;
  readonly worktreeLabel?: string;

  /** Switch to the explicit terminal for this same session; never a replacement conversation. */
  readonly onOpenTerminal?: () => void;

  /** The disclosure for this page source; null when the page is native. */
  readonly pageDisclosure?: string | null;
  /** Abandoned branches for the page: composed once here, never per turn. */
  readonly pageAbandoned?: ReferenceAbandonedBranch | null;
  readonly hasOlderPage?: boolean;
  readonly loadedOlder?: boolean;
  readonly olderState?: ReferenceOlderState;
  readonly onLoadOlder?: () => void;

  /** The owner-scoped draft, and the files the owner already staged for this target. */
  readonly draft?: string;
  readonly onDraftChange?: (text: string) => void;
  readonly draftUnsaved?: boolean;
  readonly stagedFiles?: readonly ReferenceFileReceipt[];
  readonly onAttachFiles?: (files: readonly File[], caret: number) => void;
  readonly onRemoveStagedFile?: (attachmentId: string) => void;
  readonly attaching?: boolean;
  readonly attachError?: string | null;

  /** Held messages for this target; only an explicit press sends one. */
  readonly heldMessages?: readonly HeldMessage[];
  /** Held rows whose send is still on its way: each stays disabled until its own answer lands. */
  readonly heldSendingIds?: readonly string[];
  readonly heldUnsaved?: boolean;
  readonly onEditHeld?: (id: string, text: string) => void;
  readonly onSendHeld?: (id: string) => void;
  readonly onRemoveHeld?: (id: string) => void;

  /** A prompt waiting on the original pane, drawn above the message box. */
  readonly promptCard?: React.ReactNode;
  /** The owner's handle on the message box, so an answered prompt can pass the focus on to it. */
  readonly composerHandleRef?: React.MutableRefObject<MobileChatComposerHandle | null>;
  /** Something the user must know about this lane. */
  readonly composerWarning?: string | null;
  /** Where a part bytes live when the page does not carry them. */
  readonly referenceContext?: ReferencePartRenderContext;

  readonly className?: string;
  readonly warnings?: readonly string[];
  readonly composerPlaceholder?: string;
  readonly disabled?: boolean;

  readonly headerTitle?: string;
  readonly headerSubtitle?: string;
  readonly showHeader?: boolean;
  readonly onBack?: () => void;
  readonly headerActions?: React.ReactNode;
}

const STARTER_PROMPTS = [
  "Summarize the current state",
  "What changed since my last message?",
  "Run the tests",
] as const;

/**
 * Turns a `/tree` walked away from, composed for the page: the reference renders this once at
 * the top of the transcript, not on every turn that carries one.
 */
function aggregateReferenceAbandoned(
  messages: readonly MobileChatMessageProps[],
): ReferenceAbandonedBranch | null {
  let count = 0;
  let branches = 0;
  let summary: string | null = null;
  for (const message of messages) {
    const branch = message.referenceAbandoned;
    if (!branch) continue;
    count += branch.count;
    branches += branch.branches;
    if (summary === null && branch.summary) summary = branch.summary;
  }
  if (count === 0 && branches === 0) return null;
  return summary === null ? { count, branches } : { count, branches, summary };
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
  showHeader = true,
  onOpenTerminal,
  pageDisclosure = null,
  pageAbandoned = null,
  hasOlderPage = false,
  loadedOlder = false,
  olderState = "idle",
  onLoadOlder,
  draft,
  onDraftChange,
  draftUnsaved = false,
  stagedFiles,
  onAttachFiles,
  onRemoveStagedFile,
  attaching = false,
  attachError = null,
  heldMessages,
  heldSendingIds,
  heldUnsaved = false,
  onEditHeld,
  onSendHeld,
  onRemoveHeld,
  promptCard,
  composerHandleRef,
  composerWarning = null,
  referenceContext,
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
  const composerRef = useRef<MobileChatComposerHandle | null>(null);
  const bindComposer = useCallback(
    (handle: MobileChatComposerHandle | null) => {
      composerRef.current = handle;
      if (composerHandleRef) composerHandleRef.current = handle;
    },
    [composerHandleRef],
  );

  const [internalDraft, setInternalDraft] = useState("");
  const isDraftControlled = draft !== undefined;
  const activeDraft = isDraftControlled ? draft : internalDraft;

  const handleDraftChange = useCallback(
    (next: string) => {
      if (!isDraftControlled) {
        setInternalDraft(next);
      }
      onDraftChange?.(next);
    },
    [isDraftControlled, onDraftChange],
  );

  const handleSelectStarter = useCallback(
    (promptText: string) => {
      if (!isDraftControlled) {
        setInternalDraft(promptText);
      }
      onDraftChange?.(promptText);
      composerRef.current?.focus();
    },
    [isDraftControlled, onDraftChange],
  );

  const [isAtBottom, setIsAtBottom] = useState(true);
  const [showScrollPill, setShowScrollPill] = useState(false);
  // Announced to screen readers when a running turn ends; aria-busy alone says nothing then.
  const [turnAnnouncement, setTurnAnnouncement] = useState("");
  const wasRunningRef = useRef(isRunning);
  // the viewport as it was before this commit, so an older page can be anchored
  const viewportRef = useRef<{
    top: number;
    height: number;
    count: number;
    atBottom: boolean;
    firstId: string | null;
  }>({ top: 0, height: 0, count: 0, atBottom: true, firstId: null });

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
    viewportRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      count: messages.length,
      atBottom,
      firstId: messages[0]?.id ?? null,
    };
    setIsAtBottom(atBottom);
    setShowScrollPill(distanceFromBottom > 100);
  }, [messages]);

  useEffect(() => {
    const wasRunning = wasRunningRef.current;
    wasRunningRef.current = isRunning;
    if (isRunning) {
      // cleared so the next finished turn is a text change and is announced again
      setTurnAnnouncement("");
      return;
    }
    if (!wasRunning) return;
    const last = messages[messages.length - 1];
    setTurnAnnouncement(last?.role === "assistant" ? "Assistant finished responding." : "");
  }, [isRunning, messages]);

  // An older page lands ABOVE what the reader is looking at: keep their place instead of
  // jumping. A page that lands below keeps the bottom-follow rule.
  useLayoutEffect(() => {
    const el = scrollContainerRef.current;
    if (!el) return;
    const previous = viewportRef.current;
    const firstId = messages[0]?.id ?? null;
    // a page was prepended only when the first turn changed and the old first turn is still here
    const prepended =
      previous.firstId !== null &&
      firstId !== previous.firstId &&
      messages.some((message) => message.id === previous.firstId);
    if (
      prepended &&
      messages.length > previous.count &&
      !previous.atBottom &&
      el.scrollHeight > previous.height
    ) {
      // the container is scroll-smooth; an animated anchor would show the reader's place jump
      el.style.scrollBehavior = "auto";
      el.scrollTop = previous.top + (el.scrollHeight - previous.height);
      el.style.scrollBehavior = "";
    }
    viewportRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      count: messages.length,
      atBottom: el.scrollHeight - el.scrollTop - el.clientHeight < 48,
      firstId,
    };
  }, [messages]);

  useEffect(() => {
    if (isAtBottom) {
      scrollToBottom(true);
    }
  }, [messages, isRunning, activityState, isAtBottom, scrollToBottom]);

  const hasMessages = messages.length > 0;
  // The page discloses abandoned branches once; a turn never repeats them.
  const abandoned = pageAbandoned ?? aggregateReferenceAbandoned(messages);

  return (
    <div
      data-testid="mobile-chat-workspace"
      className={cn(
        "relative flex flex-col w-full h-full min-h-0 bg-chat-screen text-chat-foreground overflow-hidden select-none",
        className
      )}
    >
      {showHeader && (
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
                  ferryx remote
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

          {onOpenTerminal && (
            <button
              type="button"
              data-testid="open-terminal-button"
              aria-label="Open the terminal for this session"
              title="Open the terminal for this session"
              onClick={onOpenTerminal}
              className="flex size-7 shrink-0 items-center justify-center rounded-md border border-chat-border bg-chat-surface/90 text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <TerminalIcon className="size-3.5" aria-hidden="true" />
            </button>
          )}

          {headerActions}
        </div>
      </header>
      )}

      <div data-testid="chat-turn-announcer" role="status" aria-live="polite" className="sr-only">
        {turnAnnouncement}
      </div>

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
        {pageDisclosure !== null ? (
          <ReferenceDisclosureBanner disclosure={pageDisclosure} />
        ) : null}
        {abandoned !== null ? (
          <ReferenceAbandonedBranchDisclosure abandoned={abandoned} />
        ) : null}
        {/* Older turns are prepended, so the control that fetches them sits above them. */}
        {hasOlderPage || loadedOlder ? (
          <ReferenceOlderPageControl
            hasOlder={hasOlderPage}
            loadedOlder={loadedOlder}
            state={olderState}
            onLoadOlder={onLoadOlder}
          />
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
            <div className="mt-3 flex flex-col gap-2 w-full">
              {STARTER_PROMPTS.map((prompt) => (
                <button
                  key={prompt}
                  type="button"
                  data-testid="chat-empty-starter"
                  aria-label={prompt}
                  onClick={() => handleSelectStarter(prompt)}
                  className="flex items-center text-left min-h-[44px] w-full px-3 py-2.5 rounded-lg border border-chat-border bg-chat-surface hover:bg-chat-surface-hover active:bg-chat-surface-raised text-xs font-medium text-chat-foreground-secondary hover:text-chat-foreground transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  {prompt}
                </button>
              ))}
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
                referenceParts={msg.referenceParts}
                referenceSource={msg.referenceSource}
                referenceContext={referenceContext}
              />
            ))}
          </>
        )}

        <div ref={messagesEndRef} className="h-2 w-full" />
      </div>

      <footer className="relative z-20 shrink-0">
        <div className="relative">
          {showScrollPill && (
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
            ref={bindComposer}
            value={activeDraft}
            onValueChange={handleDraftChange}
            draftUnsaved={draftUnsaved}
            attachments={stagedFiles}
            onAttachFiles={onAttachFiles}
            onRemoveAttachment={onRemoveStagedFile}
            attaching={attaching}
            attachError={attachError}
            heldMessages={heldMessages}
            heldSendingIds={heldSendingIds}
            heldUnsaved={heldUnsaved}
            onEditHeld={onEditHeld}
            onSendHeld={onSendHeld}
            onRemoveHeld={onRemoveHeld}
            promptCard={promptCard}
            warning={composerWarning}
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