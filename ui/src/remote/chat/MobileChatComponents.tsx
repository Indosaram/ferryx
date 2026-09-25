import React, { useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  Copy,
  Terminal,
  AlertCircle,
  CheckCircle2,
  Loader2,
  FileText,
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
      textColor: "text-amber-300/90",
      bgColor: "bg-amber-500/10",
      borderColor: "border-amber-500/20",
      dotColor: "bg-amber-400",
      icon: Loader2,
      spin: true,
    },
    running_tool: {
      text: label || "Running tool...",
      textColor: "text-sky-300/90",
      bgColor: "bg-sky-500/10",
      borderColor: "border-sky-500/20",
      dotColor: "bg-sky-400",
      icon: Terminal,
      spin: false,
    },
    waiting_for_input: {
      text: label || "Waiting for input...",
      textColor: "text-emerald-300/90",
      bgColor: "bg-emerald-500/10",
      borderColor: "border-emerald-500/20",
      dotColor: "bg-emerald-400",
      icon: AlertCircle,
      spin: false,
    },
  }[state];

  const Icon = config.icon;

  return (
    <div
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
            "animate-ping absolute inline-flex h-full w-full rounded-full opacity-75",
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
        className={cn("w-3.5 h-3.5", config.spin && "animate-spin")}
      />
      <span className="tracking-wide">{config.text}</span>
    </div>
  );
};

export interface ChatAttachment {
  id: string;
  name: string;
  type: "image" | "file";
  url?: string;
  size?: string;
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
        if (att.type === "image" && att.url) {
          return (
            <div
              key={att.id}
              className="group relative overflow-hidden rounded-lg border border-white/10 bg-zinc-900/60 shadow-xs max-w-[200px]"
            >
              <img
                src={att.url}
                alt={att.name}
                className="h-28 w-auto object-cover transition-transform duration-200 group-hover:scale-105"
                loading="lazy"
              />
              <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent p-1 px-1.5 text-[10px] text-zinc-300 truncate">
                {att.name}
              </div>
            </div>
          );
        }

