import { Ellipsis, MessageSquare, Server, SquareTerminal } from "lucide-react";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";

/**
 * The phone top bar (plan task 3): a left context island, the two-line title group and a
 * right control island - the structure of paseo compact-header.tsx:43-140 drawn in Ferryx
 * chat tokens. Purely presentational: the owner feeds every value and callback, and keeps
 * both view-mode buttons mounted in either mode so the compact layout contract holds.
 */

// 32px controls extend to a 44px vertical hit area (paseo tab-switcher.tsx:25, hitSlop {top:8,bottom:8}).
const HIT_SLOP = "before:absolute before:inset-x-0 before:-inset-y-1.5 before:content-['']";

const ICON_BUTTON_32 =
  "relative flex size-8 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring " +
  HIT_SLOP;

const ATTENTION_BUTTON =
  "relative flex h-8 min-w-0 items-center gap-1 rounded-full bg-status-warning/15 px-2 text-status-warning text-[12px] font-medium disabled:opacity-60 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring " +
  HIT_SLOP;

export type MobileTopBarActivityState = "working" | "waiting" | "done" | null;

export interface MobileTopBarHostBadge {
  readonly label: string;
  readonly transport: string;
  readonly ariaLabel: string;
}

export interface MobileTopBarAttention {
  readonly label: string;
  readonly count: number;
  readonly ariaLabel: string;
  readonly disabled: boolean;
  readonly onPress: () => void;
}

export interface MobileTopBarProps {
  readonly title: string;
  readonly agentType?: string | null;
  readonly activityState?: MobileTopBarActivityState;
  readonly contextLabel: string;
  readonly hostBadge?: MobileTopBarHostBadge | null;
  readonly pickerOpen: boolean;
  readonly onTogglePicker: () => void;
  readonly attention?: MobileTopBarAttention | null;
  readonly viewMode: "chat" | "terminal";
  readonly onViewModeChange: (mode: "chat" | "terminal") => void;
  readonly sessionCount?: number | null;
  readonly switcherOpen?: boolean;
  readonly onOpenSwitcher?: () => void;
  readonly menuOpen?: boolean;
  readonly onOpenMenu?: () => void;
  readonly hideControls?: boolean;
}

