/**
 * `ExtensionStatusStrip` (lane H5): the side channel the provider reports beside the transcript.
 *
 * Four part kinds carry it, and each draws as its own shape because each means something
 * different to a reader: a `setStatus` pill is a one-line state word, a `setWidget` block is
 * several lines of provider output, a `subagentProgress` badge names one helper task, and a
 * `goalUpdate` strip is a plan with a state. None of them is a turn, so none of them is drawn in
 * the transcript - a heartbeat landing after the final answer must not reopen the turn.
 *
 * Placement follows the provider's own `widgetPlacement`: `belowEditor` blocks are drawn under
 * the composer by the pane, and this strip draws the rest above the transcript.
 */
import React from "react";
import { CircleDot, CircleCheck, CircleX, Loader2, Target } from "lucide-react";
import { cn } from "../../../lib/cn";
import type {
  StructuredExtensionStatus,
  StructuredExtensionWidget,
  StructuredGoalUpdate,
  StructuredSubagentProgress,
} from "../../../lib/structuredChatTypes";

/** The tone a status word carries: done and failed read differently from in-progress. */
type Tone = "pending" | "done" | "failed";

function toneOf(status: string): Tone {
  const word = status.trim().toLowerCase();
  if (word === "completed" || word === "complete" || word === "done" || word === "success" || word === "succeeded") {
    return "done";
  }
  if (word === "failed" || word === "error" || word === "cancelled" || word === "canceled" || word === "aborted") {
    return "failed";
  }
  return "pending";
}

const TONE_CLASS: Record<Tone, string> = {
  pending: "text-chat-foreground-secondary",
  done: "text-chat-foreground",
  failed: "text-chat-danger",
};

function ToneIcon({ tone }: { readonly tone: Tone }): React.ReactElement {
  if (tone === "done") return <CircleCheck aria-hidden="true" className="size-3 shrink-0" />;
  if (tone === "failed") return <CircleX aria-hidden="true" className="size-3 shrink-0" />;
  return <Loader2 aria-hidden="true" className="size-3 shrink-0 animate-spin" />;
}

export interface ExtensionStatusStripProps {
  readonly statuses: readonly StructuredExtensionStatus[];
  readonly widgets: readonly StructuredExtensionWidget[];
  readonly subagents: readonly StructuredSubagentProgress[];
  readonly goals: readonly StructuredGoalUpdate[];
  /**
   * Where a widget belongs. The provider stamps `widgetPlacement` on the part; a block with no
   * placement is drawn above the transcript, which is the reader's default view.
   */
  readonly placement?: "aboveEditor" | "belowEditor";
}

/** Does this widget belong at `placement`? An unplaced widget belongs above the transcript. */
function widgetAt(widget: StructuredExtensionWidget, placement: "aboveEditor" | "belowEditor"): boolean {
  const declared = widget.placement ?? "aboveEditor";
  return declared === placement;
}

/**
 * The strip. It draws nothing at all when the provider has reported nothing, so a session whose
 * extension never speaks keeps the transcript's own spacing.
 */
export const ExtensionStatusStrip: React.FC<ExtensionStatusStripProps> = ({
  statuses,
  widgets,
  subagents,
  goals,
  placement = "aboveEditor",
}) => {
  const shown = widgets.filter((widget) => widgetAt(widget, placement));
  if (statuses.length === 0 && shown.length === 0 && subagents.length === 0 && goals.length === 0) {
    return null;
  }
  return (
    <div data-testid="extension-status-strip" className="flex flex-col gap-1.5 px-3 pt-2">
      {statuses.length > 0 ? (
        <ul data-testid="extension-status-pills" className="flex flex-wrap items-center gap-1">
          {statuses.map((status) => (
            <li
              key={status.statusKey}
              data-testid={`extension-status-${status.statusKey}`}
              className="flex items-center gap-1 rounded-full border border-chat-border bg-chat-surface px-2 py-0.5 text-xs text-chat-foreground-secondary"
            >
              <CircleDot aria-hidden="true" className="size-2.5 shrink-0" />
              <span className="truncate max-w-[14rem]">{status.statusText}</span>
            </li>
          ))}
        </ul>
      ) : null}

      {subagents.length > 0 ? (
        <ul data-testid="extension-subagents" className="flex flex-wrap items-center gap-1">
          {subagents.map((task) => {
            const tone = toneOf(task.status);
            return (
              <li
                key={task.taskId}
                data-testid={`extension-subagent-${task.taskId}`}
                data-status={tone}
                title={task.summary ?? undefined}
                className={cn(
                  "flex items-center gap-1 rounded-full border border-chat-border bg-chat-surface px-2 py-0.5 text-xs",
                  TONE_CLASS[tone],
                )}
              >
                <ToneIcon tone={tone} />
                <span className="truncate max-w-[12rem]">{task.name}</span>
                <span className="text-chat-foreground-secondary">{task.status}</span>
              </li>
            );
          })}
        </ul>
      ) : null}

      {goals.length > 0 ? (
        <ul data-testid="extension-goals" className="flex flex-col gap-1">
          {goals.map((goal) => {
            const tone = toneOf(goal.status);
            return (
              <li
                key={goal.goalId}
                data-testid={`extension-goal-${goal.goalId}`}
                data-status={tone}
                className="flex items-center gap-2 rounded-md border border-chat-border bg-chat-surface px-2 py-1 text-xs"
              >
                <Target aria-hidden="true" className="size-3 shrink-0 text-chat-foreground-secondary" />
                <span className="min-w-0 flex-1 truncate text-chat-foreground">{goal.title}</span>
                {goal.progress !== undefined ? (
                  <span className="shrink-0 text-chat-foreground-secondary">{goal.progress}</span>
                ) : null}
                <span className={cn("shrink-0", TONE_CLASS[tone])}>{goal.status}</span>
              </li>
            );
          })}
        </ul>
      ) : null}

      {shown.length > 0 ? (
        <ul data-testid="extension-widgets" className="flex flex-col gap-1">
          {shown.map((widget) => (
            <li
              key={widget.widgetKey}
              data-testid={`extension-widget-${widget.widgetKey}`}
              className="rounded-md border border-chat-border bg-chat-surface px-2 py-1 font-mono text-xs text-chat-foreground-secondary"
            >
              {widget.lines.length === 0 ? (
                // A widget that was cleared: the key stays so a reader sees it went away, and the
                // block keeps its place instead of collapsing the strip.
                <span data-testid={`extension-widget-${widget.widgetKey}-empty`}>(empty)</span>
              ) : (
                widget.lines.map((line, index) => (
                  // Provider lines are pre-formatted output, not prose: drawn as they came.
                  <p key={index} className="whitespace-pre-wrap break-words">
                    {line}
                  </p>
                ))
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
};
