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
  /** The agent detected in the pane (registry id); null or absent for a plain shell. */
  readonly agentType?: string | null;

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

  /**
   * A prompt waiting on the original pane. It is drawn at the end of the message stream, not in
   * the composer footer, so the message box stays reachable with the keyboard open.
   */
  readonly promptCard?: React.ReactNode;
  /** The owner's handle on the message box, so an answered prompt can pass the focus on to it. */
  readonly composerHandleRef?: React.MutableRefObject<MobileChatComposerHandle | null>;
  /** Something the user must know about this lane. */
  readonly composerWarning?: string | null;
  /** One action for the composer warning (retry, reconnect), drawn as a 44px button beside it. */
  readonly composerWarningAction?: { readonly label: string; readonly onAction: () => void } | null;
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

/** One distance from the bottom decides both following the stream and offering the pill. */
const FOLLOW_THRESHOLD_PX = 80;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
  );
}

/**
 * Starters built from what the client already knows about the pane. A pane with no detected agent
 * (a plain shell) gets none: there is nobody to ask, and a canned prompt would be typed into a
 * shell. An agent pane gets prompts that name its worktree, so they read as about this work.
 */
export function chatStarterPrompts(
  agentType: string | null | undefined,
  worktreeLabel: string | null | undefined,
): readonly string[] {
  if (!agentType || agentType.trim().length === 0) return [];
  const where = worktreeLabel && worktreeLabel.trim().length > 0 ? ` on ${worktreeLabel.trim()}` : "";
  return [
    `Summarize where the work${where} stands`,
    `Review the uncommitted changes${where}`,
    `Run the tests${where} and fix what fails`,
  ];
}

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
  agentType = null,
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
  composerWarningAction = null,
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

  // Only a press on the pill animates; following a stream jumps, and reduced motion never animates.
  const scrollToBottom = useCallback((smooth = false) => {
    if (messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({
        behavior: smooth && !prefersReducedMotion() ? "smooth" : "auto",
        block: "end",
      });
    }
  }, []);

  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current;
    if (!el) return;

    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    const atBottom = distanceFromBottom < FOLLOW_THRESHOLD_PX;
    viewportRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      count: messages.length,
      atBottom,
      firstId: messages[0]?.id ?? null,
    };
    setIsAtBottom(atBottom);
    setShowScrollPill(!atBottom);
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
      // an animated anchor would show the reader's place jump
      el.style.scrollBehavior = "auto";
      el.scrollTop = previous.top + (el.scrollHeight - previous.height);
      el.style.scrollBehavior = "";
    }
    viewportRef.current = {
      top: el.scrollTop,
      height: el.scrollHeight,
      count: messages.length,
      atBottom: el.scrollHeight - el.scrollTop - el.clientHeight < FOLLOW_THRESHOLD_PX,
      firstId,
    };
  }, [messages]);

  // Set by a pill press: its own smooth scroll already heads to the bottom, so the follow effect
  // that the press re-runs (isAtBottom flips) must not cut that animation short with a jump.
  const pillScrollRef = useRef(false);
  useEffect(() => {
    if (pillScrollRef.current) {
      pillScrollRef.current = false;
      return;
    }
    if (isAtBottom) {
      scrollToBottom(false);
    }
  }, [messages, isRunning, activityState, isAtBottom, scrollToBottom, promptCard]);

  const hasMessages = messages.length > 0;
  const starterPrompts = chatStarterPrompts(agentType, worktreeLabel);
  // The page discloses abandoned branches once; a turn never repeats them.
  const abandoned = pageAbandoned ?? aggregateReferenceAbandoned(messages);

  return (
    <div
      data-testid="mobile-chat-workspace"
      className={cn(
        "relative flex flex-col w-full h-full min-h-0 bg-chat-screen text-chat-foreground overflow-hidden",
        className
      )}
    >
      {showHeader && (
      <header
        data-testid="mobile-chat-header"
        className="flex items-center justify-between px-1 py-1 bg-chat-screen border-b border-chat-border shrink-0 z-10 select-none"
      >
        <div className="flex items-center gap-2 min-w-0">
          {onBack && (
            <button
              type="button"
              data-testid="thread-header-back"
              aria-label="Back"
              onClick={onBack}
              className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <ChevronLeft className="size-4" aria-hidden="true" />
            </button>
          )}
          <div className="flex flex-col min-w-0">
            <span className="text-base font-medium text-chat-foreground">
              {headerTitle ?? "Chat"}
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
            ) : null}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {activityState !== "idle" && (
            <ActivityIndicator state={activityState} label={activityLabel} />
          )}

          {onOpenTerminal && (
            <button
              type="button"
              data-testid="open-terminal-button"
              aria-label="Open the terminal for this session"
              title="Open the terminal for this session"
              onClick={onOpenTerminal}
              className="flex size-11 shrink-0 items-center justify-center rounded-md text-chat-foreground-secondary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <TerminalIcon className="size-4" aria-hidden="true" />
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
        className="flex-1 min-h-0 overflow-y-auto px-3 pt-4 pb-14 space-y-3.5 overscroll-contain select-text sm:px-5"
      >
        {warnings && warnings.length > 0 ? (
          <div
            data-testid="chat-history-warning"
            role="status"
            className="mb-3 rounded-md border border-chat-border bg-chat-surface px-3 py-2 text-sm text-chat-foreground"
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
            className="flex flex-col items-start justify-center min-h-[40vh] max-w-md mx-auto w-full px-2 my-auto select-text"
          >
            <div className="w-full rounded-lg border border-chat-border bg-chat-surface px-3 py-2.5 space-y-1">
              {workspaceLabel ? (
                <div
                  data-testid="chat-empty-context"
                  className="font-mono text-xs text-chat-foreground-secondary truncate"
                >
                  {`${workspaceLabel}${worktreeLabel ? ` · ${worktreeLabel}` : ""}`}
                </div>
              ) : null}
              <p className="text-sm text-chat-foreground">
                Messages go to the agent in this session's terminal.
              </p>
            </div>
            {starterPrompts.length > 0 ? (
            <div className="mt-3 flex flex-col gap-2 w-full">
              {starterPrompts.map((prompt) => (
                <button
                  key={prompt}
                  type="button"
                  data-testid="chat-empty-starter"
                  aria-label={prompt}
                  onClick={() => handleSelectStarter(prompt)}
                  className="flex items-center text-left min-h-11 w-full px-3 py-2.5 rounded-lg border border-chat-border bg-chat-surface hover:bg-chat-surface-hover active:bg-chat-surface-raised text-[0.9375rem] text-chat-foreground transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  {prompt}
                </button>
              ))}
            </div>
            ) : null}
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
                durationStartedAt={msg.durationStartedAt}
                pending={msg.pending ?? msg.id.startsWith("pending-")}
                referenceParts={msg.referenceParts}
                referenceSource={msg.referenceSource}
                referenceContext={referenceContext}
              />
            ))}
          </>
        )}

        {promptCard !== undefined && promptCard !== null ? (
          <div data-testid="chat-stream-prompt" className="w-full">
            {promptCard}
          </div>
        ) : null}

        <div ref={messagesEndRef} className="h-2 w-full" />
      </div>

      <footer className="relative z-20 shrink-0">
        <div className="relative">
          {showScrollPill && (
            <div className="absolute -top-14 right-3 z-20">
              <button
                type="button"
                data-testid="scroll-to-latest-pill"
                onClick={() => {
                  pillScrollRef.current = !isAtBottom;
                  setIsAtBottom(true);
                  scrollToBottom(true);
                }}
                className="flex min-h-11 items-center gap-1.5 px-4 rounded-full bg-chat-surface-raised hover:bg-chat-surface-hover text-chat-foreground text-sm font-medium border border-chat-border transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <ArrowDown aria-hidden="true" className="size-4" />
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
            promptOpen={promptCard !== undefined && promptCard !== null}
            warning={composerWarning}
            warningAction={composerWarningAction}
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