export function MobileTopBar({
  title,
  agentType = null,
  activityState = null,
  contextLabel,
  hostBadge = null,
  pickerOpen,
  onTogglePicker,
  attention = null,
  viewMode,
  onViewModeChange,
  sessionCount = null,
  switcherOpen = false,
  onOpenSwitcher,
  menuOpen = false,
  onOpenMenu,
  hideControls = false,
}: MobileTopBarProps) {
  const logo = resolveAgentLogo(agentType);
  const monochrome = isMonochromeAgentLogo(agentType);

  return (
    <header
      data-testid="mobile-top-bar"
      className="shrink-0 bg-chat-header border-b border-chat-header-border pt-safe-header px-3 max-[374px]:px-2"
    >
      <div className="h-14 flex items-center gap-2 max-[374px]:gap-0">
        {/* Left island: the workspace-context switcher (paseo compact-header.tsx island chrome). */}
        <div className="rounded-full p-[7px] max-[374px]:p-1 bg-chat-surface-raised shadow-sm">
          <button
            type="button"
            aria-label="Change workspace context"
            aria-expanded={pickerOpen}
            onClick={onTogglePicker}
            className={cn(ICON_BUTTON_32, "text-chat-foreground-secondary hover:text-chat-foreground")}
          >
            {/* Three bars: 16px, 16px, 8px at 1.5px thick, 4px apart (paseo sidebar-menu-toggle.tsx:30-45). */}
            <span aria-hidden="true" className="flex flex-col items-start gap-1">
              <span className="h-[1.5px] w-4 rounded-full bg-current" />
              <span className="h-[1.5px] w-4 rounded-full bg-current" />
              <span className="h-[1.5px] w-2 rounded-full bg-current" />
            </span>
          </button>
        </div>

        <div className="flex-1 min-w-12">
          <div className="flex min-w-0 items-center gap-1.5">
            {logo ? (
              <img
                src={logo}
                alt=""
                data-agent-type={agentType}
                className={cn("size-4 shrink-0", monochrome && "agent-tab-logo--monochrome opacity-80")}
              />
            ) : null}
            <span className="min-w-12 truncate text-base font-medium text-chat-foreground">{title}</span>
            {activityState === "working" ? (
              <>
                <span
                  aria-hidden="true"
                  className="size-1.5 shrink-0 rounded-full bg-status-working motion-safe:animate-pulse"
                />
                <span className="sr-only">Working</span>
              </>
            ) : activityState === "waiting" ? (
              <>
                <span aria-hidden="true" className="size-1.5 shrink-0 rounded-full bg-status-warning" />
                <span className="sr-only">Needs approval</span>
              </>
            ) : null}
          </div>
          <div className="flex min-w-0 items-center gap-1.5 text-[13px] text-chat-foreground-secondary">
            <span aria-label="Current desktop context" className="min-w-8 shrink truncate">
              {contextLabel}
            </span>
            {hostBadge ? (
              <>
                <span aria-hidden="true" className="shrink-0 text-chat-foreground-tertiary max-[374px]:hidden">·</span>
                {/* Cap the badge via max-w rather than shrinking so the host name stays legible (R1/IS-4). */}
                <span
                  data-testid="remote-connection-badge"
                  data-connection={hostBadge.transport}
                  aria-label={hostBadge.ariaLabel}
                  className="flex min-w-12 max-w-[50%] shrink-0 items-center gap-1"
                >
                  <Server aria-hidden="true" className="size-3 shrink-0" />
                  <span className="min-w-0 truncate">{hostBadge.label}</span>
                </span>
              </>
            ) : null}
          </div>
        </div>

        {hideControls ? null : (
          <div className="flex min-w-0 rounded-full p-[7px] max-[374px]:p-1 bg-chat-surface-raised shadow-sm gap-0.5">
            {attention ? (
              <button
                type="button"
                data-testid="remote-attention-badge"
                aria-label={attention.ariaLabel}
                disabled={attention.disabled}
                onClick={attention.onPress}
                className={ATTENTION_BUTTON}
              >
                <span aria-hidden="true" className="size-1.5 shrink-0 rounded-full bg-status-warning" />
                <span className="min-w-0 max-w-24 truncate max-[390px]:sr-only">{attention.label}</span>
                {attention.count > 1 ? <span className="shrink-0 tabular-nums">{attention.count}</span> : null}
              </button>
            ) : null}

            {typeof sessionCount === "number" ? (
              <button
                type="button"
                aria-label={"Sessions (" + sessionCount + ")"}
                aria-haspopup="dialog"
                aria-expanded={switcherOpen}
                onClick={onOpenSwitcher}
                className={cn(ICON_BUTTON_32, "text-chat-foreground-secondary hover:text-chat-foreground")}
              >
                {/* Count badge sized like paseo tab-switcher.tsx:472-480 (16px, hairline border, tabular digits). */}
                <span className="flex h-4 min-w-4 items-center justify-center rounded-[4px] border border-chat-foreground-secondary px-0.5 text-[10px] font-medium tabular-nums">
                  {sessionCount}
                </span>
              </button>
            ) : null}

            <div role="group" aria-label="View" className="flex gap-0.5">
              <button
                type="button"
                data-testid="remote-view-mode-chat"
                aria-pressed={viewMode === "chat"}
                onClick={() => onViewModeChange("chat")}
                className={cn(
                  ICON_BUTTON_32,
                  viewMode === "chat"
                    ? "bg-chat-surface-hover text-chat-foreground"
                    : "text-chat-foreground-secondary"
                )}
              >
                <MessageSquare aria-hidden="true" className="size-4" />
                <span className="sr-only">Chat</span>
              </button>
              <button
                type="button"
                data-testid="remote-view-mode-terminal"
                aria-pressed={viewMode === "terminal"}
                onClick={() => onViewModeChange("terminal")}
                className={cn(
                  ICON_BUTTON_32,
                  viewMode === "terminal"
                    ? "bg-chat-surface-hover text-chat-foreground"
                    : "text-chat-foreground-secondary"
                )}
              >
                <SquareTerminal aria-hidden="true" className="size-4" />
                <span className="sr-only">Terminal</span>
              </button>
            </div>

            <button
              type="button"
              aria-label="More actions"
              aria-haspopup="dialog"
              aria-expanded={menuOpen}
              onClick={onOpenMenu}
              className={cn(ICON_BUTTON_32, "text-chat-foreground-secondary hover:text-chat-foreground")}
            >
              <Ellipsis aria-hidden="true" className="size-4" />
            </button>
          </div>
        )}
      </div>
      <span className="sr-only">Ferryx Remote</span>
    </header>
  );
}