        return (
          <div
            key={att.id}
            className="flex items-center gap-2 rounded-lg border border-white/10 bg-zinc-900/60 px-2.5 py-1.5 text-xs text-zinc-300 shadow-xs backdrop-blur-sm"
          >
            <FileText className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
            <span className="truncate max-w-[140px] font-medium">{att.name}</span>
            {att.size && (
              <span className="text-[10px] text-zinc-500 font-mono">
                {att.size}
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
  toolName: string;
  command?: string;
  output?: string;
  status: ToolStatus;
  durationMs?: number;
  initiallyExpanded?: boolean;
  className?: string;
}

export const ToolCallCard: React.FC<ToolCallCardProps> = ({
  toolName,
  command,
  output,
  status,
  durationMs,
  initiallyExpanded = false,
  className,
}) => {
  const [expanded, setExpanded] = useState(initiallyExpanded);
  const [copied, setCopied] = useState(false);

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    const textToCopy = output || command || "";
    if (!textToCopy) return;
    navigator.clipboard.writeText(textToCopy);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const statusConfig = {
    running: {
      badge: "Running",
      badgeClass: "bg-sky-500/10 text-sky-400 border-sky-500/20",
      icon: Loader2,
      spin: true,
      borderClass: "border-sky-500/20",
    },
    success: {
      badge: "Completed",
      badgeClass: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
      icon: CheckCircle2,
      spin: false,
      borderClass: "border-zinc-800",
    },
    error: {
      badge: "Failed",
      badgeClass: "bg-rose-500/10 text-rose-400 border-rose-500/20",
      icon: AlertCircle,
      spin: false,
      borderClass: "border-rose-500/20",
    },
  }[status];

  const StatusIcon = statusConfig.icon;

  return (
    <div
      className={cn(
        "rounded-xl border bg-zinc-950/70 shadow-xs backdrop-blur-md transition-all overflow-hidden my-2",
        statusConfig.borderClass,
        className
      )}
    >
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between gap-2.5 px-3 py-2 text-left hover:bg-white/[0.03] active:bg-white/[0.05] transition-colors"
      >
        <div className="flex items-center gap-2 min-w-0">
          <Terminal className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
          <span className="text-xs font-semibold text-zinc-200 tracking-tight font-mono truncate">
            {toolName}
          </span>
          {command && (
            <span className="text-[11px] text-zinc-400 truncate font-mono hidden sm:inline opacity-80">
              {command}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {durationMs !== undefined && (
            <span className="text-[10px] text-zinc-500 font-mono flex items-center gap-1">
              <Clock className="w-2.5 h-2.5" />
              {durationMs < 1000 ? `${durationMs}ms` : `${(durationMs / 1000).toFixed(1)}s`}
            </span>
          )}

          <span
            className={cn(
              "inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium border font-mono",
              statusConfig.badgeClass
            )}
          >
            <StatusIcon
              className={cn("w-2.5 h-2.5", statusConfig.spin && "animate-spin")}
            />
            {statusConfig.badge}
          </span>

          <span className="text-zinc-500">
            {expanded ? (
              <ChevronDown className="w-3.5 h-3.5" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5" />
            )}
          </span>
        </div>
      </button>

      {expanded && (
        <div className="border-t border-zinc-800/80 bg-black/40 text-xs">
          {command && (
            <div className="px-3 py-2 border-b border-zinc-850/60 bg-zinc-900/30 flex items-start justify-between gap-2">
              <div className="flex items-center gap-1.5 text-zinc-400 font-mono text-[11px]">
                <span className="text-emerald-400 font-bold">$</span>
                <span className="text-zinc-300 break-all select-all">{command}</span>
              </div>
              <button
                type="button"
                onClick={handleCopy}
                title="Copy command"
                className="p-1 rounded text-zinc-500 hover:text-zinc-300 hover:bg-white/5 transition-colors shrink-0"
              >
                {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
              </button>
            </div>
          )}

          {output ? (
            <div className="relative">
              <pre className="p-3 text-[11px] font-mono text-zinc-300 overflow-x-auto max-h-56 scrollbar-thin scrollbar-thumb-zinc-700 leading-relaxed whitespace-pre-wrap select-text">
                {output}
              </pre>
              <div className="absolute top-2 right-2">
                <button
                  type="button"
                  onClick={handleCopy}
                  title="Copy output"
                  className="px-1.5 py-1 rounded bg-zinc-800/80 hover:bg-zinc-700/80 text-zinc-400 hover:text-zinc-200 text-[10px] font-mono flex items-center gap-1 border border-white/5 backdrop-blur-xs transition-colors"
                >
                  {copied ? (
                    <>
                      <Check className="w-3 h-3 text-emerald-400" />
                      <span>Copied</span>
                    </>
                  ) : (
                    <>
                      <Copy className="w-3 h-3" />
                      <span>Copy</span>
                    </>
                  )}
                </button>
              </div>
            </div>
          ) : (
            <div className="px-3 py-2 text-[11px] text-zinc-500 italic font-mono">
              (No output produced)
            </div>
          )}
        </div>
      )}
    </div>
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
        "rounded-xl border border-amber-500/30 bg-zinc-900/90 shadow-md p-3.5 backdrop-blur-md my-2.5 space-y-3",
        className
      )}
    >
      <div className="flex items-start gap-2.5">
        <div className="p-1 rounded-lg bg-amber-500/10 border border-amber-500/20 text-amber-400 shrink-0 mt-0.5">
          <AlertCircle className="w-4 h-4" />
        </div>
        <div className="space-y-0.5 min-w-0 flex-1">
          <h4 className="text-xs font-semibold text-zinc-100 tracking-tight">
            {title}
          </h4>
          <p className="text-xs text-zinc-300 leading-relaxed break-words">
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
            className="w-full rounded-lg border border-white/10 bg-black/50 px-2.5 py-1.5 text-xs text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-amber-500/50 focus:ring-1 focus:ring-amber-500/30 transition-all resize-none"
          />
        </div>
      )}

      <div className="flex items-center justify-between gap-2 pt-1 border-t border-white/5">
        <div>
          {allowCustomFeedback && !showFeedbackInput && (
            <button
              type="button"
              onClick={() => setShowFeedbackInput(true)}
              className="text-[11px] text-zinc-400 hover:text-zinc-200 underline decoration-zinc-600 underline-offset-2 transition-colors"
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
            className="px-3 py-1.5 rounded-lg border border-white/10 bg-zinc-800 hover:bg-zinc-700/80 active:bg-zinc-800 text-xs font-medium text-zinc-300 hover:text-white transition-all disabled:opacity-50"
          >
            {declineLabel}
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={isSubmitting}
            className="px-3 py-1.5 rounded-lg border border-amber-500/40 bg-amber-500/20 hover:bg-amber-500/30 active:bg-amber-500/20 text-xs font-medium text-amber-200 hover:text-white shadow-xs transition-all flex items-center gap-1.5 disabled:opacity-50"
          >
            {isSubmitting ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
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
