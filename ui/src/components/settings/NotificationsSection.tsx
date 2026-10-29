import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  Bell,
  CheckCircle2,
  ExternalLink,
  RotateCcw,
} from "lucide-react";

import { Alert, AlertDescription } from "../ui/alert";
import { useNotificationSettings } from "../../lib/notificationSettings";
import {
  disablePhoneAlerts,
  enablePhoneAlerts,
  readPhoneAlertState,
  sendPhoneTestAlert,
  updatePhoneAlertPreferences,
  type PhoneAlertPreferences,
  type PhoneAlertState,
  type PhoneFinishedMode,
  type PushRequest,
} from "../../lib/pushSubscription";
import {
  getNotificationPermissionStatus,
  openNotificationSystemSettings,
  pickNotificationAudio,
  playNotificationSound,
  probeNotificationDelivery,
  requestNotificationPermission,
} from "../../lib/tauri";
import type { NotificationPermissionStatus } from "../../lib/types";

import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { Slider } from "../ui/slider";
import { Switch } from "../ui/switch";
import { SettingRow, SettingsGroup, SettingsHeading } from "./primitives";

type TestResult = { tone: "success" | "warning" | "error"; message: string };

function describeProbeOutcome(outcome: string | undefined): TestResult {
  switch (outcome) {
    case "submitted":
      return {
        tone: "success",
        message:
          "Test notification sent. If no banner appeared, Focus or Do Not Disturb is suppressing it.",
      };
    case "permission-required":
      return {
        tone: "warning",
        message:
          "Notification permission has not been granted, so nothing was delivered. Allow notifications to receive alerts.",
      };
    case "blocked-by-system":
      return {
        tone: "warning",
        message: "Notifications are blocked by system settings, so nothing was delivered.",
      };
    case "unsupported":
      return { tone: "error", message: "This system does not support notifications." };
    case "failed":
      return { tone: "error", message: "The system rejected the notification." };
    default:
      return { tone: "warning", message: "Notification delivery could not be confirmed." };
  }
}

const PHONE_ALERT_NOTICE: Partial<Record<PhoneAlertState, string>> = {
  denied: "Notifications are blocked in this browser. Allow them in the browser's site settings, then try again.",
  dismissed: "Notification permission was not granted.",
  insecure: "Phone alerts need a secure (HTTPS) connection to this computer.",
  unsupported: "This browser does not support push notifications.",
  error: "Phone alerts could not be turned on. Try again.",
};

const FINISHED_MODE_LABEL: Record<PhoneFinishedMode, string> = {
  off: "Off",
  longTurns: "Long turns only",
  everyTurn: "Every turn",
};

/**
 * Phone alerts for the paired remote web app: subscribes this browser to the gateway's Web Push
 * and keeps the gateway's copy of the alert preferences in step. Permission is asked only when
 * the user turns Phone alerts on, never on load.
 */
