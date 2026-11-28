import React, { useEffect, useRef, useState } from "react";
import { AlertCircle, Check, ChevronRight, Copy, Loader2 } from "lucide-react";
import { copyTextToClipboard } from "../../lib/clipboard";
import { formatWorkedDuration } from "../agentConversation";
import { cn } from "../../lib/cn";
import { ImageActionsProvider, type ImageActions } from "./messages/imageLightbox";
import { MarkdownLinkActionsProvider, type MarkdownLinkActions } from "./messages/markdownLinks";
import { AssistantSelectionToolbar, type CiteSelection } from "./messages/AssistantSelectionToolbar";
import {
  ThreadActivityAnnouncer,
  type AnnounceLevel,
  announcementText,
  isAnnounceLevel,
  shouldAnnounce,
} from "./messages/announcer";
import { useThreadFindActive, useThreadFindMessage, countMatches } from "./messages/threadFind";
import {
  filterPartsForVisibility,
  zenErrorParts,
  zenHidesTurn,
  type ToolCallVisibility,
} from "./messages/toolCallVisibility";
import {
  ActivityIndicator,
  ActivityState,
  ApprovalActionCard,
  ApprovalActionCardProps,
  AttachmentList,
  ChatAttachment,
  ChatWorkItem,
  ChatProseMarkdown,
  formatReferenceTimestamp,
  ReferenceAbandonedBranchDisclosure,
  ReferencePartList,
  ReferencePartRenderContext,
  ReferenceTurnSkills,
  ThinkingBlock,
  ToolCallCard,
} from "./MobileChatComponents";
import { ReferenceTurnUsage } from "./referenceWorkGroup";
import type {
  ReferenceAbandonedBranch,
  ReferencePart,
  ReferenceTurnMeta,
  ReferenceTurnSource,
} from "./referenceTypes";

export interface MobileChatMessageProps {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp?: string | number;
  avatarUrl?: string;
  senderName?: string;
  attachments?: ChatAttachment[];
  toolCalls?: ChatWorkItem[];
  approvalAction?: ApprovalActionCardProps;
  activityState?: ActivityState;
  durationLabel?: string;
  /** Epoch ms the running turn started: its "Worked for" ticks once a second instead of jumping per poll. */
  durationStartedAt?: number;
  /** The user's message is on its way to the host and not yet in the transcript. */
  pending?: boolean;
  className?: string;
  /**
   * Herdr reference rich parts (plan task 5). When present they are the turn's body:
   * `content` and `toolCalls` are the legacy path and are not drawn together with them.
   */
  referenceParts?: readonly ReferencePart[];
  /** Turns a `/tree` walked away from, disclosed rather than dropped in silence. */
  referenceAbandoned?: ReferenceAbandonedBranch | null;
  /** `runtime` marks a turn the agent's runtime put in the user's seat; nobody typed it. */
  referenceSource?: ReferenceTurnSource | null;
  /** Where a part's bytes live when the page does not carry them. */
  referenceContext?: ReferencePartRenderContext;
  /**
   * Turn model and token usage. Absent hides the line. MobileChatWorkspace must pass
   * `referenceMeta={msg.referenceMeta}`; this file does not read the page itself.
   */
  referenceMeta?: ReferenceTurnMeta | null;
  /** Tool calls in chat (t3code Settings -> Chat). Absent shows everything. */
  toolCallVisibility?: ToolCallVisibility;
  /**
   * The `seq` this user turn was written at, for a lane that can rewind to it. Absent on every
   * surface that cannot (the reference chat, a legacy turn), which is what keeps the affordance
   * from appearing where nothing would happen.
   */
  editFromSeq?: number;
  /**
   * "Edit from here" (t3code composer.md:191-193): rewind the conversation to before this
   * message and put its text back in the composer. Only a user turn that names its `seq` gets it.
   */
  onEditFrom?: (seq: number) => void;
  /**
   * Zen: true for the reply that closes the turn; in-between replies stay hidden. The
   * transcript marks its own final reply (`phase: "finalAnswer"`), so this is only needed
   * for a turn whose parts carry no phase; the part's own mark always wins.
   */
  isTurnFinalReply?: boolean;
  /** Route a markdown file link to the pane's file surface. */
  onOpenFile?: (path: string, line?: number) => void;
  /** Open an external markdown link in the system browser. */
  onOpenExternal?: (url: string) => void;
  /** Save/copy image bytes; without these the lightbox falls back to a download anchor. */
  onSaveImage?: (src: string, name?: string) => void;
  onCopyImage?: (src: string) => void;
  /** Frozen contract: the pane forwards this to the composer's insertCitation handle. */
  onCiteSelection?: (citation: CiteSelection) => void;
  /** Screen reader announcements for the live turn; absent announces nothing. */
  announceLevel?: AnnounceLevel;
}

