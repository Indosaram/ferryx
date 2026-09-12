import { type RefObject, useRef, useState } from "react";
import { Bell } from "lucide-react";

import { cn } from "../../lib/cn";
import {
  notificationCenterStore,
  type NotificationCenterStore,
} from "../../lib/notificationCenter/notificationCenterStore";
import { IconButton } from "../ui/IconButton";
import { NotificationCenterPopover, type IsSessionNavigable } from "./NotificationCenterPopover";
import { useNotificationCenter } from "./useNotificationCenter";

export interface NotificationCenterButtonProps {
  onNavigateToSession?: (target: { workspaceId: string; sessionId: string; revision: number }) => void;
  isSessionNavigable?: IsSessionNavigable;
  className?: string;
  store?: NotificationCenterStore;
  anchorRef?: RefObject<HTMLElement | null>;
}

export function NotificationCenterButton({
  onNavigateToSession,
  isSessionNavigable,
  className,
  store = notificationCenterStore,
  anchorRef: externalAnchorRef,
}: NotificationCenterButtonProps) {
  const [open, setOpen] = useState(false);
  const internalAnchorRef = useRef<HTMLDivElement>(null);
  const anchorRef = externalAnchorRef ?? internalAnchorRef;
  const { unreadCount } = useNotificationCenter(store);

  return (
    <div ref={internalAnchorRef} className="no-drag relative inline-flex">
      <IconButton
        data-testid="notification-center-button"
        label="Notifications"
        size="sm"
        className={cn("no-drag relative", className)}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((prev) => !prev);
        }}
        onPointerDown={(e) => {
          e.stopPropagation();
        }}
      >
        <Bell className="size-3.5" />
        {unreadCount > 0 ? (
          <span
            data-testid="notification-center-badge"
            className="absolute -top-1 -right-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-primary px-0.5 text-[9px] font-semibold text-primary-foreground leading-none"
          >
            {unreadCount > 99 ? "99+" : unreadCount}
          </span>
        ) : null}
      </IconButton>
      {open ? (
        <NotificationCenterPopover
          anchorRef={anchorRef}
          onClose={() => setOpen(false)}
          onNavigateToSession={onNavigateToSession}
          isSessionNavigable={isSessionNavigable}
          store={store}
        />
      ) : null}
    </div>
  );
}
