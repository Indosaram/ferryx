import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  MonitorCog,
  RotateCcw,
  RotateCw,
  TerminalSquare,
  X,
} from "lucide-react";

import {
  loadSidebarOpenStartup,
  saveSidebarOpenStartup,
  useGeneralSettings,
} from "../../lib/generalSettings";
import {
  cancelUpdateDownload,
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
  getDaemonSupervisor,
  installCliLauncher,
  setDaemonSupervisor,
} from "../../lib/tauri";
import { classifyNavigatorPlatform } from "../../lib/platform";
import { saveAdvancedFeatures, useAdvancedFeatures } from "../../lib/advancedFeatures";
import type { CliLauncherStatus } from "../../lib/types";

import { SettingRow, SettingsHeading } from "./primitives";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Progress } from "../ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Switch } from "../ui/switch";

function updateStatusMessage(status: UpdateStatus): string {
  switch (status.state) {
    case "checking":
      return "Checking for updates…";
    case "available":
      if (status.lastFailure === "stalled") {
        return `Download of version ${status.version} stalled. Nothing was installed; retry when ready.`;
      }
      if (status.lastFailure === "cancelled") {
        return `Download of version ${status.version} was cancelled. Nothing was installed.`;
      }
      return `Version ${status.version} is available.`;
    case "downloading":
      if (status.installing) return `Installing version ${status.version}…`;
      if (status.cancelRequested) return "Cancelling download…";
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
  const platform = classifyNavigatorPlatform(navigator);
  return platform === "windows" || platform === "linux" ? platform : "unknown";
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
  // Installation is not cancellable, so Cancel exists only while bytes are still downloading.
  const isCancellable = status.state === "downloading" && !status.installing;
  const percent = Math.round((status.downloadProgress ?? 0) * 100);

  let statusClassName = "text-foreground";
  if (isDownloaded) statusClassName = "flex items-center gap-1.5 font-medium text-status-success";
  if (status.state === "error" || status.lastFailure === "stalled") statusClassName = "text-destructive";

  const handleInstallAndRelaunch = () => {
    if (isAvailable) {
      // The button promises install AND relaunch, so finish the job once the download stages;
      // otherwise the card sits on "ready to install" waiting for an identical second click.
      void downloadAndInstallUpdate().then(() => {
        if (getUpdateStatus().state === "downloaded") return relaunchApp();
      });
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
          {isAvailable && status.lastFailure ? "Retry Install and Relaunch" : "Install and Relaunch"}
        </Button>
        {isCancellable ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void cancelUpdateDownload()}
            disabled={status.cancelRequested === true}
            className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            <X className="size-3" />
            Cancel
          </Button>
        ) : null}
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
  const advancedFeatures = useAdvancedFeatures();
  const [supervisor, setSupervisor] = useState<{ supported: boolean; enabled: boolean } | null>(null);
  const [supervisorBusy, setSupervisorBusy] = useState(false);
  const [supervisorError, setSupervisorError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void updatesManagedExternally().then((managed) => {
      if (active) setManagedExternally(managed);
    });
    getDaemonSupervisor()
      .then((state) => {
        if (active) setSupervisor(state);
      })
      .catch(() => {
        // Hosts or daemons that cannot report a supervisor do not offer the switch.
        if (active) setSupervisor(null);
      });
    return () => {
      active = false;
    };
  }, []);

  const toggleSupervisor = async (enabled: boolean) => {
    const previous = supervisor;
    if (!previous) return;
    setSupervisor({ ...previous, enabled });
    setSupervisorBusy(true);
    setSupervisorError(null);
    try {
      await setDaemonSupervisor(enabled);
    } catch (err) {
      setSupervisor(previous);
      const message =
        typeof err === "object" && err !== null && "message" in err && typeof err.message === "string"
          ? err.message
          : "Failed to update background service setting";
      setSupervisorError(message);
    } finally {
      setSupervisorBusy(false);
    }
  };

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
            label="Advanced features"
            description="Show the built-in browser, agent run graph, supervisor, and the Browser and Notifications settings. Open browser panes and keyboard shortcuts keep working either way."
          >
            <Switch
              id="general-advanced-features"
              aria-label="Advanced features"
              checked={advancedFeatures}
              onCheckedChange={(checked) => saveAdvancedFeatures(checked)}
            />
          </SettingRow>
          {supervisor?.supported ? (
            <SettingRow
              label="Restart background service automatically"
              description={
                <>
                  Let the operating system restart the Ferryx background service if it stops, so
                  terminal sessions come back without opening the app.
                  {supervisorError ? (
                    <span role="alert" className="mt-1 block text-destructive">
                      {supervisorError}
                    </span>
                  ) : null}
                </>
              }
            >
              <Switch
                id="general-daemon-supervisor"
                aria-label="Restart background service automatically"
                checked={supervisor.enabled}
                disabled={supervisorBusy}
                onCheckedChange={(checked) => void toggleSupervisor(checked)}
              />
            </SettingRow>
          ) : null}
        </div>
        <CliLauncherCard />
        <SoftwareUpdateCard />
      </div>
    </section>
  );
}

export { GeneralSection as GeneralSettings };
export default GeneralSection;
