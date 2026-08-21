import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Bell,
  Brush,
  CheckCircle2,
  ExternalLink,
  FolderGit2,
  Keyboard,
  MonitorCog,
  Radio,
  RotateCcw,
  Settings2,
  TerminalSquare,
} from "lucide-react";

import { useNotificationSettings } from "../lib/notificationSettings";
import { SHORTCUTS, isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import {
  createPairingCode,
  disableRemoteGateway,
  enableRemoteGateway,
  getNotificationPermissionStatus,
  getRemoteStatus,
  openNotificationSystemSettings,
  pickNotificationAudio,
  playNotificationSound,
  probeNotificationDelivery,
  requestNotificationPermission,
  type RemoteGatewayStatus,
} from "../lib/tauri";
import { useTerminalSettings } from "../lib/terminalSettings";
import type { NotificationPermissionStatus } from "../lib/types";

export type SettingsDialogProps = {
  open: boolean;
  onClose: () => void;
};

type SettingsSection = "general" | "terminal" | "shortcuts" | "workspace" | "remote" | "notifications";

const inputClass =
  "h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring";

export function SettingsDialog({ open, onClose }: SettingsDialogProps) {
  const { settings, nativePreferences, updateSettings, refreshNativePreferences } = useTerminalSettings();
  const [section, setSection] = useState<SettingsSection>("general");
  const isMac = isMacShortcutPlatform();

  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [onClose, open]);

  if (!open) return null;

  const hasLocalTerminalOverride = settings.fontFamilySource === "local" || settings.macosOptionAsAltSource === "local";
  const terminalSource = hasLocalTerminalOverride
    ? "Local override"
    : nativePreferences.source === "ghostty" && nativePreferences.status === "imported"
      ? "Ghostty · Imported"
      : nativePreferences.status === "malformed"
        ? "Ghostty · Malformed · Safe defaults"
        : "Built-in defaults";

  return (
    <div
      role="dialog"
      aria-label="Settings"
      aria-modal={false}
      className="fixed inset-0 z-50 flex overflow-hidden bg-background text-foreground"
    >
      <aside data-testid="settings-nav" className="flex w-[280px] shrink-0 flex-col border-r border-border bg-card">
        <div data-tauri-drag-region className="drag-region h-titlebar shrink-0" />
        <div className="px-3 pb-3 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="no-drag mb-5 flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <ArrowLeft className="size-3.5" />
            Back to app
          </button>
          <div className="mb-2 px-2 text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/55">Settings</div>
          <nav aria-label="Settings sections" className="space-y-0.5">
            <NavButton active={section === "general"} icon={<Settings2 />} label="General" onClick={() => setSection("general")} />
            <NavButton active={section === "terminal"} icon={<TerminalSquare />} label="Terminal" onClick={() => setSection("terminal")} />
            <NavButton active={section === "shortcuts"} icon={<Keyboard />} label="Keyboard Shortcuts" onClick={() => setSection("shortcuts")} />
            <NavButton active={section === "workspace"} icon={<FolderGit2 />} label="Workspace" onClick={() => setSection("workspace")} />
            <NavButton active={section === "notifications"} icon={<Bell />} label="Notifications" onClick={() => setSection("notifications")} />
            <NavButton active={section === "remote"} icon={<Radio />} label="Remote Access" onClick={() => setSection("remote")} />
          </nav>
        </div>
        <div className="mt-auto border-t border-border px-5 py-3 text-[10px] text-muted-foreground/55">rorca · local desktop</div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto bg-background scrollbar-sleek">
        <div data-tauri-drag-region className="drag-region h-titlebar shrink-0 border-b border-border/70" />
        <div className="mx-auto w-full max-w-[896px] px-8 pb-16 pt-10">
          {section === "general" ? <GeneralSettings /> : null}
          {section === "terminal" ? (
            <TerminalSettings
              fontFamily={settings.fontFamily}
              fontSize={settings.fontSize}
              scrollback={settings.scrollback}
              macosOptionAsAlt={settings.macosOptionAsAlt}
              source={terminalSource}
              sourcePath={nativePreferences.sourcePath}
              onFontFamily={(fontFamily) => updateSettings({ fontFamily })}
              onFontSize={(fontSize) => updateSettings({ fontSize })}
              onScrollback={(scrollback) => updateSettings({ scrollback })}
              onOptionAsAlt={(macosOptionAsAlt) => updateSettings({ macosOptionAsAlt })}
              onUseImported={() => {
                updateSettings({ fontFamily: null, macosOptionAsAlt: null });
                void refreshNativePreferences();
              }}
            />
          ) : null}
          {section === "shortcuts" ? <ShortcutSettings isMac={isMac} /> : null}
          {section === "workspace" ? <WorkspaceSettings /> : null}
          {section === "notifications" ? <NotificationSettings /> : null}
          {section === "remote" ? <RemoteAccessSettings /> : null}
        </div>
      </main>
    </div>
  );
}

