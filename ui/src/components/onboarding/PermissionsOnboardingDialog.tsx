import { useCallback, useEffect, useRef, useState } from "react";
import { CheckCircle2, ExternalLink, Info, Shield } from "lucide-react";

import {
  getSystemPermissionsStatus,
  openPermissionsSystemSettings,
  requestAccessibilityPermission,
  requestNotificationPermission,
} from "../../lib/tauri";
import type { PermissionItemStatus, SystemPermissionsStatus } from "../../lib/types";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";

const POLL_INTERVAL_MS = 2000;

function StatusBadge({ item }: { item?: PermissionItemStatus }) {
  if (!item) {
    return (
      <Badge variant="outline" className="text-muted-foreground border-border">
        Loading…
      </Badge>
    );
  }
  if (item.status === "unsupported") {
    return (
      <Badge variant="outline" className="text-muted-foreground border-border text-xs">
        Not applicable
      </Badge>
    );
  }
  if (item.granted) {
    return (
      <Badge variant="secondary" className="bg-emerald-500/15 text-emerald-400 border-emerald-500/30">
        Granted
      </Badge>
    );
  }
  return (
    <Badge variant="destructive" className="bg-amber-500/15 text-amber-400 border-amber-500/30">
      Required
    </Badge>
  );
}

function grantedCount(status: SystemPermissionsStatus | null): number {
  if (!status) return 0;
  let count = 0;
  if (status.fullDiskAccess.granted) count += 1;
  if (status.accessibility.granted) count += 1;
  if (status.notifications.granted) count += 1;
  return count;
}

