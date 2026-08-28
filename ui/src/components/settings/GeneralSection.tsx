import { useCallback, useEffect, useState } from "react";
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
  loadSidebarOpenStartup,
  saveSidebarOpenStartup,
  useGeneralSettings,
} from "../../lib/generalSettings";
import {
  checkForUpdate,
  downloadAndInstallUpdate,
  getCurrentVersion,
  getUpdateStatus,
  relaunchApp,
  subscribeUpdateStatus,
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
import { Progress } from "../ui/progress";
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

export function SoftwareUpdateCard() {
  const [status, setStatus] = useState<UpdateStatus>(() => getUpdateStatus());
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);

  useEffect(() => subscribeUpdateStatus(setStatus), []);

  useEffect(() => {
    let active = true;
    void getCurrentVersion().then((version) => {
      if (active) setCurrentVersion(version);
    });
    return () => {
      active = false;
    };
  }, []);

  const busy = status.state === "checking" || status.state === "downloading";
  const isAvailable = status.state === "available";
  const isDownloaded = status.state === "downloaded";
  const isActionable = isAvailable || isDownloaded;
  const percent = Math.round((status.downloadProgress ?? 0) * 100);

  const handleInstallAndRelaunch = () => {
    if (isAvailable) {
      void downloadAndInstallUpdate();
    } else if (isDownloaded) {
      void relaunchApp();
    }
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
        className={`mt-2 text-[12px] leading-5 ${
          isDownloaded
            ? "flex items-center gap-1.5 font-medium text-status-success"
            : status.state === "error"
              ? "text-destructive"
              : "text-foreground"
        }`}
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
          disabled={busy}
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
      const s = await getCliLauncherStatus();
      setStatus(s);
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
      const s = await installCliLauncher();
      setStatus(s);
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
      <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
        Ferryx does not alter shell profiles or PATH. Ensure{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">~/.local/bin</code> is on
        PATH, then open a new terminal.
      </p>

      {error ? (
        <Alert
          variant="destructive"
          className="mt-3 flex items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-xs text-destructive [&>svg]:static [&>svg~*]:pl-0"
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <AlertDescription className="text-xs leading-normal">{error}</AlertDescription>
        </Alert>
      ) : null}

      <div className="mt-3 flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-0.5 text-xs">
          <div className="text-muted-foreground">
            Launcher location:{" "}
            <code className="font-mono text-foreground">{status?.launcherPath ?? "~/.local/bin/ferryx"}</code>
          </div>
          {status?.currentTarget ? (
            <div className="truncate font-mono text-[10px] text-muted-foreground">
              Target: {status.currentTarget}
            </div>
          ) : null}
        </div>

        <div className="flex items-center gap-2">
          {status?.isSupported === false ? (
            <span className="text-xs text-muted-foreground">Available in the Ferryx desktop app on Unix-like systems.</span>
          ) : status?.isInstalled ? (
            <Badge variant="outline" className="inline-flex items-center gap-1 border-transparent px-0 text-xs font-medium text-status-success shadow-none">
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
          )}
        </div>
      </div>
    </Card>
  );
}

export function GeneralSection() {
  const { settings, updateSettings } = useGeneralSettings();
  const [sidebarOpenStartup, setSidebarOpenStartup] = useState<boolean>(() => loadSidebarOpenStartup());

  return (
    <section aria-labelledby="settings-general-heading" aria-label="General">
      <SettingsHeading
        icon={<MonitorCog />}
        title="General"
        description="Tab behavior, startup, CLI helper, and software updates."
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
        </div>
        <CliLauncherCard />
        <SoftwareUpdateCard />
      </div>
    </section>
  );
}

export { GeneralSection as GeneralSettings };
export default GeneralSection;