export function PhoneAlertsSettings({ request }: { request: PushRequest }) {
  const { settings, updateSettings } = useNotificationSettings();
  const [state, setState] = useState<PhoneAlertState | null>(null);
  const [busy, setBusy] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const preferences: PhoneAlertPreferences = {
    phoneNeedsInput: settings.phoneNeedsInput,
    phoneFinishedMode: settings.phoneFinishedMode,
  };

  useEffect(() => {
    let cancelled = false;
    void readPhoneAlertState().then((next) => {
      if (!cancelled) setState(next);
    });
    return () => { cancelled = true; };
  }, []);

  const enabled = state === "enabled";
  const unavailable = state === "unsupported" || state === "insecure";

  const handleToggle = async (checked: boolean) => {
    setBusy(true);
    setTestResult(null);
    try {
      setState(checked ? await enablePhoneAlerts(request, preferences) : await disablePhoneAlerts(request));
    } finally {
      setBusy(false);
    }
  };

  const changePreferences = (partial: Partial<PhoneAlertPreferences>) => {
    updateSettings(partial);
    if (enabled) void updatePhoneAlertPreferences(request, { ...preferences, ...partial });
  };

  const handleTest = async () => {
    setBusy(true);
    setTestResult(null);
    try {
      const result = await sendPhoneTestAlert(request);
      setTestResult(result.ok
        ? { tone: "success", message: "Test alert sent. It should appear on this phone in a moment." }
        : { tone: "error", message: result.message });
    } finally {
      setBusy(false);
    }
  };

  const notice = state ? PHONE_ALERT_NOTICE[state] : undefined;

  return (
    <SettingsGroup title="Phone alerts" description="Alerts on this phone when an agent needs you, even with the app closed.">
      <SettingRow label="Phone alerts" description="Allow this browser to receive alerts from this computer.">
        <Switch
          aria-label="Phone alerts"
          data-testid="phone-alerts-toggle"
          checked={enabled}
          disabled={busy || state === null || unavailable}
          onCheckedChange={(checked) => { void handleToggle(checked); }}
        />
      </SettingRow>
      {notice ? (
        <Alert role="alert" variant={state === "denied" || state === "error" ? "destructive" : "default"} className="my-2" data-testid="phone-alerts-notice">
          <AlertDescription className="text-[11px] leading-normal">{notice}</AlertDescription>
        </Alert>
      ) : null}
      <SettingRow label="Needs input" description="Alert when an agent is waiting for an answer.">
        <Switch
          aria-label="Phone alert when an agent needs input"
          checked={settings.phoneNeedsInput}
          onCheckedChange={(checked) => changePreferences({ phoneNeedsInput: checked })}
        />
      </SettingRow>
      <SettingRow label="Finished" description="Alert when an agent finishes a turn. Long turns are one minute or more.">
        <select
          aria-label="Phone alert when an agent finishes"
          value={settings.phoneFinishedMode}
          onChange={(event) => changePreferences({ phoneFinishedMode: event.target.value as PhoneFinishedMode })}
          className="h-8 rounded-md border border-input bg-background px-2 text-[12px] text-foreground"
        >
          {(Object.keys(FINISHED_MODE_LABEL) as PhoneFinishedMode[]).map((mode) => (
            <option key={mode} value={mode}>{FINISHED_MODE_LABEL[mode]}</option>
          ))}
        </select>
      </SettingRow>
      <SettingRow label="Test" description="Send a test alert from this computer to this phone.">
        <Button
          type="button"
          size="sm"
          data-testid="phone-alerts-test"
          onClick={() => { void handleTest(); }}
          disabled={!enabled || busy}
          className="h-7 shrink-0 px-2.5 text-[11px] font-medium"
        >
          Send test to this phone
        </Button>
      </SettingRow>
      {testResult ? (
        <Alert role="status" variant={testResult.tone === "error" ? "destructive" : "default"} className="mt-2">
          <AlertDescription className="text-[11px] leading-normal">{testResult.message}</AlertDescription>
        </Alert>
      ) : null}
    </SettingsGroup>
  );
}

