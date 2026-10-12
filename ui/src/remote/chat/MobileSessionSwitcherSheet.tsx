import { Plus, Terminal, type LucideIcon } from "lucide-react";
import type { RemoteTerminalTabInfo } from "../RemoteSessionList";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";
import { touchStatusLabel } from "../remoteInventoryRows";
import { MobileSheet, MobileSheetItem } from "./MobileSheet";

/**
 * Mobile session switcher bottom sheet (plan todo 7e): lists open tabs in the current
 * or sibling worktrees so phones can switch panes without opening the full inventory drawer.
 * Reference: paseo tab-switcher.tsx:243-470 (Apache-2.0).
 */

export interface MobileSessionSwitcherSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  tabs: RemoteTerminalTabInfo[];
  activeTabId?: string | null;
  worktreeLabel?: string | null;
  pending?: unknown;
  disabled?: boolean;
  onSelectTab: (tab: RemoteTerminalTabInfo) => void;
  onNewTerminal: () => void;
}

export function MobileSessionSwitcherSheet({
  open,
  onOpenChange,
  tabs,
  activeTabId,
  worktreeLabel,
  pending,
  disabled = false,
  onSelectTab,
  onNewTerminal,
}: MobileSessionSwitcherSheetProps) {
  const isPending = Boolean(pending);

  const footer = (
    <button
      type="button"
      data-testid="session-switcher-new-terminal"
      disabled={isPending || disabled}
      onClick={() => {
        onNewTerminal();
        onOpenChange(false);
      }}
      className="flex h-11 w-full items-center justify-center gap-2 rounded-xl border border-chat-border bg-chat-surface-raised font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:cursor-not-allowed disabled:opacity-50"
    >
      <Plus className="size-4 shrink-0" aria-hidden="true" />
      <span>New terminal</span>
    </button>
  );

  return (
    <MobileSheet
      open={open}
      onOpenChange={onOpenChange}
      title="Sessions"
      ariaLabel="Sessions"
      footer={footer}
      testId="mobile-session-switcher-sheet"
    >
      {tabs.map((tab) => {
        const logo = resolveAgentLogo(tab.agentType);
        const monochrome = isMonochromeAgentLogo(tab.agentType);

        const TabIcon = logo
          ? (((props: { className?: string; "aria-hidden"?: boolean | "true" | "false" }) => (
              <img
                src={logo}
                alt=""
                data-agent-type={tab.agentType}
                className={cn(
                  "size-4 shrink-0",
                  monochrome && "agent-tab-logo--monochrome opacity-80",
                  props.className,
                )}
              />
            )) as unknown as LucideIcon)
          : Terminal;

        const description =
          tab.worktreeLabel && tab.worktreeLabel !== worktreeLabel
            ? tab.worktreeLabel
            : undefined;

        const status = touchStatusLabel((tab.activityState as any) ?? null);
        const trailing = status ? (
          <span
            aria-hidden="true"
            className={cn("text-[12px] font-medium tabular-nums", status.className)}
          >
            {status.text}
          </span>
        ) : null;

        return (
          <MobileSheetItem
            key={tab.id}
            icon={TabIcon}
            label={tab.label}
            description={description}
            trailing={trailing}
            selected={tab.id === activeTabId}
            disabled={isPending}
            onSelect={() => {
              onSelectTab(tab);
            }}
            testId={"session-switcher-tab-" + tab.id}
          />
        );
      })}
    </MobileSheet>
  );
}

