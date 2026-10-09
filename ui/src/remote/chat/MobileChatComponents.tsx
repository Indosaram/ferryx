import React, { useState } from "react";
import {
  Check,
  Terminal,
  AlertCircle,
  Loader2,
  FileText,
  Eye,
  SquarePen,
  Search,
  Globe,
  Sparkles,
  ListChecks,
  Wrench,
  Brain,
  MessageSquare,
} from "lucide-react";
import { cn } from "../../lib/cn";

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
    thinking: {
      text: label || "Thinking...",
      textColor: "text-status-warning",
      bgColor: "bg-status-warning/10",
      borderColor: "border-status-warning/20",
      dotColor: "bg-status-warning",
      icon: Loader2,
      spin: true,
    },
    running_tool: {
      text: label || "Running tool...",
      textColor: "text-status-working",
      bgColor: "bg-status-working/10",
      borderColor: "border-status-working/20",
      dotColor: "bg-status-working",
      icon: Terminal,
      spin: false,
    },
    waiting_for_input: {
      text: label || "Waiting for input...",
      textColor: "text-status-success",
      bgColor: "bg-status-success/10",
      borderColor: "border-status-success/20",
      dotColor: "bg-status-success",
      icon: AlertCircle,
      spin: false,
    },
  }[state];

  const Icon = config.icon;

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        "inline-flex items-center gap-2 px-2.5 py-1 rounded-full text-xs font-medium border backdrop-blur-sm shadow-xs transition-colors",
        config.bgColor,
        config.borderColor,
        config.textColor,
        className
      )}
    >
      <span className="relative flex h-2 w-2">
        <span
          className={cn(
            "animate-ping motion-reduce:animate-none absolute inline-flex h-full w-full rounded-full opacity-75",
            config.dotColor
          )}
        />
        <span
          className={cn(
            "relative inline-flex rounded-full h-2 w-2",
            config.dotColor
          )}
        />
      </span>
      <Icon
        className={cn("w-3.5 h-3.5", config.spin && "animate-spin motion-reduce:animate-none")}
      />
      <span className="tracking-wide whitespace-nowrap">{config.text}</span>
    </div>
  );
};

import type { AttachmentReceipt } from "../../lib/scopedContracts";