function NavButton({
  active,
  icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`no-drag flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] transition-colors ${
        active ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground"
      }`}
    >
      <span className="[&>svg]:size-3.5">{icon}</span>
      {label}
    </button>
  );
}

function SettingsHeading({ icon, title, description }: { icon: React.ReactNode; title: string; description: string }) {
  return (
    <header className="mb-8 border-b border-border pb-5">
      <div className="mb-2 flex items-center gap-2 text-[15px] font-semibold">
        <span className="text-muted-foreground [&>svg]:size-4">{icon}</span>
        {title}
      </div>
      <p className="max-w-2xl text-[12px] leading-5 text-muted-foreground">{description}</p>
    </header>
  );
}

function GeneralSettings() {
  return (
    <section aria-labelledby="settings-general-heading">
      <SettingsHeading
        icon={<MonitorCog />}
        title="General"
        description="Desktop shell preferences that follow the compact original Orca layout."
      />
      <h2 id="settings-general-heading" className="mb-3 flex items-center gap-2 text-[12px] font-semibold">
        <Brush className="size-3.5 text-muted-foreground" />
        Appearance
      </h2>
      <div className="border-y border-border">
        <SettingRow label="Color scheme" description="rorca uses the native charcoal desktop palette.">
          <span className="text-[11px] text-muted-foreground">Charcoal</span>
        </SettingRow>
        <SettingRow label="Density" description="Navigation, tabs, and chrome stay compact at desktop scale.">
          <span className="text-[11px] text-muted-foreground">Compact</span>
        </SettingRow>
      </div>
    </section>
  );
}