/**
 * The "Worked for" time of a running turn, isolated in a leaf so its 1s tick never re-renders the
 * parent message's markdown or tool rows, and paused while `document.hidden`.
 */
const WorkedDuration: React.FC<{
  startedAt: number | undefined;
  fallback: string | undefined;
}> = React.memo(function WorkedDuration({ startedAt, fallback }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (startedAt === undefined) return;
    let timer: ReturnType<typeof setInterval> | null = null;
    const isHidden = () => typeof document !== "undefined" && document.hidden;
    const start = () => {
      if (timer !== null) return;
      setNow(Date.now());
      timer = setInterval(() => setNow(Date.now()), 1000);
    };
    const stop = () => {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const onVisibilityChange = () => {
      if (isHidden()) stop();
      else start();
    };
    if (!isHidden()) start();
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      stop();
      if (typeof document !== "undefined") document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [startedAt]);
  return <>{startedAt === undefined ? fallback : formatWorkedDuration(Math.max(0, now - startedAt))}</>;
});

interface WorkRowsContainerProps {
  children: React.ReactNode;
  count: number;
}

// The work list grows inline: a nested scroller inside the stream traps a thumb scroll on a phone.
const WorkRowsContainer: React.FC<WorkRowsContainerProps> = ({ children }) => (
  <div className="flex flex-col gap-px w-full my-1">{children}</div>
);

