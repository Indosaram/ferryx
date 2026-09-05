import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  Info,
  RefreshCw,
  Shield,
} from "lucide-react";

import { isMacShortcutPlatform } from "../../lib/shortcuts";
import {
  getSystemPermissionsStatus,
  openPermissionsSystemSettings,
  requestAccessibilityPermission,
} from "../../lib/tauri";
import type { PermissionItemStatus, SystemPermissionsStatus } from "../../lib/types";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { toast } from "../ui/sonner";

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

export function PermissionsSection() {
  const [status, setStatus] = useState<SystemPermissionsStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const isMountedRef = useRef(true);
  const isMac = isMacShortcutPlatform();

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

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
    } finally {
      if (isMountedRef.current) {
        setLoading(false);
        setRefreshing(false);
      }
    }
  }, []);

  useEffect(() => {
    void fetchStatus();
  }, [fetchStatus]);

  useEffect(() => {
    const handleFocus = () => {
      void fetchStatus();
    };
    window.addEventListener("focus", handleFocus);
    return () => {
      window.removeEventListener("focus", handleFocus);
    };
  }, [fetchStatus]);

  const handleRefresh = useCallback(() => {
    setRefreshing(true);
    void fetchStatus();
  }, [fetchStatus]);

  const handleOpenSettings = useCallback(async (target: string) => {
    try {
      const res = await openPermissionsSystemSettings(target);
      if (!res.opened && res.reason) {
        toast.error(`Could not open system settings: ${res.reason}`);
      }
    } catch {
      setStatus(null);
    }
  }, []);

  const handleRequestAccessibility = useCallback(async () => {
    try {
      await requestAccessibilityPermission();
      void fetchStatus();
    } catch {
      setStatus(null);
    }
  }, [fetchStatus]);

  const allGranted = status?.allGranted ?? false;

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h2 className="text-lg font-semibold tracking-tight text-foreground flex items-center gap-2">
            <Shield className="size-5 text-primary" />
            System Permissions
          </h2>
          <p className="text-sm text-muted-foreground">
            {isMac
              ? "Configure macOS permissions for terminal execution, file system access, and notifications."
              : "System permissions for terminal execution, background tasks, and notifications."}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={handleRefresh}
          disabled={loading || refreshing}
          className="gap-2"
        >
          <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin" : ""}`} />
          Refresh Status
        </Button>
      </div>

      {allGranted ? (
        <Alert className="border-emerald-500/30 bg-emerald-500/10 text-emerald-300">
          <CheckCircle2 className="size-4 text-emerald-400" />
          <AlertDescription className="text-xs">
            All system permissions granted. Ferryx has full access for terminal commands, subagents, and alerts.
          </AlertDescription>
        </Alert>
      ) : (
        <Alert className="border-amber-500/30 bg-amber-500/10 text-amber-300">
          <AlertTriangle className="size-4 text-amber-400" />
          <AlertDescription className="text-xs">
            Granting Full Disk Access stops macOS from showing alerts such as &ldquo;Ferryx would like to access your Photo Library&rdquo; or folder access prompts when subagents and terminal tools inspect files.
          </AlertDescription>
        </Alert>
      )}

      <div className="space-y-4">
        <Card className="p-4 bg-card/60 border-border">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm text-foreground">Full Disk Access</span>
                <StatusBadge item={status?.fullDiskAccess} />
              </div>
              <p className="text-xs text-muted-foreground leading-relaxed">
                {status?.fullDiskAccess.description ||
                  (loading ? "Checking permission status…" : "Allows terminal tools and git worktrees to inspect project files without folder access prompts.")}
              </p>
              {isMac && !status?.fullDiskAccess.granted && (
                <div className="pt-1 flex items-center gap-1.5 text-[11px] text-amber-400/90">
                  <Info className="size-3.5 shrink-0" />
                  <span>
                    Prevents &ldquo;Ferryx would like to access your Photo Library&rdquo; and Documents/Downloads prompts.
                  </span>
                </div>
              )}
            </div>
            <div className="shrink-0">
              <Button
                variant="secondary"
                size="sm"
                data-testid="open-fda-settings"
                disabled={status?.fullDiskAccess.status === "unsupported"}
                onClick={() => handleOpenSettings("full_disk_access")}
                className="gap-1.5 text-xs"
              >
                <ExternalLink className="size-3.5" />
                Open System Settings
              </Button>
            </div>
          </div>
        </Card>

        <Card className="p-4 bg-card/60 border-border">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm text-foreground">Accessibility</span>
                <StatusBadge item={status?.accessibility} />
              </div>
              <p className="text-xs text-muted-foreground leading-relaxed">
                {status?.accessibility.description ||
                  (loading ? "Checking permission status…" : "Allows global keyboard shortcuts and native terminal focus management.")}
              </p>
            </div>
            <div className="shrink-0 flex items-center gap-2">
              {status?.accessibility.canRequest && !status.accessibility.granted ? (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleRequestAccessibility}
                  className="gap-1.5 text-xs"
                >
                  Request Access
                </Button>
              ) : null}
              <Button
                variant="secondary"
                size="sm"
                data-testid="open-accessibility-settings"
                disabled={status?.accessibility.status === "unsupported"}
                onClick={() => handleOpenSettings("accessibility")}
                className="gap-1.5 text-xs"
              >
                <ExternalLink className="size-3.5" />
                Open System Settings
              </Button>
            </div>
          </div>
        </Card>

        <Card className="p-4 bg-card/60 border-border">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm text-foreground">Desktop Notifications</span>
                <StatusBadge item={status?.notifications} />
              </div>
              <p className="text-xs text-muted-foreground leading-relaxed">
                {status?.notifications.description ||
                  (loading ? "Checking permission status…" : "Allows desktop alerts for agent task completions and updates.")}
              </p>
            </div>
            <div className="shrink-0">
              <Button
                variant="secondary"
                size="sm"
                data-testid="open-notifications-settings"
                disabled={status?.notifications.status === "unsupported"}
                onClick={() => handleOpenSettings("notifications")}
                className="gap-1.5 text-xs"
              >
                <ExternalLink className="size-3.5" />
                Open System Settings
              </Button>
            </div>
          </div>
        </Card>
      </div>

      {isMac && (
        <div className="rounded-lg border border-border/70 bg-muted/20 p-4 text-xs text-muted-foreground space-y-2">
          <p className="font-medium text-foreground">How to grant permissions in macOS:</p>
          <ol className="list-decimal pl-4 space-y-1">
            <li>Click &ldquo;Open System Settings&rdquo; next to the permission you wish to grant.</li>
            <li>In the macOS System Settings pane that opens, find Ferryx in the list and toggle the switch to ON.</li>
            <li>If Ferryx is not listed, click the &ldquo;+&rdquo; button and select Ferryx from Applications.</li>
            <li>Return to Ferryx and click &ldquo;Refresh Status&rdquo; to confirm the permission is granted.</li>
          </ol>
        </div>
      )}
    </div>
  );
}