function TerminalSettings({
  fontFamily,
  fontSize,
  scrollback,
  macosOptionAsAlt,
  source,
  sourcePath,
  onFontFamily,
  onFontSize,
  onScrollback,
  onOptionAsAlt,
  onUseImported,
}: {
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  macosOptionAsAlt: boolean;
  source: string;
  sourcePath: string | null;
  onFontFamily: (fontFamily: string) => void;
  onFontSize: (fontSize: number) => void;
  onScrollback: (scrollback: number) => void;
  onOptionAsAlt: (enabled: boolean) => void;
  onUseImported: () => void;
}) {
  return (
    <section aria-labelledby="settings-terminal-heading">
      <SettingsHeading
        icon={<TerminalSquare />}
        title="Terminal"
        description="Ghostty preferences are imported by the native runtime. Explicit values set here take precedence locally."
      />
      <div className="mb-5 flex items-start justify-between gap-5 border-y border-border py-3">
        <div className="min-w-0">
          <h2 id="settings-terminal-heading" className="text-[12px] font-semibold">Effective preferences</h2>
          <div className="mt-1 text-[11px] text-muted-foreground">{source}</div>
          {sourcePath ? <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground/65">{sourcePath}</div> : null}
        </div>
        <button
          type="button"
          onClick={onUseImported}
          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Use imported
        </button>
      </div>

      <div className="space-y-5">
        <div>
          <label htmlFor="terminal-font-family" className="mb-1.5 block text-[11px] font-medium">Font family</label>
          <input
            id="terminal-font-family"
            aria-label="Font family"
            value={fontFamily}
            onChange={(event) => onFontFamily(event.target.value)}
            className={inputClass}
          />
          <p className="mt-1 text-[10px] text-muted-foreground">Leave the local override reset to follow Ghostty.</p>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label htmlFor="terminal-font-size" className="mb-1.5 block text-[11px] font-medium">Font size</label>
            <input
              id="terminal-font-size"
              aria-label="Font size"
              type="number"
              min={10}
              max={24}
              value={fontSize}
              onChange={(event) => onFontSize(Number(event.target.value))}
              className={inputClass}
            />
          </div>
          <div>
            <label htmlFor="terminal-scrollback" className="mb-1.5 block text-[11px] font-medium">Scrollback</label>
            <input
              id="terminal-scrollback"
              aria-label="Scrollback"
              type="number"
              min={1000}
              max={100000}
              step={1000}
              value={scrollback}
              onChange={(event) => onScrollback(Number(event.target.value))}
              className={inputClass}
            />
          </div>
        </div>
        <label className="flex items-center justify-between gap-4 border-y border-border py-3 text-[11px]">
          <div>
            <div className="font-medium text-foreground">macOS Option as Alt</div>
            <div className="mt-0.5 text-[10px] text-muted-foreground">Maps the Option key to xterm Meta/Alt behavior.</div>
          </div>
          <input
            aria-label="macOS Option as Alt"
            type="checkbox"
            checked={macosOptionAsAlt}
            onChange={(event) => onOptionAsAlt(event.target.checked)}
            className="size-4 accent-foreground"
          />
        </label>
      </div>
    </section>
  );
}

function ShortcutSettings({ isMac }: { isMac: boolean }) {
  return (
    <section aria-labelledby="settings-shortcuts-heading">
      <SettingsHeading
        icon={<Keyboard />}
        title="Keyboard Shortcuts"
        description="Only registered modifier chords are intercepted by the shell. Ordinary terminal typing and Ctrl-C remain with xterm."
      />
      <h2 id="settings-shortcuts-heading" className="sr-only">Keyboard Shortcuts</h2>
      <div className="border-y border-border">
        {SHORTCUTS.map((shortcut) => (
          <div key={shortcut.id} className="flex min-h-10 items-center justify-between gap-4 border-b border-border px-1 last:border-b-0">
            <div className="min-w-0 py-2">
              <div className="truncate text-[12px] font-medium text-foreground">{shortcut.title}</div>
              <div className="text-[10px] text-muted-foreground">{shortcut.group}</div>
            </div>
            <kbd className="shrink-0 rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground">
              {shortcutLabel(shortcut.id, isMac)}
            </kbd>
          </div>
        ))}
      </div>
    </section>
  );
}

function WorkspaceSettings() {
  return (
    <section aria-labelledby="settings-workspace-heading">
      <SettingsHeading
        icon={<FolderGit2 />}
        title="Workspace"
        description="Projects are registered with the native repository registry; worktrees keep the existing safety checks before deletion."
      />
      <h2 id="settings-workspace-heading" className="sr-only">Workspace</h2>
      <div className="border-y border-border">
        <SettingRow label="Projects" description="Local repositories appear in the workspace rail after native registration.">
          <span className="text-[11px] text-muted-foreground">Native registry</span>
        </SettingRow>
        <SettingRow label="Worktree deletion" description="Dirty-state and branch deletion previews remain enforced by the native safety contract.">
          <span className="text-[11px] text-muted-foreground">Protected</span>
        </SettingRow>
      </div>
    </section>
  );
}

export function NotificationSettings() {
  const { settings, updateSettings, resetSettings } = useNotificationSettings();
  const [permissionStatus, setPermissionStatus] = useState<NotificationPermissionStatus | null>(null);
  const [isTesting, setIsTesting] = useState(false);

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
  }, [refreshPermission]);

  const handleRequestPermission = async () => {
    try {
      await requestNotificationPermission();
      await refreshPermission();
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
    try {
      await probeNotificationDelivery(true);
      await handlePreviewSound();
    } catch (err) {
      console.error("Failed to send test notification:", err);
    } finally {
      setIsTesting(false);
    }
  };

  const auth = permissionStatus?.authorization ?? "unknown";
  const isAuthorized = auth === "authorized" || auth === "provisional";
  const isDenied = auth === "denied";
  const isNotDetermined = auth === "not-determined" || auth === "unknown";

  return (
    <section role="region" aria-label="Notifications" className="space-y-6">
      <div className="flex items-center justify-between border-b border-border/70 pb-4">
        <div>
          <h2 className="text-sm font-semibold text-foreground flex items-center gap-2">
            <Bell className="size-4 text-muted-foreground" />
            Notifications
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            Configure desktop alerts, audio feedback, and agent completion triggers.
          </p>
        </div>
        <button
          type="button"
          onClick={() => resetSettings()}
          className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-muted-foreground hover:text-foreground rounded border border-border hover:bg-accent transition-colors"
          title="Reset to defaults"
        >
          <RotateCcw className="size-3" />
          Reset
        </button>
      </div>

      {/* Permission Status */}
      <div className="rounded-md border border-border/70 bg-card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-muted-foreground">OS Permission Status:</span>
            <span
              className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium ${
                isAuthorized
                  ? "bg-emerald-500/10 text-emerald-500 border border-emerald-500/20"
                  : isDenied
                  ? "bg-rose-500/10 text-rose-500 border border-rose-500/20"
                  : "bg-amber-500/10 text-amber-500 border border-amber-500/20"
              }`}
            >
              {isAuthorized ? <CheckCircle2 className="size-3" /> : null}
              {isDenied ? <AlertTriangle className="size-3" /> : null}
              {auth}
            </span>
          </div>
          {isNotDetermined || isDenied ? (
            <button
              type="button"
              onClick={handleRequestPermission}
              className="text-xs text-primary hover:underline"
            >
              Request Permission
            </button>
          ) : null}
          {isDenied ? (
            <button
              type="button"
              onClick={handleOpenSettings}
              className="text-xs text-muted-foreground hover:text-foreground hover:underline flex items-center gap-1"
            >
              Open System Settings
              <ExternalLink className="size-3" />
            </button>
          ) : null}
        </div>
      </div>

      <div className="space-y-1">
        <SettingRow
          title="Enable Notifications"
          description="Master toggle for all desktop notifications and audio cues."
        >
          <label className="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              aria-label="Enable Notifications"
              checked={settings.enabled}
              onChange={(e) => updateSettings({ enabled: e.target.checked })}
              className="sr-only peer"
            />
            <div className="w-9 h-5 bg-muted peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-border after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary" />
          </label>
        </SettingRow>

        <SettingRow
          title="Agent Task Complete"
          description="Notify when background agent tasks transition to waiting or done."
        >
          <label className="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              aria-label="Agent Task Complete"
              checked={settings.agentTaskComplete}
              disabled={!settings.enabled}
              onChange={(e) => updateSettings({ agentTaskComplete: e.target.checked })}
              className="sr-only peer"
            />
            <div className="w-9 h-5 bg-muted peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-border after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary" />
          </label>
        </SettingRow>

        <SettingRow
          title="Terminal Bell"
          description="Notify when background terminal sessions produce a bell signal."
        >
          <label className="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              aria-label="Terminal Bell"
              checked={settings.terminalBell}
              disabled={!settings.enabled}
              onChange={(e) => updateSettings({ terminalBell: e.target.checked })}
              className="sr-only peer"
            />
            <div className="w-9 h-5 bg-muted peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-border after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-primary" />
          </label>
        </SettingRow>

        <SettingRow
          title="Notification Sound"
          description="Audio played when a desktop notification is dispatched."
        >
          <div className="flex flex-col gap-2">
            <select
              aria-label="Notification Sound"
              value={settings.customSoundId}
              disabled={!settings.enabled}
              onChange={(e) => updateSettings({ customSoundId: e.target.value })}
              className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none"
            >
              <option value="system">System Default</option>
              <option value="none">None (Mute)</option>
              <option value="custom">Custom Audio File</option>
            </select>
            {settings.customSoundId === "custom" ? (
              <div className="flex items-center gap-2">
                <span className="text-[11px] text-muted-foreground truncate max-w-[200px]">
                  {settings.customSoundPath ? settings.customSoundPath.split(/[\\/]/).pop() : "No file chosen"}
                </span>
                <button
                  type="button"
                  aria-label="Browse custom audio file"
                  onClick={handlePickAudio}
                  className="px-2 py-1 text-xs border border-border rounded hover:bg-accent"
                >
                  Browse...
                </button>
                <button
                  type="button"
                  onClick={handlePreviewSound}
                  disabled={!settings.customSoundPath}
                  className="px-2 py-1 text-xs border border-border rounded hover:bg-accent"
                >
                  Preview
                </button>
              </div>
            ) : null}
          </div>
        </SettingRow>

        <SettingRow
          title="Volume"
          description="Adjust the playback volume of notification alerts."
        >
          <div className="flex items-center gap-3">
            <input
              type="range"
              aria-label="Volume"
              min="0"
              max="1"
              step="0.05"
              value={settings.customSoundVolume}
              disabled={!settings.enabled || settings.customSoundId === "none"}
              onChange={(e) => updateSettings({ customSoundVolume: parseFloat(e.target.value) })}
              className="w-32 accent-primary"
            />
            <span className="text-xs text-muted-foreground w-8">
              {Math.round(settings.customSoundVolume * 100)}%
            </span>
          </div>
        </SettingRow>

        <SettingRow
          title="Test Notification"
          description="Send a sample notification immediately to verify OS delivery."
        >
          <button
            type="button"
            onClick={handleTestNotification}
            disabled={isTesting}
            className="px-3 py-1.5 text-xs font-medium bg-primary text-primary-foreground rounded hover:bg-primary/90 transition-colors"
          >
            {isTesting ? "Sending..." : "Send Test Notification"}
          </button>
        </SettingRow>
      </div>
    </section>
  );
}

function RemoteAccessSettings() {
  const [status, setStatus] = useState<RemoteGatewayStatus | null>(null);
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  const refreshStatus = async () => {
    try {
      const s = await getRemoteStatus();
      setStatus(s);
    } catch {
      // ignore
    }
  };

  useEffect(() => {
    refreshStatus();
  }, []);

  const handleToggle = async (enabled: boolean) => {
    setLoading(true);
    try {
      if (!enabled) {
        const s = await disableRemoteGateway();
        setStatus(s);
        setPairingCode(null);
        setQrDataUrl(null);
      } else {
        const s = await enableRemoteGateway({ mode: "localNetwork" });
        setStatus(s);
        // Auto-generate pairing PIN and QR upon enable
        generatePairing(s);
      }
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  };

  const generatePairing = async (currentStatus?: RemoteGatewayStatus | null) => {
    const s = currentStatus ?? status;
    try {
      const res = await createPairingCode("control");
      setPairingCode(res.code);

      // Determine host for QR URL: Tailscale domain preferred, fallback to local IP, then localhost
      const port = s?.port ?? 43821;
      let host = s?.localIp ? `${s.localIp}:${port}` : `localhost:${port}`;
      let proto = "http";
      if (s?.tailscale?.running && s?.tailscale?.selfDns) {
        host = s.tailscale.selfDns;
        proto = "https";
      }

      const connectUrl = `${proto}://${host}/#pair=${res.code}`;
      const QRCode = (await import("qrcode")).default;
      const dataUrl = await QRCode.toDataURL(connectUrl, {
        width: 180,
        margin: 1,
        color: { dark: "#ffffff", light: "#171717" },
      });
      setQrDataUrl(dataUrl);
    } catch {
      // ignore
    }
  };

  const port = status?.port ?? 43821;
  const localUrl = status?.localIp ? `http://${status.localIp}:${port}` : `http://localhost:${port}`;
  const tailscaleUrl = status?.tailscale?.running && status?.tailscale?.selfDns ? `https://${status.tailscale.selfDns}` : null;

  return (
    <section aria-labelledby="settings-remote-heading">
      <SettingsHeading
        icon={<Radio />}
        title="Remote Access"
        description="Access desktop terminal sessions directly from your mobile browser or other devices."
      />
      <h2 id="settings-remote-heading" className="sr-only">Remote Access</h2>
      <div className="border-y border-border">
        <SettingRow
          label="Remote Access"
          description="Enable access to live terminal sessions over your local network and Tailscale."
        >
          <div className="flex items-center gap-2">
            <button
              onClick={() => handleToggle(!status?.enabled)}
              disabled={loading}
              className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                status?.enabled
                  ? "bg-emerald-600 text-white hover:bg-emerald-500"
                  : "bg-muted text-muted-foreground hover:bg-muted/80"
              }`}
            >
              {status?.enabled ? "Active" : "Disabled"}
            </button>
          </div>
        </SettingRow>

        {status?.enabled && (
          <>
            <SettingRow
              label="Instant QR Connect"
              description="Scan this QR code with your phone camera to pair and connect immediately without typing."
            >
              <div className="flex flex-col items-center gap-2 bg-neutral-900 border border-neutral-800 p-3 rounded-lg">
                {qrDataUrl ? (
                  <img src={qrDataUrl} alt="Pairing QR Code" className="w-[160px] h-[160px] rounded" />
                ) : (
                  <div className="w-[160px] h-[160px] flex items-center justify-center text-xs text-muted-foreground">
                    Generating...
                  </div>
                )}
                {pairingCode && (
                  <div className="flex items-center justify-between w-full px-1">
                    <span className="font-mono text-xs text-emerald-400 font-semibold">
                      PIN: {pairingCode}
                    </span>
                    <button
                      onClick={() => generatePairing()}
                      className="text-[10px] text-muted-foreground hover:text-foreground underline"
                    >
                      New Code
                    </button>
                  </div>
                )}
              </div>
            </SettingRow>

            <SettingRow
              label="Connection URLs"
              description="Direct browser address for mobile and other devices on your network."
            >
              <div className="space-y-1 text-right">
                <div className="flex items-center justify-end gap-1.5 font-mono text-[11px] text-foreground">
                  <span>{localUrl}</span>
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText(localUrl);
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1500);
                    }}
                    className="text-[10px] px-1.5 py-0.5 bg-muted hover:bg-muted/80 rounded"
                  >
                    {copied ? "Copied" : "Copy"}
                  </button>
                </div>
                {tailscaleUrl && (
                  <div className="font-mono text-[10px] text-muted-foreground">
                    Tailscale: {tailscaleUrl}
                  </div>
                )}
              </div>
            </SettingRow>
          </>
        )}
      </div>
    </section>
  );
}

function SettingRow({
  label,
  title,
  description,
  children,
}: {
  label?: string;
  title?: string;
  description?: string;
  children: React.ReactNode;
}) {
  const heading = title ?? label ?? "";
  return (
    <div className="flex items-start justify-between gap-4 py-3 border-b border-border/40 last:border-0">
      <div className="space-y-0.5 max-w-[480px]">
        <div className="text-xs font-medium text-foreground">{heading}</div>
        {description ? <p className="text-[11px] text-muted-foreground">{description}</p> : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}