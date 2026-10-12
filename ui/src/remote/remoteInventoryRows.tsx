import { Check, GitBranch, LoaderCircle, Terminal as TerminalIcon } from "lucide-react";
import React from "react";
import { resolveAgentLogo, isMonochromeAgentLogo } from "../lib/agentIcon";
import { cn } from "../lib/cn";
import { StatusDot } from "../components/ui/StatusDot";
import type { RemoteInventoryActivityState } from "./remoteInventoryView";

/** Row density for the shared picker rows: `touch` is the mobile session-picker drawer
 * (44px rows, 14px labels, 4px glyphs), `compact` is the desktop sidebar, which must stay
 * byte-identical to the pre-touch rendering. Row anatomy follows t3code
 * thread-list-v2-items.tsx (t3code, MIT). */
export type RemoteInventoryRowDensity = "touch" | "compact";

/** Status vocabulary from t3code STATUS_LABEL_BY_STATUS (apps/mobile/src/features/threads/
 * thread-list-v2-items.tsx, t3code MIT): working -> "Working", waiting -> "Approval",
 * done recedes with no label. */
export function touchStatusLabel(
  activity: RemoteInventoryActivityState | null,
): { text: string; className: string } | null {
  if (activity === "working") return { text: "Working", className: "text-status-working" };
  if (activity === "waiting") return { text: "Approval", className: "text-status-warning" };
  return null;
}

/** Right-aligned status caption for a touch row. It renders beside the row button, never
 * inside it, so row textContent and accessible names stay exactly what they were. */
export function TouchStatusCaption({
  activity,
  className,
}: {
  activity: RemoteInventoryActivityState | null;
  className?: string;
}) {
  const status = touchStatusLabel(activity);
  if (status === null) return null;
  return (
    <span
      data-testid="row-status-label"
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 shrink-0 text-[12px] tabular-nums",
        className,
        status.className,
      )}
    >
      {status.text}
    </span>
  );
}

/** Wraps a touch row so its caption can occupy the row's right lane without changing the
 * button's textContent. Rows without a caption keep their exact original DOM. */
export function withTouchCaption(
  row: React.ReactElement,
  activity: RemoteInventoryActivityState | null,
  disabled?: boolean,
): React.ReactElement {
  if (touchStatusLabel(activity) === null) return row;
  return (
    <div key={row.key} className="relative flex w-full items-center">
      {row}
      <TouchStatusCaption activity={activity} className={disabled ? "opacity-60" : undefined} />
    </div>
  );
}

type ActivityGlyphProps = {
  state: RemoteInventoryActivityState | null;
  testId: string;
  className?: string;
};

export function RemoteInventoryActivityGlyph({ state, testId, className }: ActivityGlyphProps) {
  if (state === null) return null;
  return (
    <span data-testid={testId} className={cn("flex shrink-0 items-center", className)}>
      <StatusDot state={state} />
    </span>
  );
}

export type RemoteInventoryProjectRowProps = {
  label: string;
  activity: RemoteInventoryActivityState | null;
  density?: RemoteInventoryRowDensity;
};

export function RemoteInventoryProjectRow({
  label,
  activity,
  density = "compact",
}: RemoteInventoryProjectRowProps) {
  const caption = density === "touch" ? touchStatusLabel(activity) : null;
  return (
    <div
      className={cn(
        density === "touch"
          ? "relative flex min-h-9 items-center gap-1.5 border-b border-worktree-sidebar-border px-2 pb-1.5 pt-2 text-[12px] font-medium text-chat-foreground-secondary"
          : "flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium text-worktree-sidebar-foreground/65",
        caption ? "pr-16" : null,
      )}
    >
      <h3 className="min-w-0 flex-1 truncate" title={label}>
        {label}
      </h3>
      <RemoteInventoryActivityGlyph
        state={activity}
        testId={`project-${activity ?? "idle"}-indicator`}
      />
      {caption ? <TouchStatusCaption activity={activity} /> : null}
    </div>
  );
}

export type RemoteInventoryWorktreeRowProps = {
  /** Accessible name: the full project/worktree identity the picker has always exposed. */
  label: string;
  /** Visible row text: the worktree name alone, since the project header owns the project name. */
  visibleLabel?: string;
  isRootWorktree: boolean;
  active: boolean;
  disabled: boolean;
  busy: boolean;
  activity: RemoteInventoryActivityState | null;
  hasSessions: boolean;
  onSelect: () => void;
  density?: RemoteInventoryRowDensity;
};

