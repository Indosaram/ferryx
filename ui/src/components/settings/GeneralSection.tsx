import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  MonitorCog,
  RotateCcw,
  RotateCw,
  TerminalSquare,
} from "lucide-react";

import {
  DEFAULT_GENERAL_SETTINGS,
  loadSidebarOpenStartup,
  MAX_SESSION_IDLE_TIMEOUT_MINUTES,
  MIN_SESSION_IDLE_TIMEOUT_MINUTES,
  saveSidebarOpenStartup,
  SESSION_IDLE_TIMEOUT_OFF_MINUTES,
  useGeneralSettings,
} from "../../lib/generalSettings";
import {
  checkForUpdate,
  downloadAndInstallUpdate,
  getCurrentVersion,
  getUpdateStatus,
  relaunchApp,
  subscribeUpdateStatus,
  updatesManagedExternally,
  type UpdateStatus,
} from "../../lib/updater";
import {
  getCliLauncherStatus,
  installCliLauncher,
} from "../../lib/tauri";
import type { CliLauncherStatus } from "../../lib/types";

import { SettingRow, SettingsHeading } from "./primitives";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Progress } from "../ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Switch } from "../ui/switch";

function updateStatusMessage(status: UpdateStatus): string {
  switch (status.state) {
    case "checking":
      return "Checking for updates…";
    case "available":
      return `Version ${status.version} is available.`;
    case "downloading":
      return `Downloading version ${status.version}…`;
    case "downloaded":
      return `Version ${status.version} is ready to install.`;
    case "error":
      return `Update failed: ${status.error}`;
    case "idle":
      return "Ferryx is up to date.";
  }
}

type ExternallyManagedHost = "windows" | "linux" | "unknown";

/**
 * Host platform of the running desktop shell. `updatesManagedExternally()` answers with one
 * boolean for two different owners — the Microsoft Store on Windows and the distribution
 * package manager on a Linux deb/rpm install — so the card has to name the owner this host
 * actually has. The remote web client never reaches that branch: outside the desktop runtime
 * the probe fails and reports `false`, so the webview platform is the host platform here.
 */
function externallyManagedHost(): ExternallyManagedHost {
  if (typeof navigator === "undefined") return "unknown";
  const signals = [navigator.userAgent, navigator.platform];
  if (signals.some((signal) => /Windows|Win32|Win64/i.test(signal))) return "windows";
  if (signals.some((signal) => /Linux|X11|CrOS/i.test(signal))) return "linux";
  return "unknown";
}

function externallyManagedUpdateMessage(host: ExternallyManagedHost): string {
  if (host === "windows") return "Updates are managed by the Microsoft Store.";
  if (host === "linux") return "Updates are managed by your system package manager.";
  return "Updates for this install are managed outside the app.";
}

export function SoftwareUpdateCard() {
  const [status, setStatus] = useState<UpdateStatus>(() => getUpdateStatus());
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [managedExternally, setManagedExternally] = useState<boolean>(false);

  useEffect(() => subscribeUpdateStatus(setStatus), []);

  useEffect(() => {
    let active = true;
    void getCurrentVersion().then((version) => {
      if (active) setCurrentVersion(version);
    });
    void updatesManagedExternally().then((managed) => {
      if (active) setManagedExternally(managed);
    });
    return () => {
      active = false;
    };
  }, []);

  if (managedExternally) {
    return (
      <Card className="rounded-lg border border-border bg-card p-4">
        <h3 className="text-[12px] font-semibold">Software Update</h3>
        <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
          Current version {currentVersion ?? "unknown"}. {externallyManagedUpdateMessage(externallyManagedHost())}
        </p>
      </Card>
    );
  }

  const busy = status.state === "checking" || status.state === "downloading";
  const isAvailable = status.state === "available";
  const isDownloaded = status.state === "downloaded";
  const isActionable = isAvailable || isDownloaded;
  const percent = Math.round((status.downloadProgress ?? 0) * 100);

  let statusClassName = "text-foreground";
  if (isDownloaded) statusClassName = "flex items-center gap-1.5 font-medium text-status-success";
  if (status.state === "error") statusClassName = "text-destructive";

  const handleInstallAndRelaunch = () => {
    if (isAvailable) {
      void downloadAndInstallUpdate();
      return;
    }
    if (isDownloaded) void relaunchApp();
  };

  return (
    <Card className="rounded-lg border border-border bg-card p-4">
      <h3 className="text-[12px] font-semibold">Software Update</h3>
      <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
        Current version {currentVersion ?? "unknown"}. Updates are signed and verified before they install.
      </p>
      <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
        Ferryx restarts, but active agents and terminal sessions continue in the background.
      </p>
      <p
        data-testid="settings-update-status"
        aria-live="polite"
        className={`mt-2 text-[12px] leading-5 ${statusClassName}`}
      >
        {isDownloaded ? <CheckCircle2 className="size-3.5 shrink-0" /> : null}
        <span>{updateStatusMessage(status)}</span>
      </p>
      {status.state === "downloading" || status.state === "downloaded" ? (
        <Progress
          value={percent}
          aria-label="Update download progress"
          className={`mt-2 h-1.5 w-full bg-accent ${
            isDownloaded ? "[&>div]:bg-status-success" : "[&>div]:bg-foreground"
          }`}
        />
      ) : null}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void checkForUpdate()}
          disabled={busy || isDownloaded}
          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
        >
          <RotateCw className="size-3" />
          Check for Updates
        </Button>
        <Button
          type="button"
          variant={isActionable ? "default" : "outline"}
          size="sm"
          onClick={handleInstallAndRelaunch}
          disabled={!isActionable}
          data-variant={isActionable ? "primary" : "secondary"}
          className={`no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium transition-colors ${
            isActionable
              ? "bg-primary text-primary-foreground hover:bg-primary/90 shadow-sm"
              : "border border-border text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
          }`}
        >
          <RotateCcw className="size-3" />
          Install and Relaunch
        </Button>
      </div>
    </Card>
  );
}

