import React, { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { AlertCircle, Check, ChevronRight, Copy } from "lucide-react";
import { copyTextToClipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
  ApprovalActionCard,
  ApprovalActionCardProps,
  AttachmentList,
  ChatAttachment,
  ChatWorkItem,
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

interface CodeBlockProps {
  inline?: boolean;
  className?: string;
  children?: React.ReactNode;
}

const InlineCode: React.FC<CodeBlockProps> = ({ className, children, ...props }) => {
  return (
    <code
      className={cn(
        "bg-chat-surface-raised text-chat-code px-1.5 py-0.5 rounded font-mono text-xs",
        className
      )}
      {...props}
    >
      {children}
    </code>
  );
};

const CodeBlock: React.FC<CodeBlockProps> = ({ className, children, ...props }) => {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const match = /language-(\w+)/.exec(className || "");
  const language = match ? match[1] : "";
  const codeText = String(children).replace(/\n$/, "");

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    const ok = await copyTextToClipboard(codeText);
    setCopied(ok);
    if (!ok) setCopyFailed(true);
    timerRef.current = setTimeout(() => {
      setCopied(false);
      setCopyFailed(false);
    }, 2000);
  };

  return (
    <div className="relative group my-2.5 rounded-xl border border-chat-border bg-chat-screen/90 overflow-hidden shadow-xs">
      <div className="flex items-center justify-between px-3 py-1.5 bg-chat-surface/60 border-b border-chat-border text-[11px] font-mono text-chat-foreground-secondary">
        <span className="uppercase text-[10px] tracking-wider text-chat-foreground-tertiary font-semibold">
          {language || "code"}
        </span>
        <button
          type="button"
          onClick={handleCopy}
          aria-label={copied ? "Copied" : copyFailed ? "Copy failed" : "Copy code"}
          className="flex items-center gap-1 text-[10px] text-chat-foreground-secondary hover:text-chat-foreground transition-colors p-1 rounded hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {copied ? (
            <>
              <Check className="w-3 h-3 text-status-success" />
              <span className="text-status-success font-sans">Copied</span>
            </>
          ) : copyFailed ? (
            <>
              <AlertCircle aria-hidden="true" className="w-3 h-3 text-chat-danger" />
              <span className="text-chat-danger font-sans">Failed</span>
            </>
          ) : (
            <>
              <Copy className="w-3 h-3" />
              <span className="font-sans">Copy</span>
            </>
          )}
        </button>
      </div>
      <div className="overflow-x-auto p-3 text-[11px] font-mono text-chat-foreground leading-relaxed scrollbar-sleek">
        <pre className="!bg-transparent !p-0 !m-0">
          <code className={className} {...props}>
            {children}
          </code>
        </pre>
      </div>
    </div>
  );
};

function formatTimestamp(timestamp?: string | number): string | null {
  if (!timestamp) return null;
  if (typeof timestamp === "string" && !/^\d+$/.test(timestamp)) {
    return timestamp;
  }
  const date = new Date(Number(timestamp));
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

interface WorkRowsContainerProps {
  children: React.ReactNode;
  count: number;
}

const WorkRowsContainer: React.FC<WorkRowsContainerProps> = ({ children, count }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const checkOverflow = () => {
      setOverflows(el.scrollHeight > el.clientHeight);
    };
    checkOverflow();
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(checkOverflow);
      observer.observe(el);
      return () => observer.disconnect();
    }
  }, [children, count]);

  const shouldFade = count > 8 || overflows;

  return (
    <div
      ref={containerRef}
      style={
        shouldFade
          ? {
              maskImage:
                "linear-gradient(to bottom, transparent 0, black 12px, black calc(100% - 12px), transparent 100%)",
              WebkitMaskImage:
                "linear-gradient(to bottom, transparent 0, black 12px, black calc(100% - 12px), transparent 100%)",
            }
          : undefined
      }
      className="flex flex-col gap-px max-h-64 overflow-y-auto w-full my-1 scrollbar-sleek"
    >
      {children}
    </div>
  );
};