export function NotificationsSection() {
  const { settings, updateSettings, resetSettings } = useNotificationSettings();
  const [permissionStatus, setPermissionStatus] = useState<NotificationPermissionStatus | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [requestNotice, setRequestNotice] = useState<string | null>(null);

  const refreshPermission = useCallback(async () => {
    try {
      const status = await getNotificationPermissionStatus();
      setPermissionStatus(status);
    } catch (err) {
      console.error("Failed to retrieve notification permission status:", err);
    }
  }, []);

  useEffect(() => {
    void refreshPermission();
    const onFocus = () => { void refreshPermission(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshPermission]);

  const auth = permissionStatus?.supported === false ? "unavailable"
    : permissionStatus?.authoritative === false ? "unknown"
    : permissionStatus?.authorization ?? "unknown";

  const isAuthorized = auth === "authorized" || auth === "provisional";
  const isDenied = auth === "denied";
  const canRequestPermission = auth !== "unavailable";

  const handleRequestPermission = async () => {
    const preClickAuth = auth;
    setRequestNotice(null);
    try {
      const result = await requestNotificationPermission();
      await refreshPermission();
      if (result?.error) {
        setRequestNotice(result.error);
        return;
      }
      if (preClickAuth === "authorized" || preClickAuth === "provisional") {
        setRequestNotice(
          "macOS does not re-prompt apps that already have permission. The badge capability was registered — enable 'Badge application icon' under System Settings → Notifications → Ferryx if the Dock badge is missing."
        );
      }
    } catch (err) {
      console.error("Failed to request notification permission:", err);
    }
  };

  const handleOpenSettings = async () => {
    try {
      await openNotificationSystemSettings();
    } catch (err) {
      console.error("Failed to open system notification settings:", err);
    }
  };

  const handlePickAudio = async () => {
    try {
      const file = await pickNotificationAudio();
      if (file?.path) {
        updateSettings({
          customSoundId: "custom",
          customSoundPath: file.path,
        });
      }
    } catch (err) {
      console.error("Failed to select custom audio file:", err);
    }
  };

  const handlePreviewSound = async () => {
    try {
      await playNotificationSound({
        soundId: settings.customSoundId,
        customSoundPath: settings.customSoundPath,
        volume: settings.customSoundVolume,
        force: true,
      });
    } catch (err) {
      console.error("Failed to preview sound:", err);
    }
  };

  const handleTestNotification = async () => {
    setIsTesting(true);
    setTestResult(null);
    try {
      let result = await probeNotificationDelivery(true, settings.customSoundId === "system" ? "system" : "silent");
      if (result?.outcome === "permission-required") {
        const requested = await requestNotificationPermission();
        if (requested?.error) {
          setTestResult({ tone: "error", message: requested.error });
          return;
        }
        result = await probeNotificationDelivery(true, settings.customSoundId === "system" ? "system" : "silent");
      }
      setTestResult(describeProbeOutcome(result?.outcome));
      // "system" delivers the OS default sound with the banner and "none" is muted;
      // only an explicit custom file goes through the audio player.
      if (settings.customSoundId === "custom" && settings.customSoundPath) {
        await handlePreviewSound();
      }
    } catch (err) {
      setTestResult({
        tone: "error",
        message: err instanceof Error ? err.message : "Failed to send test notification.",
      });
    } finally {
      setIsTesting(false);
    }
  };

  let permissionBadgeClass =
    "border-status-warning/20 bg-status-warning/10 text-status-warning";
  if (isAuthorized) {
    permissionBadgeClass =
      "border-status-success/20 bg-status-success/10 text-status-success";
  } else if (isDenied) {
    permissionBadgeClass =
      "border-destructive/20 bg-destructive/10 text-destructive";
  }

  return (
    <section aria-labelledby="settings-notifications-heading">
      <SettingsHeading
        icon={<Bell />}
        title="Notifications"
        description="Configure desktop alerts, audio feedback, and agent completion triggers."
      />
      <h2 id="settings-notifications-heading" className="sr-only">
        Notifications
      </h2>

      <Card className="mb-6 rounded-lg border border-border bg-card p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-[12px] font-medium text-muted-foreground">
              OS Permission Status:
            </span>
            <Badge
              data-testid="notification-permission-status"
              data-permission-state={auth}
              variant="outline"
              className={`gap-1 px-2 py-0.5 text-[11px] font-medium ${permissionBadgeClass}`}
            >
              {isAuthorized ? <CheckCircle2 className="size-3" /> : null}
              {isDenied ? <AlertTriangle className="size-3" /> : null}
              {auth}
            </Badge>
          </div>
          {canRequestPermission ? (
            <Button
              type="button"
              variant="link"
              size="sm"
              onClick={handleRequestPermission}
              className="h-auto p-0 text-[11px] text-primary hover:underline"
            >
              Request Permission
            </Button>
          ) : null}
          {permissionStatus?.canOpenSettings ? (
            <Button
              type="button"
              variant="link"
              size="sm"
              onClick={handleOpenSettings}
              className="flex h-auto items-center gap-1 p-0 text-[11px] text-muted-foreground hover:text-foreground hover:underline"
            >
              Open System Settings
              <ExternalLink className="size-3" />
            </Button>
          ) : null}
        </div>

        {permissionStatus &&
        permissionStatus.supported &&
        permissionStatus.platform === "macos" &&
        permissionStatus.badgesEnabled !== undefined &&
        permissionStatus.badgesEnabled !== null ? (
          <div className="mt-2.5 flex flex-wrap items-center gap-2 border-t border-border/50 pt-2 text-[11px]">
            <Badge
              variant="outline"
              className={`px-2 py-0.5 text-[11px] font-medium ${
                permissionStatus.badgesEnabled
                  ? "border-status-success/20 bg-status-success/10 text-status-success"
                  : "border-status-warning/20 bg-status-warning/10 text-status-warning"
              }`}
            >
              Dock badge: {permissionStatus.badgesEnabled ? "On" : "Off"}
            </Badge>
            {!permissionStatus.badgesEnabled ? (
              <span className="text-muted-foreground">
                Enable 'Badge application icon' under System Settings → Notifications → Ferryx.
              </span>
            ) : null}
          </div>
        ) : null}

        {requestNotice ? (
          <Alert role="alert" className="mt-3">
            <AlertDescription className="text-[11px] leading-normal">
              {requestNotice}
            </AlertDescription>
          </Alert>
        ) : null}
      </Card>

      <SettingsGroup
        title="Alerts & Sounds"
        action={
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={resetSettings}
            className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
            title="Reset to defaults"
          >
            <RotateCcw className="size-3" />
            Reset to defaults
          </Button>
        }
      >
        <div className="border-b border-border">
        <SettingRow
          label="Enable Notifications"
          description="Master toggle for all desktop notifications and audio cues."
        >
          <Switch
            aria-label="Enable Notifications"
            checked={settings.enabled}
            onCheckedChange={(checked) => {
              updateSettings({ enabled: checked });
              if (checked && auth === "not-determined") {
                void handleRequestPermission();
              }
            }}
          />
        </SettingRow>

        <SettingRow
          label="Agent Task Complete"
          description="Notify when background agent tasks transition to waiting or done."
        >
          <Switch
            aria-label="Agent Task Complete"
            checked={settings.agentTaskComplete}
            disabled={!settings.enabled}
            onCheckedChange={(checked) =>
              updateSettings({ agentTaskComplete: checked })
            }
          />
        </SettingRow>

        <SettingRow
          label="Pane attention border"
          description="Outline a pane when its agent is blocked or finished and unseen."
        >
          <Switch
            aria-label="Pane attention border"
            checked={settings.attentionFrame}
            disabled={!settings.enabled}
            onCheckedChange={(checked) =>
              updateSettings({ attentionFrame: checked })
            }
          />
        </SettingRow>

        <SettingRow
          label="Terminal Bell"
          description="Notify when background terminal sessions produce a bell signal."
        >
          <Switch
            aria-label="Terminal Bell"
            checked={settings.terminalBell}
            disabled={!settings.enabled}
            onCheckedChange={(checked) =>
              updateSettings({ terminalBell: checked })
            }
          />
        </SettingRow>

        <SettingRow
          label="Notification Sound"
          description="Audio played when a desktop notification is dispatched."
        >
          <div className="flex flex-col gap-2">
            <Select
              value={settings.customSoundId}
              disabled={!settings.enabled}
              onValueChange={(val) => updateSettings({ customSoundId: val })}
            >
              <SelectTrigger
                aria-label="Notification Sound"
                className="h-8 w-[180px] text-[11px]"
              >
                <SelectValue placeholder="Notification sound" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="system">System Default</SelectItem>
                <SelectItem value="none">None (Mute)</SelectItem>
                <SelectItem value="custom">Custom Audio File</SelectItem>
              </SelectContent>
            </Select>
            {settings.customSoundId === "custom" ? (
              <div className="flex items-center gap-2">
                <span className="max-w-[200px] truncate text-[11px] text-muted-foreground">
                  {settings.customSoundPath
                    ? settings.customSoundPath.split(/[/\\]/).pop()
                    : "No file chosen"}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label="Browse custom audio file"
                  onClick={handlePickAudio}
                  disabled={!settings.enabled}
                  className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  Browse...
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handlePreviewSound}
                  disabled={!settings.enabled || !settings.customSoundPath}
                  className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
                >
                  Preview
                </Button>
              </div>
            ) : null}
            {settings.customSoundId === "custom" && !settings.customSoundPath ? (
              <span role="alert" className="text-[11px] text-status-warning">
                Choose an audio file to enable the custom sound.
              </span>
            ) : null}
          </div>
        </SettingRow>

        <SettingRow
          label="Volume"
          description="Adjust the playback volume of notification alerts."
        >
          <div className="flex items-center gap-3">
            <Slider
              aria-label="Volume"
              min={0}
              max={1}
              step={0.05}
              value={[settings.customSoundVolume]}
              disabled={!settings.enabled || settings.customSoundId === "none"}
              onValueChange={(val) => {
                if (val[0] !== undefined) {
                  updateSettings({ customSoundVolume: val[0] });
                }
              }}
              className="w-32"
            />
            <span className="w-8 text-[11px] text-muted-foreground">
              {Math.round(settings.customSoundVolume * 100)}%
            </span>
          </div>
        </SettingRow>

        <SettingRow
          label="Test Notification"
          description="Send a sample notification immediately to verify OS delivery."
        >
          <Button
            type="button"
            onClick={handleTestNotification}
            disabled={!settings.enabled || isTesting}
            size="sm"
            className="no-drag h-7 shrink-0 gap-1.5 px-2.5 text-[11px] font-medium shadow-sm transition-colors"
          >
            {isTesting ? "Sending..." : "Send Test Notification"}
          </Button>
        </SettingRow>

        {testResult ? (
          <Alert
            role="alert"
            variant={testResult.tone === "error" ? "destructive" : "default"}
            className="mt-2"
          >
            <AlertDescription className="text-[11px] leading-normal">
              {testResult.message}
            </AlertDescription>
          </Alert>
        ) : null}
      </div>
      </SettingsGroup>
    </section>
  );
}

export { NotificationsSection as NotificationSettings };