export function CliLauncherCard() {
  const [status, setStatus] = useState<CliLauncherStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setError(null);
      setStatus(await getCliLauncherStatus());
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retrieve CLI launcher status");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const handleInstall = async () => {
    setInstalling(true);
    setError(null);
    try {
      setStatus(await installCliLauncher());
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to install CLI launcher");
    } finally {
      setInstalling(false);
    }
  };

  return (
    <Card className="rounded-lg border border-border bg-card p-4">
      <div className="flex items-center gap-2">
        <TerminalSquare className="size-4 text-muted-foreground" />
        <h3 className="text-[12px] font-semibold">Ferryx CLI</h3>
      </div>
      {status?.isSupported !== false ? (
        <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
          Ferryx does not alter shell profiles or PATH. Ensure{" "}
          <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">~/.local/bin</code> is on
          PATH, then open a new terminal.
        </p>
      ) : null}

      {error ? (
        <Alert
          variant="destructive"
          className="mt-3 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-[11px] text-destructive [&>svg]:static [&>svg~*]:pl-0"
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <AlertDescription className="text-[11px] leading-normal">{error}</AlertDescription>
        </Alert>
      ) : null}

      <div className="mt-3 flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-0.5 text-[11px]">
          {status?.isSupported !== false ? (
            <div className="text-muted-foreground">
              Launcher location:{" "}
              <code className="font-mono text-foreground">{status?.launcherPath ?? "~/.local/bin/ferryx"}</code>
            </div>
          ) : null}
          {status?.currentTarget ? (
            <div className="truncate font-mono text-[11px] text-muted-foreground">
              Target: {status.currentTarget}
            </div>
          ) : null}
        </div>

        <div className="flex items-center gap-2">
          {status?.isSupported === false && (
            <span className="text-[11px] text-muted-foreground">Available in the Ferryx desktop app on Unix-like systems.</span>
          )}
          {status?.isSupported !== false && (status?.isInstalled ? (
            <Badge variant="outline" className="inline-flex items-center gap-1 border-transparent px-0 text-[11px] font-medium text-status-success shadow-none">
              <CheckCircle2 className="size-3.5" />
              Installed
            </Badge>
          ) : (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void handleInstall()}
              disabled={installing || loading}
              className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] font-medium text-foreground hover:bg-accent disabled:opacity-50 transition-colors"
            >
              {installing ? (
                <>
                  <RotateCw className="size-3 animate-spin" />
                  Installing Ferryx CLI…
                </>
              ) : (
                <>
                  <Download className="size-3" />
                  Install Ferryx CLI
                </>
              )}
            </Button>
          ))}
        </div>
      </div>
    </Card>
  );
}

