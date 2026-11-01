import React, { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { AlertCircle, Check, ChevronRight, Copy, Loader2 } from "lucide-react";
import { copyTextToClipboard } from "../../lib/clipboard";
import { formatWorkedDuration } from "../agentConversation";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
  ApprovalActionCard,
  ApprovalActionCardProps,
  AttachmentList,
  ChatAttachment,
  ChatWorkItem,
  CHAT_MARKDOWN_COMPONENTS,
  chatMarkdownUrlTransform,
  formatReferenceTimestamp,
  ReferenceAbandonedBranchDisclosure,
  ReferencePartList,
  ReferencePartRenderContext,
  ReferenceTurnSkills,
  ThinkingBlock,
  ToolCallCard,
} from "./MobileChatComponents";
import type {
  ReferenceAbandonedBranch,
  ReferencePart,
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
}

/** The "Worked for" time of a running turn, ticking on its own between polls. */
function useTickingDuration(startedAt: number | undefined, fallback: string | undefined): string | undefined {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (startedAt === undefined) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [startedAt]);
  return startedAt === undefined ? fallback : formatWorkedDuration(Math.max(0, now - startedAt));
}

interface WorkRowsContainerProps {
  children: React.ReactNode;
  count: number;
}

// The work list grows inline: a nested scroller inside the stream traps a thumb scroll on a phone.
const WorkRowsContainer: React.FC<WorkRowsContainerProps> = ({ children }) => (
  <div className="flex flex-col gap-px w-full my-1">{children}</div>
);

const MobileChatMessageImpl: React.FC<MobileChatMessageProps> = ({
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
  className,
}) => {
  const isUser = role === "user";
  const durationLabel = useTickingDuration(durationStartedAt, settledDurationLabel);
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
  // A user turn's bubble already holds its prose; only its other parts are drawn below it.
  const userExtraParts = visibleParts.filter((part) => part.kind !== "text");
  const isRuntime = referenceSource === "runtime";
  // The Worked for toggle folds a native turn's work (tools, thinking) as it folds `toolCalls`;
  // a turn with no such work has nothing to fold and draws no toggle.
  const hasWork = hasRichParts
    ? visibleParts.some((part) => part.kind === "tool" || part.kind === "thinking")
    : toolCalls.length > 0;
  const workFolded = Boolean(durationLabel) && hasWork && !workExpanded;

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

  if (isUser) {
    // A runtime turn, or a native user turn made only of parts nobody typed (a compaction, a
    // notice, a task result), is drawn on its own, never inside a user bubble. A pasted image
    // without prose is still the user's own message.
    const nobodyTyped =
      hasRichParts &&
      !hasProse &&
      attachments.length === 0 &&
      visibleParts.every(
        (part) => part.kind === "compact" || part.kind === "notice" || part.kind === "taskResult",
      );
    if (isRuntime || nobodyTyped) {
      return (
        <article aria-label={turnLabel} className={cn("mb-5 flex flex-col items-start gap-1 w-full", className)}>
          <ReferencePartList parts={visibleParts} context={referenceContext} timestamp={timestamp} />
          {hasProse && metaRow}
          <ReferenceTurnSkills parts={richParts} />
        </article>
      );
    }
    return (
      <article
        aria-label={turnLabel}
        className={cn(
          "mb-5 flex flex-col items-end gap-1 max-w-[88%] ml-auto",
          className
        )}
      >
        <div
          data-testid="user-message-bubble"
          className="min-w-0 gap-2 rounded-[20px] px-3.5 py-2.5 bg-chat-user-bubble text-chat-foreground leading-relaxed text-base break-words select-text"
        >
          <p className="whitespace-pre-wrap">{copyText}</p>
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
            />
          )}
        </div>
        {metaRow}
        <ReferenceTurnSkills parts={richParts} />
      </article>
    );
  }

  // System and runtime output is not the agent speaking: a quiet centred caption, not prose.
  if (role === "system" && !hasRichParts) {
    return (
      <article
        aria-label={turnLabel}
        data-testid="system-message"
        className={cn("my-3 flex w-full justify-center", className)}
      >
        <p className="max-w-[90%] whitespace-pre-wrap break-words text-center text-xs text-chat-foreground-secondary select-text">
          {content}
        </p>
      </article>
    );
  }

  return (
    <article
      aria-label={turnLabel}
      className={cn(
        "flex flex-col items-start gap-1 my-2 w-full",
        className
      )}
    >
      {referenceAbandoned && (
        <ReferenceAbandonedBranchDisclosure abandoned={referenceAbandoned} />
      )}

      <ReferenceTurnSkills parts={richParts} />

      {durationLabel && hasWork && (
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
              workExpanded && "rotate-90"
            )}
          />
          <span>Worked for {durationLabel}</span>
        </button>
      )}

      {!hasRichParts &&
        (!durationLabel || workExpanded) &&
        toolCalls &&
        toolCalls.length > 0 && (
          <WorkRowsContainer count={toolCalls.length}>
            {toolCalls.map((tc, index) =>
              tc.kind === "thinking" ? (
                <ThinkingBlock key={tc.workKey ?? `thinking-${index}`} text={tc.text} source={tc.source} />
              ) : (
                <ToolCallCard key={tc.workKey ?? `${tc.toolName}-${index}`} {...tc} />
              ),
            )}
          </WorkRowsContainer>
        )}

      {hasRichParts ? (
        <div data-testid="assistant-reference-body" className="w-full">
          <ReferencePartList
            parts={visibleParts}
            hideWork={workFolded}
            context={referenceContext}
            timestamp={timestamp}
          />
        </div>
      ) : hasProse ? (
        <div
          data-testid="assistant-message-body"
          className="w-full text-chat-foreground leading-relaxed text-base break-words select-text"
        >
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={CHAT_MARKDOWN_COMPONENTS}
            urlTransform={chatMarkdownUrlTransform}
          >
            {content}
          </ReactMarkdown>

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

      {hasProse && metaRow}
    </article>
  );
};

/** Memoized so a poll that leaves a turn's props untouched does not re-render its markdown. */
export const MobileChatMessage = React.memo(MobileChatMessageImpl);