const MobileChatMessageImpl: React.FC<MobileChatMessageProps> = ({
  id,
  role,
  content,
  timestamp,
  attachments = [],
  toolCalls = [],
  approvalAction,
  activityState,
  durationLabel: settledDurationLabel,
  durationStartedAt,
  pending = false,
  referenceParts,
  referenceAbandoned,
  referenceSource,
  referenceContext,
  referenceMeta,
  toolCallVisibility = "all",
  isTurnFinalReply: isTurnFinalReplyProp = false,
  editFromSeq,
  onEditFrom,
  onOpenFile,
  onOpenExternal,
  onSaveImage,
  onCopyImage,
  onCiteSelection,
  announceLevel,
  className,
}) => {
  const isUser = role === "user";
  const hasDuration = durationStartedAt !== undefined || Boolean(settledDurationLabel);
  // A native turn's start is an ISO instant; it prints as a local time like every other turn.
  const formattedTime = formatReferenceTimestamp(timestamp);
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [workExpanded, setWorkExpanded] = useState(false);
  // A rich turn's prose lives in its `text` parts; the legacy path keeps it in `content`.
  const richParts = referenceParts ?? [];
  const hasRichParts = richParts.length > 0;
  const richText = hasRichParts
    ? richParts
        .filter((part) => part.kind === "text")
        .map((part) => part.text)
        .join("\n\n")
    : "";
  // `content` of a native turn also carries its thinking and compaction text; only the text
  // parts are the prose a reader sees, copies, or (on a user turn) wrote.
  const copyText = hasRichParts ? richText : content;
  const hasProse = Boolean(copyText && copyText.trim().length > 0);
  // A skill chip is drawn by the turn's skill list, never inside the parts drawn in order: the
  // reference filters `skill` out of the parts it renders (`ChatView.tsx:322`).
  const visibleParts = richParts.filter((part) => part.kind !== "skill");
  // The turn that closes the conversation is the one whose prose the transcript phases as its
  // final answer; zen keeps it (activity-log.md: "the agent's last reply, which appears when the
  // turn ends"), and a turn finished before a steer keeps its answer too. Without the mark
  // (a legacy turn, a turn still streaming commentary) the caller's flag decides.
  const isTurnFinalReply =
    isTurnFinalReplyProp ||
    visibleParts.some((part) => part.kind === "text" && part.phase === "finalAnswer");
  // Tool calls in chat + find. While find is open, zen-hidden messages show again
  // (activity-log.md), so find wins over every hiding below.
  const findActive = useThreadFindActive();
  const zenMode = toolCallVisibility === "zen";
  const zenHideBody = zenMode && zenHidesTurn(role, isTurnFinalReply, findActive);
  const effectiveMode: ToolCallVisibility = findActive ? "all" : toolCallVisibility;
  // A zen-hidden in-between reply keeps only its errors; a visible turn filters its work.
  const filteredParts = findActive
    ? visibleParts
    : zenHideBody
      ? zenErrorParts(visibleParts)
      : filterPartsForVisibility(visibleParts, zenMode ? "hide-all" : effectiveMode);
  const legacyToolCalls = findActive
    ? toolCalls
    : toolCalls.filter((tc) => {
        // Zen's in-between reply keeps failed calls and nothing else, not even thinking.
        if (zenHideBody) return tc.kind === "tool" && tc.status === "error";
        if (tc.kind !== "tool") return true;
        if (tc.status === "error") return true;
        if (tc.status === "running" && effectiveMode === "hide-successful") return true;
        return effectiveMode === "all";
      });
  // A user turn's bubble already holds its prose; only its other parts are drawn below it.
  const userExtraParts = filteredParts.filter((part) => part.kind !== "text");
  const isRuntime = referenceSource === "runtime";
  // The Worked for toggle folds a native turn's work (tools, thinking) as it folds `toolCalls`;
  // a turn with no such work has nothing to fold and draws no toggle.
  const hasWork = hasRichParts
    ? filteredParts.some((part) => part.kind === "tool" || part.kind === "thinking")
    : legacyToolCalls.length > 0;
  const workFolded = hasDuration && hasWork && !workExpanded;

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  const handleCopy = async () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    const ok = await copyTextToClipboard(copyText);
    setCopied(ok);
    setCopyFailed(!ok);
    timerRef.current = setTimeout(() => {
      setCopied(false);
      setCopyFailed(false);
    }, 2000);
  };

  const metaRow = (
    <div className="flex items-center gap-1">
      {formattedTime && (
        <span className="font-mono text-xs text-chat-foreground-secondary select-none">
          {formattedTime}
        </span>
      )}
      <button
        type="button"
        data-testid="message-copy-button"
        onClick={handleCopy}
        title={copyFailed ? "Copy unavailable" : "Copy message"}
        aria-label={copyFailed ? "Copy failed" : "Copy message"}
        className="-my-2.5 flex size-11 items-center justify-center rounded text-chat-foreground-secondary hover:text-chat-foreground transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        {copied ? (
          <Check aria-hidden="true" className="size-3.5 text-status-success" />
        ) : copyFailed ? (
          <AlertCircle aria-hidden="true" className="size-3.5 text-chat-danger" />
        ) : (
          <Copy aria-hidden="true" className="size-3.5" />
        )}
      </button>
      <span role="status" className="sr-only">
        {copied ? "Message copied" : copyFailed ? "Couldn't copy the message" : ""}
      </span>
    </div>
  );

  const turnLabel = [
    isRuntime ? "Runtime message" : isUser ? "You" : role === "system" ? "System" : "Assistant",
    formattedTime,
  ]
    .filter(Boolean)
    .join(", ");

  // Find registers exactly the strings this turn draws marks in, so the bar's count and
  // its marks never disagree. A user turn draws one joined bubble; a rich turn draws its
  // text parts in order; the legacy path draws `content`.
  const proseParts = visibleParts.filter(
    (part): part is Extract<ReferencePart, { kind: "text" }> => part.kind === "text",
  );
  const searchableTexts = !hasProse
    ? []
    : isUser || !hasRichParts
      ? [copyText]
      : proseParts.map((part) => part.text);
  const findMessage = useThreadFindMessage(id, searchableTexts);
  // Ordinals run across the turn's prose parts in render order, so the Nth mark of the
  // turn is the Nth match the provider walks to when next/prev steps.
  const proseOrdinal = new Map<Extract<ReferencePart, { kind: "text" }>, number>();
  if (findActive) {
    let ordinal = 0;
    for (const part of proseParts) {
      proseOrdinal.set(part, ordinal);
      ordinal += countMatches(part.text, findMessage.query);
    }
  }
  const highlightProse = findActive
    ? (part: ReferencePart) =>
        part.kind === "text" ? findMessage.highlight(part.text, proseOrdinal.get(part) ?? 0) : null
    : null;
  const linkActions: MarkdownLinkActions = { onOpenFile, onOpenExternal };
  const imageActions: ImageActions | null =
    onSaveImage || onCopyImage ? { onSaveImage, onCopyImage } : null;
  const articleRef = useRef<HTMLElement | null>(null);

  // Screen reader announcements for the live turn (activity-log.md): the status is
  // always read, the completed in-between message per level, the running step only
  // at "everything". A final reply is never read here; the reader reads it after Done.
  // The end of a turn is the one status the doc names twice, so an idle turn says Done:
  // the text appears in the live region the moment the turn goes idle and stays put.
  let announcement: string | null = null;
  if (announceLevel && isAnnounceLevel(announceLevel)) {
    if (activityState === "idle") {
      announcement = announcementText(announceLevel, "status", "Done");
    } else if (activityState) {
      const statusText =
        activityState === "waiting_for_input"
          ? "Waiting for you"
          : activityState === "thinking"
            ? "Thinking"
            : "Working";
      const bits: string[] = [];
      const status = announcementText(announceLevel, "status", statusText);
      if (status) bits.push(status);
      if (activityState === "running_tool") {
        // The setting follows the agent's real activity, whatever "Tool calls in chat"
        // hides (activity-log.md), so this reads the unfiltered parts.
        const runningTool = visibleParts.find(
          (part) => part.kind === "tool" && part.status === "running",
        );
        if (runningTool && runningTool.kind === "tool" && shouldAnnounce(announceLevel, "step")) {
          const step = announcementText(announceLevel, "step", `Running ${runningTool.name}`);
          if (step) bits.push(step);
        }
      }
      if (role === "assistant" && hasProse && !isTurnFinalReply) {
        const message = announcementText(announceLevel, "message", copyText);
        if (message) bits.push(message);
      }
      announcement = bits.length > 0 ? bits.join("; ") : null;
    }
  }

  // A runtime turn, or a native user turn made only of parts nobody typed (a compaction, a
  // notice, a task result), is drawn on its own, never inside a user bubble. A pasted image
  // without prose is still the user's own message.
  const nobodyTyped =
    hasRichParts &&
    !hasProse &&
    attachments.length === 0 &&
    filteredParts.every(
      (part) => part.kind === "compact" || part.kind === "notice" || part.kind === "taskResult",
    );
  const announcementNode =
    announcement !== null && announceLevel ? (
      <ThreadActivityAnnouncer announcement={announcement} level={announceLevel} />
    ) : null;

  if (isRuntime || nobodyTyped) {
    return (
      <MarkdownLinkActionsProvider actions={linkActions}>
        <ImageActionsProvider actions={imageActions}>
          <article aria-label={turnLabel} className={cn("mb-5 flex flex-col items-start gap-1 w-full", className)}>
            <ReferenceTurnUsage meta={referenceMeta} />
            <ReferencePartList
              parts={filteredParts}
              context={referenceContext}
              timestamp={timestamp}
              highlightProse={highlightProse}
            />
            {hasProse && metaRow}
            <ReferenceTurnSkills parts={richParts} />
          </article>
          {announcementNode}
        </ImageActionsProvider>
      </MarkdownLinkActionsProvider>
    );
  }

  if (isUser) {
    return (
      <MarkdownLinkActionsProvider actions={linkActions}>
        <ImageActionsProvider actions={imageActions}>
          <article
            aria-label={turnLabel}
            data-message-role="user"
            className={cn(
              "mb-5 flex flex-col items-end gap-1 max-w-[88%] ml-auto",
              className
            )}
          >
            <ReferenceTurnUsage meta={referenceMeta} />
            <div
              data-testid="user-message-bubble"
              data-user-message-bubble=""
              className="min-w-0 gap-2 rounded-2xl border border-chat-border px-3.5 py-2.5 bg-chat-user-bubble text-chat-foreground leading-relaxed text-base break-words select-text"
            >
              <p className="whitespace-pre-wrap">{findMessage.highlight(copyText, 0)}</p>
              {pending && (
                <span
                  data-testid="user-message-sending"
                  className="mt-1 flex items-center gap-1.5 text-xs text-chat-foreground-secondary"
                >
                  <Loader2 aria-hidden="true" className="size-3 animate-spin motion-reduce:animate-none" />
                  Sending…
                </span>
              )}
              {attachments.length > 0 && (
                <AttachmentList attachments={attachments} className="mt-1" />
              )}
              {userExtraParts.length > 0 && (
                <ReferencePartList
                  parts={userExtraParts}
                  context={referenceContext}
                  timestamp={timestamp}
                  className="mt-1"
                  highlightProse={highlightProse}
                />
              )}
            </div>
            {metaRow}
            {isUser && editFromSeq !== undefined && onEditFrom !== undefined ? (
              <button
                type="button"
                data-testid="message-edit-from-here"
                data-seq={editFromSeq}
                aria-label="Edit from here"
                title="Edit from here"
                onClick={() => onEditFrom(editFromSeq)}
                className="flex min-h-9 items-center gap-1 rounded px-2 text-xs text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Edit from here
              </button>
            ) : null}
            <ReferenceTurnSkills parts={richParts} />
          </article>
          {announcementNode}
        </ImageActionsProvider>
      </MarkdownLinkActionsProvider>
    );
  }

  // System and runtime output is not the agent speaking: a quiet centred caption, not prose.
  if (role === "system" && !hasRichParts) {
    return (
      <MarkdownLinkActionsProvider actions={linkActions}>
        <ImageActionsProvider actions={imageActions}>
          <article
            aria-label={turnLabel}
            data-testid="system-message"
            className={cn("my-3 flex w-full justify-center", className)}
          >
            <p className="max-w-[90%] whitespace-pre-wrap break-words text-center text-xs text-chat-foreground-secondary select-text">
              {findMessage.highlight(content, 0)}
            </p>
          </article>
          {announcementNode}
        </ImageActionsProvider>
      </MarkdownLinkActionsProvider>
    );
  }

  return (
    <MarkdownLinkActionsProvider actions={linkActions}>
      <ImageActionsProvider actions={imageActions}>
        <article
          ref={articleRef}
          aria-label={turnLabel}
          className={cn("flex flex-col items-start gap-1 my-2 w-full", className)}
        >
          {referenceAbandoned && (
            <ReferenceAbandonedBranchDisclosure abandoned={referenceAbandoned} />
          )}

          <ReferenceTurnUsage meta={referenceMeta} />
          <ReferenceTurnSkills parts={richParts} />

          {hasDuration && hasWork && (
            <button
              type="button"
              data-testid="worked-for-toggle"
              aria-expanded={workExpanded}
              onClick={() => setWorkExpanded((prev) => !prev)}
              className="flex min-h-11 items-center gap-1.5 pr-3 rounded-md text-sm text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <ChevronRight
                aria-hidden="true"
                className={cn(
                  "size-3.5 transition-transform duration-200 ease-out motion-reduce:transition-none",
                  workExpanded && "rotate-90",
                )}
              />
              <span>Worked for <WorkedDuration startedAt={durationStartedAt} fallback={settledDurationLabel} /></span>
            </button>
          )}

          {!hasRichParts && (!hasDuration || workExpanded) && legacyToolCalls.length > 0 && (
            <WorkRowsContainer count={legacyToolCalls.length}>
              {legacyToolCalls.map((tc, index) =>
                tc.kind === "thinking" ? (
                  <ThinkingBlock
                    key={tc.workKey ?? `thinking-${index}`}
                    text={tc.text}
                    source={tc.source}
                  />
                ) : (
                  <ToolCallCard key={tc.workKey ?? `${tc.toolName}-${index}`} {...tc} />
                ),
              )}
            </WorkRowsContainer>
          )}

          {zenHideBody ? (
            // Zen's in-between reply keeps only its errors: no prose body, no prose copy row.
            filteredParts.length > 0 ? (
              <ReferencePartList
                parts={filteredParts}
                context={referenceContext}
                timestamp={timestamp}
                highlightProse={highlightProse}
              />
            ) : null
          ) : hasRichParts ? (
            <div data-testid="assistant-reference-body" className="w-full">
              <ReferencePartList
                parts={filteredParts}
                hideWork={workFolded}
                mathInProse={role === "assistant"}
                context={referenceContext}
                timestamp={timestamp}
                highlightProse={highlightProse}
              />
            </div>
          ) : hasProse ? (
            <div
              data-testid="assistant-message-body"
              className="w-full text-chat-foreground leading-relaxed text-base break-words select-text"
            >
              {findActive ? (
                // While find is open the prose is drawn as highlighted text: the match
                // marks, and the scroll target for next/prev, have to be in the DOM.
                <p className="whitespace-pre-wrap">{findMessage.highlight(content, 0)}</p>
              ) : (
                <ChatProseMarkdown text={content} assistant={role === "assistant"} />
              )}

              {attachments.length > 0 && (
                <AttachmentList attachments={attachments} className="mt-2" />
              )}
            </div>
          ) : null}

          {approvalAction && (
            <div className="w-full mt-1.5">
              <ApprovalActionCard {...approvalAction} />
            </div>
          )}

          {activityState && activityState !== "idle" && (
            <div className="mt-2">
              <ActivityIndicator state={activityState} />
            </div>
          )}

          {hasProse && !zenHideBody && metaRow}
        </article>
        {role === "assistant" && hasProse && !zenHideBody && (
          <AssistantSelectionToolbar
            messageId={id}
            anchorRef={articleRef}
            onCiteSelection={onCiteSelection}
          />
        )}
        {announcementNode}
      </ImageActionsProvider>
    </MarkdownLinkActionsProvider>
  );
};

/** Memoized so a poll that leaves a turn's props untouched does not re-render its markdown. */
export const MobileChatMessage = React.memo(MobileChatMessageImpl);
