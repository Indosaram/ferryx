import { useId } from "react";
import { ChevronLeft, Ellipsis, Inbox, MessageSquare, Server, SquareTerminal } from "lucide-react";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";

/**
 * The phone top bar (plan task 3): a left context island, the two-line title group and a
 * right control island - the structure of paseo compact-header.tsx:43-140 drawn in Ferryx
 * chat tokens. Purely presentational: the owner feeds every value and callback, and keeps
 * both view-mode buttons mounted in either mode so the compact layout contract holds.
 */

// Every control is a 44x44 target; the visible chip inside it stays 32px.
const ICON_BUTTON =
  "group/icon relative flex size-11 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

const ATTENTION_BUTTON =
  "relative flex h-11 min-w-0 items-center gap-1.5 rounded-full px-2.5 text-status-warning text-xs font-medium disabled:opacity-60 transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

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

/** The Inbox entry: `count` is the sessions waiting on the user, the badge it shows. */
export interface MobileTopBarInbox {
  readonly count: number;
  readonly open: boolean;
  readonly onOpen: () => void;
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
  readonly inbox?: MobileTopBarInbox | null;
  readonly switcherOpen?: boolean;
  readonly onOpenSwitcher?: () => void;
  readonly menuOpen?: boolean;
  readonly onOpenMenu?: () => void;
  readonly hideControls?: boolean;
  /** Phone home: returns from the Session screen to the home destination it was opened from. */
  readonly onBack?: () => void;
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
  inbox = null,
  switcherOpen = false,
  onOpenSwitcher,
  menuOpen = false,
  onOpenMenu,
  hideControls = false,
  onBack,
}: MobileTopBarProps) {
  const contextLabelId = useId();
  const logo = resolveAgentLogo(agentType);
  const monochrome = isMonochromeAgentLogo(agentType);

  return (
    <header
      data-testid="mobile-top-bar"
      className="shrink-0 bg-chat-header border-b border-chat-header-border pt-safe-header px-1"
    >
      <div className="h-14 flex items-center gap-1">
        <div className="flex shrink-0">
          {onBack ? (
            <button
              type="button"
              data-testid="mobile-session-back"
              aria-label="Back to home"
              title="Back to home"
              onClick={onBack}
              className={cn(ICON_BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}
            >
              <ChevronLeft aria-hidden="true" className="size-4" />
            </button>
          ) : null}
          <button
            type="button"
            aria-label="Change workspace context"
            title="Change workspace context"
            aria-expanded={pickerOpen}
            onClick={onTogglePicker}
            className={cn(ICON_BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}
          >
            {/* Three bars: 16px, 16px, 8px at 1.5px thick, 4px apart (paseo sidebar-menu-toggle.tsx:30-45). */}
            <span aria-hidden="true" className="flex flex-col items-start gap-1">
              <span className="h-0.5 w-4 rounded-full bg-current" />
              <span className="h-0.5 w-4 rounded-full bg-current" />
              <span className="h-0.5 w-2 rounded-full bg-current" />
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
          <div className="flex min-w-0 items-center gap-1.5 text-[0.8125rem] text-chat-foreground-secondary">
            <span id={contextLabelId} className="sr-only">Current desktop context</span>
            <span
              aria-labelledby={contextLabelId}
              data-testid="remote-context-label"
              className="min-w-8 shrink truncate"
            >
              {contextLabel}
            </span>
            {hostBadge ? (
              <>
                <span aria-hidden="true" className="shrink-0 text-chat-foreground-tertiary max-[374px]:hidden">·</span>
                {/* Cap the badge via max-w rather than shrinking so the host name stays legible (R1/IS-4). */}
                <span
                  data-testid="remote-connection-badge"
                  data-connection={hostBadge.transport}
                  className="flex min-w-12 max-w-[50%] shrink-0 items-center gap-1"
                >
                  <span className="sr-only">{hostBadge.ariaLabel}</span>
                  <Server aria-hidden="true" className="size-3 shrink-0" />
                  <span data-testid="remote-connection-badge-label" className="min-w-0 truncate">
                    {hostBadge.label}
                  </span>
                </span>
              </>
            ) : null}
          </div>
        </div>

        {hideControls ? null : (
          <div className="flex min-w-0 shrink-0">
            {attention ? (
              <button
                type="button"
                data-testid="remote-attention-badge"
                aria-label={attention.ariaLabel}
                title={attention.ariaLabel}
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
                title={"Sessions (" + sessionCount + ")"}
                aria-haspopup="dialog"
                aria-expanded={switcherOpen}
                onClick={onOpenSwitcher}
                className={cn(ICON_BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}
              >
                {/* Count badge sized like paseo tab-switcher.tsx:472-480 (16px, hairline border, tabular digits). */}
                <span className="flex h-5 min-w-5 items-center justify-center rounded border border-chat-foreground-secondary px-1 text-xs font-medium tabular-nums">
                  {sessionCount}
                </span>
              </button>
            ) : null}

            {inbox ? (
              <button
                type="button"
                data-testid="mobile-inbox-button"
                aria-label={inbox.count > 0 ? `Inbox (${inbox.count} waiting)` : "Inbox"}
                title="Inbox"
                aria-haspopup="dialog"
                aria-expanded={inbox.open}
                onClick={inbox.onOpen}
                className={cn(ICON_BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}
              >
                <Inbox aria-hidden="true" className="size-4" />
                {inbox.count > 0 ? (
                  <span
                    data-testid="mobile-inbox-badge"
                    className="absolute right-0.5 top-0.5 flex h-5 min-w-5 items-center justify-center rounded-full bg-status-warning px-1 text-xs font-semibold tabular-nums text-chat-screen"
                  >
                    {inbox.count}
                  </span>
                ) : null}
              </button>
            ) : null}

            <div role="group" aria-label="View" className="flex">
              <button
                type="button"
                data-testid="remote-view-mode-chat"
                title="Chat"
                aria-pressed={viewMode === "chat"}
                onClick={() => onViewModeChange("chat")}
                className={cn(
                  ICON_BUTTON,
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
                title="Terminal"
                aria-pressed={viewMode === "terminal"}
                onClick={() => onViewModeChange("terminal")}
                className={cn(
                  ICON_BUTTON,
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
              title="More actions"
              aria-haspopup="dialog"
              aria-expanded={menuOpen}
              onClick={onOpenMenu}
              className={cn(ICON_BUTTON, "text-chat-foreground-secondary hover:text-chat-foreground")}
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

