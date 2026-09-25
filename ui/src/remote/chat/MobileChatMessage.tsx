import React, { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, ChevronRight, Copy } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  ActivityIndicator,
  ActivityState,
  ApprovalActionCard,
  ApprovalActionCardProps,
  AttachmentList,
  ChatAttachment,
  ToolCallCard,
  ToolCallCardProps,
} from "./MobileChatComponents";

export interface MobileChatMessageProps {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp?: string | number;
  avatarUrl?: string;
  senderName?: string;
  attachments?: ChatAttachment[];
  toolCalls?: ToolCallCardProps[];
  approvalAction?: ApprovalActionCardProps;
  activityState?: ActivityState;
  durationLabel?: string;
  className?: string;
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
        "bg-zinc-800 text-sky-300 px-1.5 py-0.5 rounded font-mono text-xs",
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
  const match = /language-(\w+)/.exec(className || "");
  const language = match ? match[1] : "";
  const codeText = String(children).replace(/\n$/, "");

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(codeText);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="relative group my-2.5 rounded-xl border border-zinc-800 bg-zinc-950/90 overflow-hidden shadow-xs">
      <div className="flex items-center justify-between px-3 py-1.5 bg-zinc-900/60 border-b border-zinc-800/80 text-[11px] font-mono text-zinc-400">
        <span className="uppercase text-[10px] tracking-wider text-zinc-500 font-semibold">
          {language || "code"}
        </span>
        <button
          type="button"
          onClick={handleCopy}
          className="flex items-center gap-1 text-[10px] text-zinc-400 hover:text-zinc-200 transition-colors p-1 rounded hover:bg-white/5"
        >
          {copied ? (
            <>
              <Check className="w-3 h-3 text-emerald-400" />
              <span className="text-emerald-400 font-sans">Copied</span>
            </>
          ) : (
            <>
              <Copy className="w-3 h-3" />
              <span className="font-sans">Copy</span>
            </>
          )}
        </button>
      </div>
      <div className="overflow-x-auto p-3 text-[11px] font-mono text-zinc-200 leading-relaxed scrollbar-thin scrollbar-thumb-zinc-750">
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

export const MobileChatMessage: React.FC<MobileChatMessageProps> = ({
  role,
  content,
  timestamp,
  attachments = [],
  toolCalls = [],
  approvalAction,
  activityState,
  durationLabel,
  className,
}) => {
  const isUser = role === "user";
  const formattedTime = formatTimestamp(timestamp);
  const [copied, setCopied] = useState(false);
  const [workExpanded, setWorkExpanded] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const metaRow = (
    <div className="flex items-center gap-1.5">
      {formattedTime && (
        <span className="text-[10px] text-zinc-500 font-mono select-none">
          {formattedTime}
        </span>
      )}
      <button
        type="button"
        data-testid="message-copy-button"
        onClick={handleCopy}
        title="Copy message"
        className="p-0.5 rounded text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/60 transition-colors"
      >
        {copied ? (
          <Check className="size-3 text-emerald-400" />
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
          "flex flex-col items-end gap-1 my-2 max-w-[88%] ml-auto",
          className
        )}
      >
        <div
          data-testid="user-message-bubble"
          className="rounded-2xl rounded-tr-xs bg-primary text-primary-foreground px-3.5 py-2.5 shadow-xs leading-relaxed text-sm break-words select-text"
        >
          <p className="whitespace-pre-wrap">{content}</p>
          {attachments.length > 0 && (
            <AttachmentList attachments={attachments} className="mt-1" />
          )}
        </div>
        {metaRow}
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
      {durationLabel && (
        <button
          type="button"
          data-testid="worked-for-toggle"
          onClick={() => setWorkExpanded((prev) => !prev)}
          className="flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[11px] font-mono text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/60 transition-colors"
        >
          <ChevronRight
            className={cn("size-3 transition-transform", workExpanded && "rotate-90")}
          />
          <span>Worked for {durationLabel}</span>
        </button>
      )}

      {(!durationLabel || workExpanded) && (
        <>
          {content && (
            <div
              data-testid="assistant-message-body"
              className="w-full text-zinc-200 leading-relaxed text-sm break-words select-text"
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
                      className="text-sky-400 hover:text-sky-300 underline underline-offset-2"
                    >
                      {children}
                    </a>
                  ),
                  blockquote: ({ children }) => (
                    <blockquote className="border-l-2 border-zinc-750 pl-2.5 my-2 text-zinc-400 italic">
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
          )}

          {toolCalls && toolCalls.length > 0 && (
            <div className="w-full mt-1.5">
              {toolCalls.map((tc, index) => (
                <ToolCallCard key={`${tc.toolName}-${index}`} {...tc} />
              ))}
            </div>
          )}

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
        </>
      )}

      {metaRow}
    </div>
  );
};
