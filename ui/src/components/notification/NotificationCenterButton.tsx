import { type RefObject, useCallback, useRef, useState } from "react";
import { Bell } from "lucide-react";

import { cn } from "../../lib/cn";
import {
  notificationCenterStore,
  type NotificationCenterStore,
} from "../../lib/notificationCenter/notificationCenterStore";
import { actionableCount } from "../../lib/notificationCenter/attentionView";
import type { Agent } from "../../features/ferryx/control/client";
import type { DesktopWorkspace } from "../../features/ferryx/control/desktopInventory";
import { IconButton } from "../ui/IconButton";
import { NotificationCenterPopover, type IsSessionNavigable } from "./NotificationCenterPopover";
import { useNotificationCenter } from "./useNotificationCenter";

export interface NotificationCenterButtonProps {
  onNavigateToSession?: (target: { workspaceId: string; sessionId: string; revision: number }) => void;
  isSessionNavigable?: IsSessionNavigable;
  className?: string;
  store?: NotificationCenterStore;
  anchorRef?: RefObject<HTMLElement | null>;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  isNotificationCenterOpen?: boolean;
  onOpenChangeNotificationCenter?: (open: boolean) => void;
  attentionInventory?: {
    workspaces: DesktopWorkspace[];
    unavailableHosts?: readonly string[];
    onSelectAgent: (agent: Agent) => void;
  };
  /** False when the caller renders the popover itself, so the dialog is never mounted twice. */
  renderPopover?: boolean;
}

export function NotificationCenterButton({
  onNavigateToSession,
  isSessionNavigable,
  className,
  store = notificationCenterStore,
  anchorRef: externalAnchorRef,
  open: externalOpen,
  onOpenChange,
  isNotificationCenterOpen,
  onOpenChangeNotificationCenter,
  attentionInventory,
  renderPopover = true,
}: NotificationCenterButtonProps) {
  const [internalOpen, setInternalOpen] = useState(false);
  // When embedded in Sidebar, props may include Sidebar's own `open` prop (whether the sidebar itself is open).
  // Therefore, prefer the explicit `isNotificationCenterOpen` if present over `externalOpen`.
  const controlledOpen = isNotificationCenterOpen !== undefined ? isNotificationCenterOpen : externalOpen;
  const isControlled = controlledOpen !== undefined;
  const open = isControlled ? controlledOpen : internalOpen;
  const setOpenChange = onOpenChangeNotificationCenter ?? onOpenChange;

  // Stable identity. This component subscribes to the notification store, so every
  // arriving notification (and every Sidebar re-render) produced a fresh inline
  // `onClose`, which tore down and re-ran the popover's focus-trap effect: focus was
  // yanked to the trigger and then back to the first focusable element. A keyboard
  // user who had tabbed to "Mark all read" or a specific row was thrown to the top of
  // the popover on every incoming notification, with the screen-reader announcement
  // repeating each time. It also re-registered the keydown/resize listeners each cycle.
  const handleClose = useCallback(() => {
    if (isControlled) {
      setOpenChange?.(false);
    } else {
      setInternalOpen(false);
    }
  }, [isControlled, setOpenChange]);

  const handleToggle = useCallback(() => {
    if (isControlled) {
      setOpenChange?.(!open);
    } else {
      setInternalOpen((prev) => !prev);
    }
  }, [isControlled, open, setOpenChange]);

  const internalAnchorRef = useRef<HTMLDivElement>(null);
  const anchorRef = externalAnchorRef ?? internalAnchorRef;
  const { state } = useNotificationCenter(store);
  // The badge answers "does anything need me?", so it counts blocked-on-you work and finished
  // work nobody has looked at — never plain mentions, or it would stay lit permanently.
  const actionable = actionableCount(state.entries);

  return (
    <div ref={internalAnchorRef} className="no-drag relative inline-flex">
      <IconButton
        data-testid="notification-center-button"
        data-shortcut="notifications.toggle"
        label="Notifications"
        size="sm"
        className={cn("no-drag relative", className)}
        onClick={(e) => {
          e.stopPropagation();
          handleToggle();
        }}
        onPointerDown={(e) => {
          e.stopPropagation();
        }}
      >
        <Bell className="size-3.5" />
        {actionable > 0 ? (
          <span
            data-testid="notification-center-badge"
            className="absolute -top-1 -right-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-primary px-0.5 text-[9px] font-semibold text-primary-foreground leading-none"
          >
            {actionable > 99 ? "99+" : actionable}
          </span>
        ) : null}
      </IconButton>
      {open && renderPopover ? (
        <NotificationCenterPopover
          anchorRef={anchorRef}
          onClose={handleClose}
          onNavigateToSession={onNavigateToSession}
          isSessionNavigable={isSessionNavigable}
          store={store}
          attentionInventory={attentionInventory}
        />
      ) : null}
    </div>
  );
}