export const MobileChatMessage: React.FC<MobileChatMessageProps> = ({
  role,
  content,
  timestamp,
  attachments = [],
  toolCalls = [],
  approvalAction,
  activityState,
  durationLabel,
  referenceParts,
  referenceAbandoned,
  referenceSource,
  referenceContext,
  className,
}) => {
  const isUser = role === "user";
  const formattedTime = formatTimestamp(timestamp);
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
  const copyText = hasRichParts && content.trim().length === 0 ? richText : content;
  const hasProse = Boolean(copyText && copyText.trim().length > 0);
  // A skill chip is drawn by the turn's skill list, never inside the parts drawn in order: the
  // reference filters `skill` out of the parts it renders (`ChatView.tsx:322`).
  const visibleParts = richParts.filter((part) => part.kind !== "skill");
  // A user turn's bubble already holds its prose; only its other parts are drawn below it.
  const userExtraParts = visibleParts.filter((part) => part.kind !== "text");
  const isRuntime = referenceSource === "runtime";

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
    if (!ok) setCopyFailed(true);
    timerRef.current = setTimeout(() => {
      setCopied(false);
      setCopyFailed(false);
    }, 2000);
  };

  const metaRow = (
    <div className="flex items-center gap-1.5">
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
        className="p-0.5 rounded text-chat-foreground-tertiary hover:text-chat-foreground-secondary hover:bg-chat-surface-raised/60 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        {copied ? (
          <Check aria-hidden="true" className="size-3 text-status-success" />
        ) : copyFailed ? (
          <AlertCircle aria-hidden="true" className="size-3 text-chat-danger" />
        ) : (
          <Copy className="size-3" />
        )}
      </button>
    </div>
  );

  if (isUser) {
    return (
      <div
        className={cn(
          "mb-5 flex flex-col items-end gap-1 max-w-[88%] ml-auto",
          className
        )}
      >
        <div
          data-testid="user-message-bubble"
          className="min-w-0 gap-2 rounded-[20px] px-3.5 py-2.5 bg-chat-user-bubble text-chat-foreground leading-relaxed text-base break-words select-text"
        >
          <p className="whitespace-pre-wrap">{hasRichParts && content.trim().length === 0 ? richText : content}</p>
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
      </div>
    );
  }

  return (
    <div
      className={cn(
        "flex flex-col items-start gap-1 my-2 max-w-[94%] mr-auto",
        className
      )}
    >
      {referenceAbandoned && (
        <ReferenceAbandonedBranchDisclosure abandoned={referenceAbandoned} />
      )}

      <ReferenceTurnSkills parts={richParts} />

      {isRuntime && <span className="sr-only">Runtime message</span>}

      {durationLabel && (
        <button
          type="button"
          data-testid="worked-for-toggle"
          aria-expanded={workExpanded}
          onClick={() => setWorkExpanded((prev) => !prev)}
          className="flex items-center gap-1 px-1.5 py-0.5 rounded-md text-xs font-mono text-chat-foreground-secondary hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <ChevronRight
            className={cn(
              "size-3 transition-transform duration-200 ease-out",
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
            components={{
              code: ({ node, inline, className, children, ...props }: any) => {
                const contentStr = String(children);
                const hasLang = Boolean(className && (className.startsWith("language-") || /language-(\w+)/.test(className)));
                const hasNewline = contentStr.includes("\n");
                const isFenced = hasLang || inline === false || (inline === undefined && hasNewline);
                const isPureInline = !hasLang && !hasNewline && (inline === true || inline === undefined);

                if (isPureInline && !isFenced) {
                  return (
                    <InlineCode className={className} {...props}>
                      {children}
                    </InlineCode>
                  );
                }
                return (
                  <CodeBlock className={className} {...props}>
                    {children}
                  </CodeBlock>
                );
              },
              pre: ({ children }: any) => <>{children}</>,
              p: ({ children }: any) => {
                const childArray = React.Children.toArray(children);
                const hasBlockChild = childArray.some(
                  (child) =>
                    React.isValidElement(child) &&
                    (child.type === CodeBlock ||
                      (typeof child.type === "string" && ["div", "pre", "blockquote", "ul", "ol", "table"].includes(child.type)))
                );
                if (hasBlockChild) {
                  return <div className="mb-2 last:mb-0">{children}</div>;
                }
                return <p className="mb-2 last:mb-0">{children}</p>;
              },
              ul: ({ children }) => <ul className="list-disc pl-4 mb-2 space-y-1">{children}</ul>,
              ol: ({ children }) => <ol className="list-decimal pl-4 mb-2 space-y-1">{children}</ol>,
              a: ({ href, children }) => (
                <a
                  href={href}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-chat-link decoration-chat-link/60 hover:decoration-chat-link underline underline-offset-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded"
                >
                  {children}
                </a>
              ),
              blockquote: ({ children }) => (
                <blockquote className="border-l-2 border-chat-border pl-2.5 my-2 text-chat-foreground-secondary italic">
                  {children}
                </blockquote>
              ),
            }}
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
    </div>
  );
};