export function GeneralSection() {
  const { settings, updateSettings } = useGeneralSettings();
  const [managedExternally, setManagedExternally] = useState<boolean>(false);
  const [sidebarOpenStartup, setSidebarOpenStartup] = useState<boolean>(() => loadSidebarOpenStartup());
  const autoSuspendEnabled = settings.sessionIdleTimeoutMinutes > 0;

  useEffect(() => {
    let active = true;
    void updatesManagedExternally().then((managed) => {
      if (active) setManagedExternally(managed);
    });
    return () => {
      active = false;
    };
  }, []);
  // Preserve the user's last custom timeout so toggling auto-suspend back on
  // restores it instead of silently resetting to the default.
  const lastEnabledIdleMinutesRef = useRef(
    autoSuspendEnabled ? settings.sessionIdleTimeoutMinutes : DEFAULT_GENERAL_SETTINGS.sessionIdleTimeoutMinutes,
  );
  useEffect(() => {
    if (autoSuspendEnabled) lastEnabledIdleMinutesRef.current = settings.sessionIdleTimeoutMinutes;
  }, [autoSuspendEnabled, settings.sessionIdleTimeoutMinutes]);

  return (
    <section aria-labelledby="settings-general-heading" aria-label="General">
      <SettingsHeading
        icon={<MonitorCog />}
        title="General"
        description={
          managedExternally
            ? "Tab behavior, startup, and CLI helper."
            : "Tab behavior, startup, CLI helper, and software updates."
        }
      />
      <h2 id="settings-general-heading" className="sr-only">General</h2>
      <div data-testid="settings-general-overview" className="space-y-6">
        <div className="border-y border-border">
          <SettingRow
            label="Confirm before closing a tab"
            description="Ask before closing a terminal or browser tab from the tab bar, menu, or keyboard shortcut."
          >
            <Switch
              id="general-confirm-close-tab"
              aria-label="Confirm before closing a tab"
              checked={settings.confirmCloseTab}
              onCheckedChange={(checked) => updateSettings({ confirmCloseTab: checked })}
            />
          </SettingRow>
          <SettingRow
            label="Show sidebar on startup"
            description="Keep the project sidebar open when Ferryx launches. Changes apply on next app start."
          >
            <Switch
              id="general-show-sidebar-startup"
              aria-label="Show sidebar on startup"
              checked={sidebarOpenStartup}
              onCheckedChange={(checked) => {
                setSidebarOpenStartup(checked);
                saveSidebarOpenStartup(checked);
              }}
            />
          </SettingRow>
          <SettingRow
            label="Session Restore Policy"
            description="Choose how terminal and agent processes are restored after Ferryx restarts. Lazy is recommended for lower memory use."
          >
            <Select
              value={settings.sessionRestorePolicy}
              onValueChange={(value) => updateSettings({ sessionRestorePolicy: value as typeof settings.sessionRestorePolicy })}
            >
              <SelectTrigger aria-label="Session Restore Policy" className="h-8 w-[180px] text-[12px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="lazy">Lazy (Recommended)</SelectItem>
                <SelectItem value="activeOnly">Active Only</SelectItem>
                <SelectItem value="eager">Eager (Legacy)</SelectItem>
              </SelectContent>
            </Select>
          </SettingRow>
          <SettingRow
            label="Auto-suspend idle sessions"
            description="Background sessions that have completed work are suspended (paused, resumable on focus) after this many minutes of inactivity. Turn off to keep them running."
          >
            <div className="flex items-center gap-2">
              <Input
                aria-label="Session idle timeout minutes"
                type="number"
                min={MIN_SESSION_IDLE_TIMEOUT_MINUTES}
                max={MAX_SESSION_IDLE_TIMEOUT_MINUTES}
                value={autoSuspendEnabled ? settings.sessionIdleTimeoutMinutes : lastEnabledIdleMinutesRef.current}
                disabled={!autoSuspendEnabled}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  // The switch is the only way to disable auto-suspend; transient
                  // edits (empty field, 0) must not silently turn it off.
                  if (Number.isFinite(next) && next > 0) updateSettings({ sessionIdleTimeoutMinutes: next });
                }}
                className="h-8 w-20 text-right text-[12px] disabled:opacity-50"
              />
              <span className="text-[11px] text-muted-foreground">min</span>
              <Switch
                id="general-auto-suspend"
                aria-label="Auto-suspend idle sessions"
                checked={autoSuspendEnabled}
                onCheckedChange={(checked) =>
                  updateSettings({
                    sessionIdleTimeoutMinutes: checked
                      ? lastEnabledIdleMinutesRef.current
                      : SESSION_IDLE_TIMEOUT_OFF_MINUTES,
                  })
                }
              />
            </div>
          </SettingRow>
        </div>
        <CliLauncherCard />
        <SoftwareUpdateCard />
      </div>
    </section>
  );
}

export { GeneralSection as GeneralSettings };
export default GeneralSection;