export function PermissionsOnboardingDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: (dontShowAgain: boolean) => void;
}) {
  const [status, setStatus] = useState<SystemPermissionsStatus | null>(null);
  const isMountedRef = useRef(true);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await getSystemPermissionsStatus();
      if (isMountedRef.current) {
        setStatus(res);
      }
    } catch {
      if (isMountedRef.current) {
        setStatus(null);
      }
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    isMountedRef.current = true;
    void fetchStatus();

    const interval = setInterval(() => {
      void fetchStatus();
    }, POLL_INTERVAL_MS);
    const handleFocus = () => {
      void fetchStatus();
    };
    window.addEventListener("focus", handleFocus);

    return () => {
      isMountedRef.current = false;
      clearInterval(interval);
      window.removeEventListener("focus", handleFocus);
    };
  }, [open, fetchStatus]);

  const handleOpenSettings = useCallback(
    async (target: "full_disk_access" | "accessibility" | "notifications") => {
      try {
        await openPermissionsSystemSettings(target);
      } catch {
        if (isMountedRef.current) setStatus(null);
      }
    },
    []
  );

  const handleRequestAccessibility = useCallback(async () => {
    try {
      await requestAccessibilityPermission();
      void fetchStatus();
    } catch {
      if (isMountedRef.current) setStatus(null);
    }
  }, [fetchStatus]);

  const handleRequestNotifications = useCallback(async () => {
    try {
      await requestNotificationPermission();
      void fetchStatus();
    } catch {
      if (isMountedRef.current) setStatus(null);
    }
  }, [fetchStatus]);

  if (!open) return null;

  const allGranted = status?.allGranted ?? false;
  const granted = grantedCount(status);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        role="dialog"
        aria-label="Welcome to Ferryx"
        className="bg-background border border-border rounded-xl shadow-2xl w-full max-w-lg p-6"
      >
        <div className="flex items-start gap-3">
          <div className="rounded-lg bg-primary/10 p-2 shrink-0">
            <Shield className="size-5 text-primary" />
          </div>
          <div className="space-y-1">
            <h2 className="text-lg font-semibold tracking-tight text-foreground">
              Welcome to Ferryx
            </h2>
            <p className="text-sm text-muted-foreground leading-relaxed">
              A one-time setup: grant three macOS permissions so terminals, agents, file access,
              and alerts work without interruptions.
            </p>
          </div>
        </div>

        <p className="mt-4 text-xs font-medium text-muted-foreground">
          {granted} of 3 granted
        </p>

        <div className="mt-3 space-y-3">
          <Card className="p-4 bg-card/60 border-border">
            <div className="flex items-center gap-2">
              <span className="font-medium text-sm text-foreground">Full Disk Access</span>
              <StatusBadge item={status?.fullDiskAccess} />
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground leading-relaxed">
              Allows terminal tools and git worktrees to inspect project files without folder
              access prompts.
            </p>
            <div className="pt-1 flex items-center gap-1.5 text-[11px] text-muted-foreground">
              <Info className="size-3.5 shrink-0" />
              <span>
                macOS opens the Full Disk Access pane — toggle Ferryx on in the list.
              </span>
            </div>
            {!status?.fullDiskAccess.granted ? (
              <div className="mt-3 flex justify-end gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  data-testid="onboarding-open-fda-settings"
                  onClick={() => handleOpenSettings("full_disk_access")}
                  className="gap-1.5 text-xs"
                >
                  <ExternalLink className="size-3.5" />
                  Open System Settings
                </Button>
              </div>
            ) : null}
          </Card>

          <Card className="p-4 bg-card/60 border-border">
            <div className="flex items-center gap-2">
              <span className="font-medium text-sm text-foreground">Accessibility</span>
              <StatusBadge item={status?.accessibility} />
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground leading-relaxed">
              Allows global keyboard shortcuts and native terminal focus management.
            </p>
            {!status?.accessibility.granted ? (
              <div className="mt-3 flex justify-end gap-2">
                {status?.accessibility.canRequest ? (
                  <Button
                    variant="outline"
                    size="sm"
                    data-testid="onboarding-request-accessibility"
                    onClick={handleRequestAccessibility}
                    className="gap-1.5 text-xs"
                  >
                    Request Access
                  </Button>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    data-testid="onboarding-open-accessibility-settings"
                    onClick={() => handleOpenSettings("accessibility")}
                    className="gap-1.5 text-xs"
                  >
                    <ExternalLink className="size-3.5" />
                    Open System Settings
                  </Button>
                )}
              </div>
            ) : null}
          </Card>

          <Card className="p-4 bg-card/60 border-border">
            <div className="flex items-center gap-2">
              <span className="font-medium text-sm text-foreground">Notifications</span>
              <StatusBadge item={status?.notifications} />
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground leading-relaxed">
              Allows desktop alerts for agent task completions and updates.
            </p>
            {!status?.notifications.granted ? (
              <div className="mt-3 flex justify-end gap-2">
                {status?.notifications.canRequest ? (
                  <Button
                    variant="outline"
                    size="sm"
                    data-testid="onboarding-request-notifications"
                    onClick={handleRequestNotifications}
                    className="gap-1.5 text-xs"
                  >
                    Enable Notifications
                  </Button>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    data-testid="onboarding-open-notifications-settings"
                    onClick={() => handleOpenSettings("notifications")}
                    className="gap-1.5 text-xs"
                  >
                    <ExternalLink className="size-3.5" />
                    Open System Settings
                  </Button>
                )}
              </div>
            ) : null}
          </Card>
        </div>

        <div className="mt-6">
          {allGranted ? (
            <div className="flex items-center justify-between gap-4">
              <p className="flex items-center gap-1.5 text-sm text-emerald-400">
                <CheckCircle2 className="size-4" />
                All permissions granted — you're all set.
              </p>
              <Button
                size="sm"
                data-testid="onboarding-get-started"
                onClick={() => onClose(true)}
              >
                Get Started
              </Button>
            </div>
          ) : (
            <div className="flex items-center justify-between gap-4">
              <Button
                variant="ghost"
                size="sm"
                data-testid="onboarding-dont-show-again"
                onClick={() => onClose(true)}
                className="text-muted-foreground"
              >
                Don't show again
              </Button>
              <Button
                variant="outline"
                size="sm"
                data-testid="onboarding-remind-later"
                onClick={() => onClose(false)}
              >
                Remind Me Later
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