export const RemoteInventoryWorktreeRow: React.FC<RemoteInventoryWorktreeRowProps> = ({
  label,
  visibleLabel,
  isRootWorktree,
  active,
  disabled,
  busy,
  activity,
  hasSessions,
  onSelect,
  density = "compact",
}) => {
  const caption = density === "touch" ? touchStatusLabel(activity) : null;
  const row = (
    <button
      type="button"
      aria-current={active ? "true" : undefined}
      aria-label={label}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        density === "touch"
          ? "flex min-h-11 w-full items-center gap-1.5 rounded-md py-1 pl-3 pr-2 text-left transition-colors"
          : "flex h-7 w-full items-center gap-1.5 rounded-md py-1 pl-3 pr-2 text-left transition-colors",
        caption ? "pr-16" : null,
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60",
        active
          ? "bg-worktree-sidebar-accent text-foreground"
          : "text-worktree-sidebar-foreground hover:bg-white/[0.04]",
      )}
    >
      {busy ? (
        <LoaderCircle
          className={
            density === "touch"
              ? "size-4 shrink-0 animate-spin motion-reduce:animate-none"
              : "size-3 shrink-0 animate-spin motion-reduce:animate-none"
          }
          aria-hidden="true"
        />
      ) : (
        <RemoteInventoryActivityGlyph
          state={activity}
          testId={`worktree-${activity ?? "idle"}-indicator`}
          className={
            density === "touch" ? "size-4 justify-center" : "size-3 justify-center"
          }
        />
      )}
      {activity === null && !busy ? (
        hasSessions ? (
          <TerminalIcon
            className={
              density === "touch"
                ? "size-4 shrink-0 opacity-70"
 : "size-3 shrink-0 opacity-70"
            }
            aria-hidden="true"
          />
        ) : (
          <GitBranch
            className={
              density === "touch"
                ? "size-4 shrink-0 opacity-70"
                : "size-3 shrink-0 opacity-70"
            }
            aria-hidden="true"
          />
        )
      ) : null}
      <span
        data-testid="worktree-label-text"
        className={
          density === "touch"
            ? "min-w-0 flex-1 truncate text-[14px] font-semibold leading-tight"
            : "min-w-0 flex-1 truncate text-[12px] font-semibold leading-tight"
        }
      >
        {visibleLabel ?? label}
      </span>
      {isRootWorktree ? (
        <span
          data-testid="worktree-primary-badge"
          className="shrink-0 rounded-sm border border-worktree-sidebar-border px-1 text-[9px] font-medium uppercase tracking-wide text-worktree-sidebar-foreground/65"
        >
          primary
        </span>
      ) : null}
      {active ? <Check className="size-3 shrink-0" aria-label="Active" /> : null}
    </button>
  );
  if (density === "compact") return row;
  return withTouchCaption(row, activity, disabled);
};

export type RemoteInventorySessionRowProps = {
  label: string;
  sessionId: string | null | undefined;
  activity: RemoteInventoryActivityState | null;
  agentType?: string | null;
  active: boolean;
  disabled: boolean;
  busy: boolean;
  testId: string;
  onSelect: () => void;
  density?: RemoteInventoryRowDensity;
};

export const RemoteInventorySessionRow: React.FC<RemoteInventorySessionRowProps> = ({
  label,
  sessionId,
  activity,
  agentType,
  active,
  disabled,
  busy,
  testId,
  onSelect,
  density = "compact",
}) => {
  const logo = agentType ? resolveAgentLogo(agentType) : null;
  const monochrome = agentType ? isMonochromeAgentLogo(agentType) : false;
  const caption = density === "touch" ? touchStatusLabel(activity) : null;
  const row = (
    <button
      type="button"
      data-testid={testId}
      data-session-id={sessionId ?? undefined}
      aria-current={active ? "true" : undefined}
      aria-label={activity ? `${label} (${activity})` : label}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        density === "touch"
          ? "flex min-h-11 w-full items-center gap-1.5 rounded-md py-0.5 pl-6 pr-2 text-left transition-colors"
          : "flex min-h-[24px] w-full items-center gap-1.5 rounded-md py-0.5 pl-6 pr-2 text-left transition-colors",
        caption ? "pr-16" : null,
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60",
        active ? "bg-white/[0.06] text-foreground" : "text-worktree-sidebar-foreground/85 hover:bg-white/[0.04]",
      )}
    >
      {busy ? (
        <LoaderCircle
          className={
            density === "touch"
              ? "size-4 shrink-0 animate-spin motion-reduce:animate-none"
              : "size-3 shrink-0 animate-spin motion-reduce:animate-none"
          }
          aria-hidden="true"
        />
      ) : logo ? (
        <img
          src={logo}
          alt=""
          data-testid="tab-agent-icon"
          data-agent-type={agentType ?? undefined}
          className={cn(
            density === "touch" ? "size-4 shrink-0" : "size-3 shrink-0",
            monochrome && "agent-tab-logo--monochrome opacity-80",
          )}
        />
      ) : (
        <TerminalIcon
          data-testid="tab-terminal-icon"
          className={
            density === "touch"
              ? "size-4 shrink-0 opacity-70"
              : "size-3 shrink-0 opacity-70"
          }
          aria-hidden="true"
        />
      )}
      <span
        className={
          density === "touch"
            ? "min-w-0 flex-1 truncate text-[14px] leading-tight"
            : "min-w-0 flex-1 truncate text-[11px] leading-tight"
        }
      >
        {label}
      </span>
      <RemoteInventoryActivityGlyph
        state={activity}
        testId={`${testId}-${activity}-indicator`}
        className="justify-center"
      />
    </button>
  );
  if (density === "compact") return row;
  return withTouchCaption(row, activity, disabled);
};