export interface ChatAttachment {
  id: string;
  attachmentId?: string;
  name: string;
  type: string;
  url?: string;
  size?: number | string;
  file?: File;
  receipt?: AttachmentReceipt;
  isStaging?: boolean;
  error?: string;
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
              className="relative overflow-hidden rounded-lg border border-chat-foreground/10 bg-chat-surface/60 shadow-xs max-w-[200px]"
            >
              <img
                src={att.url}
                alt={att.name}
                className="h-28 w-auto object-cover"
                loading="lazy"
              />
              <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-chat-screen/80 via-chat-screen/40 to-transparent p-1 px-1.5 text-[10px] text-chat-foreground-secondary truncate">
                {att.name}
              </div>
            </div>
          );
        }

        return (
          <div
            key={att.id}
            className="flex items-center gap-2 rounded-lg border border-chat-foreground/10 bg-chat-surface/60 px-2.5 py-1.5 text-xs text-chat-foreground-secondary shadow-xs backdrop-blur-sm"
          >
            <FileText className="w-3.5 h-3.5 text-chat-foreground-secondary shrink-0" />
            <span className="truncate max-w-[140px] font-medium">{att.name}</span>
            {att.size !== undefined && (
              <span className="text-[10px] text-chat-foreground-tertiary font-mono">
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
  if (lower === "task" || lower.startsWith("agent") || lower === "workpool") return Sparkles;
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
      <div className="w-6 h-6 shrink-0 flex items-center justify-center">
        <Icon
          aria-hidden="true"
          className={cn("w-3.5 h-3.5", isError ? "text-chat-danger" : "text-chat-foreground-secondary")}
        />
      </div>
      <div
        className={cn(
          "min-w-0 flex-1 truncate text-sm leading-none",
          isError ? "text-chat-danger" : "text-chat-foreground-secondary",
          isRunning && "work-shimmer-text"
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
            "w-full min-h-[32px] flex items-center gap-1.5 px-1 py-0.5 text-left rounded hover:bg-chat-row-hover transition-colors group focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            isError ? "text-chat-danger" : "text-chat-foreground-secondary"
          )}
        >
          {rowContent}
        </button>
      ) : (
        <div
          data-testid="work-row"
          className={cn(
            "w-full min-h-[32px] flex items-center gap-1.5 px-1 py-0.5 text-left rounded transition-colors group",
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
    <div className="ml-7 border-l border-chat-border pl-3 py-1 space-y-1.5">
      {command && (
        <pre
          data-testid="tool-call-input"
          tabIndex={0}
          className="font-mono text-[12px] text-chat-foreground-secondary whitespace-pre-wrap break-words max-h-60 overflow-y-auto select-text"
        >
          {command}
        </pre>
      )}
      {output && (
        <pre
          data-testid="work-row-output"
          tabIndex={0}
          className="font-mono text-[12px] text-chat-foreground-secondary whitespace-pre-wrap break-words max-h-60 overflow-y-auto select-text"
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
      {firstLine && <span className="italic opacity-80">{firstLine}</span>}
    </>
  ) : (
    <>
      <span className="font-medium">Thinking</span>
      {firstLine && (
        <>
          {" "}
          <span className="italic opacity-80">{firstLine}</span>
        </>
      )}
    </>
  );

  const expandedPanel = (
    <div className="ml-7 border-l border-chat-border pl-3 py-1">
      <p
        tabIndex={0}
        className="font-sans text-[12px] text-chat-foreground-secondary italic whitespace-pre-wrap break-words max-h-60 overflow-y-auto select-text"
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
  description: string;
  confirmLabel?: string;
  declineLabel?: string;
  allowCustomFeedback?: boolean;
  feedbackPlaceholder?: string;
  onAccept: (feedback?: string) => void;
  onDecline: (feedback?: string) => void;
  isSubmitting?: boolean;
  className?: string;
}

export const ApprovalActionCard: React.FC<ApprovalActionCardProps> = ({
  title = "Action Required",
  description,
  confirmLabel = "Approve",
  declineLabel = "Decline",
  allowCustomFeedback = true,
  feedbackPlaceholder = "Add instructions or reason...",
  onAccept,
  onDecline,
  isSubmitting = false,
  className,
}) => {
  const [feedback, setFeedback] = useState("");
  const [showFeedbackInput, setShowFeedbackInput] = useState(false);

  const handleConfirm = () => {
    onAccept(feedback.trim() ? feedback.trim() : undefined);
  };

  const handleDecline = () => {
    onDecline(feedback.trim() ? feedback.trim() : undefined);
  };

  return (
    <div
      className={cn(
        "rounded-xl border border-status-warning/30 bg-chat-surface/90 shadow-md p-3.5 backdrop-blur-md my-2.5 space-y-3",
        className
      )}
    >
      <div className="flex items-start gap-2.5">
        <div className="p-1 rounded-lg bg-status-warning/10 border border-status-warning/20 text-status-warning shrink-0 mt-0.5">
          <AlertCircle className="w-4 h-4" />
        </div>
        <div className="space-y-0.5 min-w-0 flex-1">
          <h4 className="text-xs font-semibold text-chat-foreground tracking-tight">
            {title}
          </h4>
          <p className="text-xs text-chat-foreground-secondary leading-relaxed break-words">
            {description}
          </p>
        </div>
      </div>

      {allowCustomFeedback && showFeedbackInput && (
        <div className="relative">
          <textarea
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            placeholder={feedbackPlaceholder}
            rows={2}
            disabled={isSubmitting}
            className="w-full rounded-lg border border-chat-foreground/10 bg-chat-screen/50 px-2.5 py-1.5 text-xs text-chat-foreground placeholder:text-chat-foreground-tertiary focus:outline-none focus:border-status-warning/50 focus:ring-1 focus:ring-status-warning/30 transition-all resize-none"
          />
        </div>
      )}

      <div className="flex items-center justify-between gap-2 pt-1 border-t border-chat-foreground/10">
        <div>
          {allowCustomFeedback && !showFeedbackInput && (
            <button
              type="button"
              onClick={() => setShowFeedbackInput(true)}
              className="text-[11px] text-chat-foreground-secondary hover:text-chat-foreground underline decoration-chat-foreground-tertiary underline-offset-2 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded"
            >
              Add note...
            </button>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <button
            type="button"
            onClick={handleDecline}
            disabled={isSubmitting}
            className="px-3 py-1.5 rounded-lg border border-chat-foreground/10 bg-chat-surface-raised hover:bg-chat-surface-hover active:bg-chat-surface-raised text-xs font-medium text-chat-foreground-secondary hover:text-chat-foreground transition-all disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {declineLabel}
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={isSubmitting}
            aria-busy={isSubmitting || undefined}
            className="px-3 py-1.5 rounded-lg border border-status-warning/40 bg-status-warning/20 hover:bg-status-warning/30 active:bg-status-warning/20 text-xs font-medium text-status-warning hover:text-chat-foreground shadow-xs transition-all flex items-center gap-1.5 disabled:opacity-50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {isSubmitting ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin motion-reduce:animate-none" />
            ) : (
              <Check className="w-3.5 h-3.5" />
            )}
            <span>{confirmLabel}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
