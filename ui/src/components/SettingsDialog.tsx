import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Bell,
  Brush,
  CheckCircle2,
  ExternalLink,
  FolderGit2,
  Globe,
  Keyboard,
  MonitorCog,
  Palette,
  Plus,
  Radio,
  RotateCcw,
  Search,
  Settings2,
  TerminalSquare,
  Trash2,
  Zap,
} from "lucide-react";

import { useNotificationSettings } from "../lib/notificationSettings";
import { SHORTCUTS, isMacShortcutPlatform, shortcutAliasesLabels, shortcutLabel } from "../lib/shortcuts";
import {
  createPairingCode,
  disableRemoteGateway,
  enableRemoteGateway,
  getNotificationPermissionStatus,
  getRemoteStatus,
  getTailscaleStatus,
  listRemoteDevices,
  openNotificationSystemSettings,
  pickNotificationAudio,
  playNotificationSound,
  probeNotificationDelivery,
  requestNotificationPermission,
  revokeRemoteDevice,
  type DeviceInfo,
  type RemoteGatewayStatus,
  type TailscaleStatus,
} from "../lib/tauri";
import { focusBrowser, listBrowsers, setBrowserZoom } from "../lib/browserTauri";
import { useTerminalSettings } from "../lib/terminalSettings";
import type { BrowserSessionSummary, NotificationPermissionStatus } from "../lib/types";

export type SettingsDialogProps = {
  open: boolean;
  onClose: () => void;
};

type SettingsSection =
  | "general"
  | "appearance"
  | "terminal"
  | "shortcuts"
  | "workspace"
  | "quickCommands"
  | "browser"
  | "notifications"
  | "remote";

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
            <NavButton active={section === "appearance"} icon={<Palette />} label="Appearance" onClick={() => setSection("appearance")} />
            <NavButton active={section === "terminal"} icon={<TerminalSquare />} label="Terminal" onClick={() => setSection("terminal")} />
            <NavButton active={section === "shortcuts"} icon={<Keyboard />} label="Keyboard Shortcuts" onClick={() => setSection("shortcuts")} />
            <NavButton active={section === "workspace"} icon={<FolderGit2 />} label="Workspace" onClick={() => setSection("workspace")} />
            <NavButton active={section === "quickCommands"} icon={<Zap />} label="Quick Commands" onClick={() => setSection("quickCommands")} />
            <NavButton active={section === "browser"} icon={<Globe />} label="Browser" onClick={() => setSection("browser")} />
            <NavButton active={section === "notifications"} icon={<Bell />} label="Notifications" onClick={() => setSection("notifications")} />
            <NavButton active={section === "remote"} icon={<Radio />} label="Remote Access" onClick={() => setSection("remote")} />
          </nav>
        </div>
        <div className="mt-auto border-t border-border px-5 py-3 text-[10px] text-muted-foreground/55">Ferryx · local desktop</div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto bg-background scrollbar-sleek">
        <div data-tauri-drag-region className="drag-region h-titlebar shrink-0 border-b border-border/70" />
        <div className="mx-auto w-full max-w-[896px] px-8 pb-16 pt-10">
          {section === "general" ? <GeneralSettings /> : null}
          {section === "appearance" ? <AppearanceSettings /> : null}
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
          {section === "quickCommands" ? <QuickCommandsSettings /> : null}
          {section === "browser" ? <BrowserSettings /> : null}
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
        description="Desktop shell preferences that follow the compact Ferryx desktop layout."
      />
      <h2 id="settings-general-heading" className="mb-3 flex items-center gap-2 text-[12px] font-semibold">
        <Brush className="size-3.5 text-muted-foreground" />
        Appearance
      </h2>
      <div className="border-y border-border">
        <SettingRow label="Color scheme" description="Ferryx uses the native charcoal desktop palette.">
          <span className="text-[11px] text-muted-foreground">Charcoal</span>
        </SettingRow>
        <SettingRow label="Density" description="Navigation, tabs, and chrome stay compact at desktop scale.">
          <span className="text-[11px] text-muted-foreground">Compact</span>
        </SettingRow>
      </div>
    </section>
  );
}

export const APPEARANCE_SETTINGS_STORAGE_KEY = "ferryx.settings.appearance";

export type AppearanceSettingsState = {
  theme: "charcoal" | "dark" | "light" | "system";
  accentColor: "default" | "blue" | "emerald" | "purple" | "amber" | "rose";
  density: "compact" | "comfortable";
};

const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettingsState = {
  theme: "charcoal",
  accentColor: "default",
  density: "compact",
};

export function AppearanceSettings() {
  const [settings, setSettings] = useState<AppearanceSettingsState>(() => {
    try {
      const raw = localStorage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY);
      if (raw) return { ...DEFAULT_APPEARANCE_SETTINGS, ...JSON.parse(raw) };
    } catch {
      // ignore
    }
    return DEFAULT_APPEARANCE_SETTINGS;
  });

  const update = (patch: Partial<AppearanceSettingsState>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      try {
        localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify(next));
      } catch {
        // ignore
      }
      return next;
    });
  };

  const reset = () => {
    setSettings(DEFAULT_APPEARANCE_SETTINGS);
    try {
      localStorage.removeItem(APPEARANCE_SETTINGS_STORAGE_KEY);
    } catch {
      // ignore
    }
  };

  return (
    <section aria-labelledby="settings-appearance-heading">
      <SettingsHeading
        icon={<Palette />}
        title="Appearance"
        description="Customize the desktop theme palette, accent colors, and interface scale density."
      />
      <h2 id="settings-appearance-heading" className="sr-only">Appearance</h2>
      <div className="mb-5 flex items-center justify-between border-b border-border pb-3">
        <h3 className="text-[12px] font-semibold">Display & Styling</h3>
        <button
          type="button"
          onClick={reset}
          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Reset to defaults
        </button>
      </div>

      <div className="border-y border-border">
        <SettingRow label="Theme mode" description="Select the base color theme for the desktop interface.">
          <select
            id="appearance-theme-mode"
            aria-label="Theme mode"
            value={settings.theme}
            onChange={(e) => update({ theme: e.target.value as AppearanceSettingsState["theme"] })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring"
          >
            <option value="charcoal">Charcoal (Default)</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
            <option value="system">System</option>
          </select>
        </SettingRow>

        <SettingRow label="Accent color" description="Accent highlight color used for active badges, selection rings, and indicators.">
          <select
            id="appearance-accent-color"
            aria-label="Accent color"
            value={settings.accentColor}
            onChange={(e) => update({ accentColor: e.target.value as AppearanceSettingsState["accentColor"] })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring"
          >
            <option value="default">Slate (Default)</option>
            <option value="blue">Ocean Blue</option>
            <option value="emerald">Emerald</option>
            <option value="purple">Violet</option>
            <option value="amber">Amber</option>
            <option value="rose">Rose</option>
          </select>
        </SettingRow>

        <SettingRow label="Interface density" description="Adjust spacing and padding across tabs, sidebars, and dialog chrome.">
          <select
            id="appearance-density"
            aria-label="Interface density"
            value={settings.density}
            onChange={(e) => update({ density: e.target.value as AppearanceSettingsState["density"] })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring"
          >
            <option value="compact">Compact (Default)</option>
            <option value="comfortable">Comfortable</option>
          </select>
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
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedGroup, setSelectedGroup] = useState<string>("all");

  const groups = ["all", "Tabs", "Workspaces", "Terminal Panes", "Global", "View"];
  const normalizedQuery = searchQuery.trim().toLowerCase();

  const filteredShortcuts = SHORTCUTS.filter((shortcut) => {
    if (selectedGroup !== "all" && shortcut.group !== selectedGroup) return false;
    if (!normalizedQuery) return true;
    return (
      shortcut.title.toLowerCase().includes(normalizedQuery) ||
      shortcut.group.toLowerCase().includes(normalizedQuery) ||
      shortcut.id.toLowerCase().includes(normalizedQuery)
    );
  });

  return (
    <section aria-labelledby="settings-shortcuts-heading">
      <SettingsHeading
        icon={<Keyboard />}
        title="Keyboard Shortcuts"
        description="Only registered modifier chords are intercepted by the shell. Ordinary terminal typing and Ctrl-C remain with xterm."
      />
      <h2 id="settings-shortcuts-heading" className="sr-only">Keyboard Shortcuts</h2>

      <div className="mb-3 space-y-2">
        <div className="relative flex items-center">
          <Search className="pointer-events-none absolute left-2.5 size-3.5 text-muted-foreground" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search shortcuts (e.g. tab, split, zoom, workspace)..."
            aria-label="Search keyboard shortcuts"
            className="h-8 w-full rounded-md border border-input bg-background pl-8 pr-3 text-xs text-foreground placeholder:text-muted-foreground outline-none transition-colors focus:border-ring"
          />
        </div>
        <div className="flex flex-wrap gap-1">
          {groups.map((group) => (
            <button
              key={group}
              type="button"
              onClick={() => setSelectedGroup(group)}
              className={`rounded px-2 py-0.5 text-[11px] font-medium transition-colors ${
                selectedGroup === group
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground"
              }`}
            >
              {group === "all" ? "All" : group}
            </button>
          ))}
        </div>
      </div>

      <div className="border-y border-border">
        {filteredShortcuts.length > 0 ? (
          filteredShortcuts.map((shortcut) => {
            const aliases = shortcutAliasesLabels(shortcut.id, isMac);
            return (
              <div key={shortcut.id} className="flex min-h-10 items-center justify-between gap-4 border-b border-border px-1 last:border-b-0">
                <div className="min-w-0 py-2">
                  <div className="truncate text-[12px] font-medium text-foreground">{shortcut.title}</div>
                  <div className="text-[10px] text-muted-foreground">{shortcut.group}</div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <kbd className="rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground">
                    {shortcutLabel(shortcut.id, isMac)}
                  </kbd>
                  {aliases.map((alias) => (
                    <kbd key={alias} className="rounded border border-border/60 bg-muted/40 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                      {alias}
                    </kbd>
                  ))}
                </div>
              </div>
            );
          })
        ) : (
          <div className="py-8 text-center text-xs text-muted-foreground">
            No keyboard shortcuts matching &quot;{searchQuery}&quot;.
          </div>
        )}
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

export const QUICK_COMMANDS_STORAGE_KEY = "ferryx.settings.quickCommands";

export type QuickCommandItem = {
  id: string;
  label: string;
  command: string;
};

export type QuickCommandsState = {
  enabled: boolean;
  commands: QuickCommandItem[];
};

const DEFAULT_QUICK_COMMANDS: QuickCommandsState = {
  enabled: true,
  commands: [
    { id: "1", label: "Git Status", command: "git status" },
    { id: "2", label: "Git Diff", command: "git diff" },
    { id: "3", label: "List Files", command: "ls -la" },
  ],
};

export function QuickCommandsSettings() {
  const [state, setState] = useState<QuickCommandsState>(() => {
    try {
      const raw = localStorage.getItem(QUICK_COMMANDS_STORAGE_KEY);
      if (raw) return { ...DEFAULT_QUICK_COMMANDS, ...JSON.parse(raw) };
    } catch {
      // ignore
    }
    return DEFAULT_QUICK_COMMANDS;
  });

  const [newLabel, setNewLabel] = useState("");
  const [newCommand, setNewCommand] = useState("");

  const save = (next: QuickCommandsState) => {
    setState(next);
    try {
      localStorage.setItem(QUICK_COMMANDS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // ignore
    }
  };

  const handleAdd = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!newLabel.trim() || !newCommand.trim()) return;
    const newItem: QuickCommandItem = {
      id: Date.now().toString(),
      label: newLabel.trim(),
      command: newCommand.trim(),
    };
    save({
      ...state,
      commands: [...state.commands, newItem],
    });
    setNewLabel("");
    setNewCommand("");
  };

  const handleDelete = (id: string) => {
    save({
      ...state,
      commands: state.commands.filter((c) => c.id !== id),
    });
  };

  const handleToggle = (enabled: boolean) => {
    save({
      ...state,
      enabled,
    });
  };

  return (
    <section aria-labelledby="settings-quick-commands-heading">
      <SettingsHeading
        icon={<Zap />}
        title="Quick Commands"
        description="Configure one-click terminal commands and quick action macros accessible from the terminal dock."
      />
      <h2 id="settings-quick-commands-heading" className="sr-only">Quick Commands</h2>
      <div className="border-y border-border">
        <SettingRow
          label="Enable quick commands"
          description="Show the quick command palette dock across active terminal panes."
        >
          <input
            id="quick-commands-enabled"
            aria-label="Enable quick commands"
            type="checkbox"
            checked={state.enabled}
            onChange={(e) => handleToggle(e.target.checked)}
            className="size-4 accent-foreground"
          />
        </SettingRow>
      </div>

      <div className="mt-8 space-y-4">
        <h3 className="text-[12px] font-semibold">Custom Commands</h3>
        <form onSubmit={handleAdd} className="flex items-end gap-3 rounded-lg border border-border bg-card p-3">
          <div className="flex-1 space-y-1">
            <label htmlFor="qc-new-label" className="text-[10px] font-medium text-muted-foreground">Label</label>
            <input
              id="qc-new-label"
              aria-label="Command label"
              placeholder="e.g. Git Status"
              value={newLabel}
              onChange={(e) => setNewLabel(e.target.value)}
              className={inputClass}
            />
          </div>
          <div className="flex-1 space-y-1">
            <label htmlFor="qc-new-cmd" className="text-[10px] font-medium text-muted-foreground">Command</label>
            <input
              id="qc-new-cmd"
              aria-label="Command string"
              placeholder="e.g. git status"
              value={newCommand}
              onChange={(e) => setNewCommand(e.target.value)}
              className={inputClass}
            />
          </div>
          <button
            type="submit"
            disabled={!newLabel.trim() || !newCommand.trim()}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border px-3 text-xs font-medium text-foreground hover:bg-accent disabled:opacity-40"
          >
            <Plus className="size-3.5" />
            Add command
          </button>
        </form>

        <div className="divide-y divide-border rounded-lg border border-border bg-card">
          {state.commands.length === 0 ? (
            <div className="p-4 text-center text-xs text-muted-foreground">No quick commands configured.</div>
          ) : (
            state.commands.map((cmd) => (
              <div key={cmd.id} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0">
                  <div className="text-xs font-medium text-foreground">{cmd.label}</div>
                  <div className="font-mono text-[11px] text-muted-foreground">{cmd.command}</div>
                </div>
                <button
                  type="button"
                  aria-label={`Delete command ${cmd.label}`}
                  onClick={() => handleDelete(cmd.id)}
                  className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-destructive"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            ))
          )}
        </div>
      </div>
    </section>
  );
}

export const BROWSER_SETTINGS_STORAGE_KEY = "ferryx.settings.browser";

export type BrowserSettingsState = {
  searchEngine: "duckduckgo" | "google" | "bing" | "brave";
  defaultZoom: number;
  restoreTabsOnLaunch: boolean;
};

const DEFAULT_BROWSER_SETTINGS: BrowserSettingsState = {
  searchEngine: "duckduckgo",
  defaultZoom: 100,
  restoreTabsOnLaunch: false,
};

export function BrowserSettings() {
  const [settings, setSettings] = useState<BrowserSettingsState>(() => {
    try {
      const raw = localStorage.getItem(BROWSER_SETTINGS_STORAGE_KEY);
      if (raw) return { ...DEFAULT_BROWSER_SETTINGS, ...JSON.parse(raw) };
    } catch {
      // ignore
    }
    return DEFAULT_BROWSER_SETTINGS;
  });
  const [activeBrowsers, setActiveBrowsers] = useState<BrowserSessionSummary[]>([]);

  const refreshBrowsers = useCallback(async () => {
    try {
      const list = await listBrowsers();
      setActiveBrowsers(list);
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    void refreshBrowsers();
  }, [refreshBrowsers]);

  const update = async (patch: Partial<BrowserSettingsState>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      try {
        localStorage.setItem(BROWSER_SETTINGS_STORAGE_KEY, JSON.stringify(next));
      } catch {
        // ignore
      }
      return next;
    });

    if (patch.defaultZoom !== undefined) {
      const zoomFactor = patch.defaultZoom / 100;
      try {
        const list = await listBrowsers();
        setActiveBrowsers(list);
        await Promise.all(list.map((b) => setBrowserZoom(b.browserId, zoomFactor)));
      } catch {
        // ignore
      }
    }
  };

  const reset = () => {
    setSettings(DEFAULT_BROWSER_SETTINGS);
    try {
      localStorage.removeItem(BROWSER_SETTINGS_STORAGE_KEY);
    } catch {
      // ignore
    }
  };

  const handleFocus = async (browserId: string) => {
    try {
      await focusBrowser(browserId);
    } catch {
      // ignore
    }
  };

  return (
    <section aria-labelledby="settings-browser-heading">
      <SettingsHeading
        icon={<Globe />}
        title="Browser"
        description="Configure embedded browser search engine, default zoom level, and web navigation."
      />
      <h2 id="settings-browser-heading" className="sr-only">Browser</h2>
      <div className="mb-5 flex items-center justify-between border-b border-border pb-3">
        <h3 className="text-[12px] font-semibold">Web & Navigation</h3>
        <button
          type="button"
          onClick={reset}
          className="no-drag flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Reset to defaults
        </button>
      </div>

      <div className="border-y border-border">
        <SettingRow
          label="Default search engine"
          description="Default search provider used when queries are entered directly in the address bar."
        >
          <select
            id="browser-search-engine"
            aria-label="Default search engine"
            value={settings.searchEngine}
            onChange={(e) => void update({ searchEngine: e.target.value as BrowserSettingsState["searchEngine"] })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring"
          >
            <option value="duckduckgo">DuckDuckGo</option>
            <option value="google">Google</option>
            <option value="bing">Bing</option>
            <option value="brave">Brave</option>
          </select>
        </SettingRow>

        <SettingRow
          label="Default zoom level"
          description="Default webview content scaling applied to newly opened browser tabs."
        >
          <select
            id="browser-default-zoom"
            aria-label="Default zoom level"
            value={settings.defaultZoom.toString()}
            onChange={(e) => void update({ defaultZoom: Number(e.target.value) })}
            className="h-8 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors focus:border-ring"
          >
            <option value="80">80%</option>
            <option value="90">90%</option>
            <option value="100">100%</option>
            <option value="110">110%</option>
            <option value="125">125%</option>
            <option value="150">150%</option>
          </select>
        </SettingRow>

        <SettingRow
          label="Restore tabs on launch"
          description="Re-open previously active web tabs when relaunching the workspace."
        >
          <input
            id="browser-restore-tabs"
            aria-label="Restore tabs on launch"
            type="checkbox"
            checked={settings.restoreTabsOnLaunch}
            onChange={(e) => void update({ restoreTabsOnLaunch: e.target.checked })}
            className="size-4 accent-foreground"
          />
        </SettingRow>
      </div>

      <div className="mt-8 space-y-3">
        <h3 className="text-[12px] font-semibold">Active Browser Tabs</h3>
        <div className="divide-y divide-border rounded-lg border border-border bg-card">
          {activeBrowsers.length === 0 ? (
            <div className="p-4 text-center text-xs text-muted-foreground">No active browser tabs open.</div>
          ) : (
            activeBrowsers.map((b) => (
              <div key={b.browserId} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="text-xs font-medium text-foreground">{b.title || b.url || b.browserId}</div>
                  <div className="truncate font-mono text-[10px] text-muted-foreground">{b.url}</div>
                </div>
                <button
                  type="button"
                  aria-label={`Focus browser tab ${b.title || b.browserId}`}
                  onClick={() => void handleFocus(b.browserId)}
                  className="h-7 rounded border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
                >
                  Focus
                </button>
              </div>
            ))
          )}
        </div>
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
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [tailscaleStatus, setTailscaleStatus] = useState<TailscaleStatus | null>(null);
  const [confirmRevokeId, setConfirmRevokeId] = useState<string | null>(null);
  const [pairingCode, setPairingCode] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const [s, devList, ts] = await Promise.all([
        getRemoteStatus().catch(() => null),
        listRemoteDevices().catch(() => []),
        getTailscaleStatus().catch(() => null),
      ]);
      if (s) setStatus(s);
      setDevices(devList);
      setTailscaleStatus(ts);
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const handleToggle = async (enabled: boolean) => {
    setLoading(true);
    try {
      if (!enabled) {
        const s = await disableRemoteGateway();
        setStatus(s);
        setPairingCode(null);
        setQrDataUrl(null);
        await refreshStatus();
      } else {
        const s = await enableRemoteGateway({ mode: "localNetwork" });
        setStatus(s);
        await refreshStatus();
        // Auto-generate pairing PIN and QR upon enable
        generatePairing(s);
      }
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  };

  const handleRevoke = async (deviceId: string) => {
    try {
      await revokeRemoteDevice(deviceId);
      setConfirmRevokeId(null);
      await refreshStatus();
    } catch {
      // ignore
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

        <SettingRow
          label="Tailscale Status"
          description="Mesh network status for secure peer-to-peer remote terminal connections."
        >
          <div className="text-right text-xs">
            {tailscaleStatus?.running ? (
              <span className="inline-flex items-center gap-1 font-medium text-emerald-500">
                <CheckCircle2 className="size-3.5" />
                Running {tailscaleStatus.tailnetName ? `(${tailscaleStatus.tailnetName})` : ""}
              </span>
            ) : tailscaleStatus?.installed ? (
              <span className="text-amber-500 font-medium">Installed (Disconnected)</span>
            ) : (
              <span className="text-muted-foreground">Not installed</span>
            )}
            {tailscaleStatus?.selfDns ? (
              <div className="font-mono text-[10px] text-muted-foreground">{tailscaleStatus.selfDns}</div>
            ) : null}
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

      <div className="mt-8 space-y-3">
        <h3 className="text-[12px] font-semibold">Paired Devices</h3>
        <div className="divide-y divide-border rounded-lg border border-border bg-card">
          {devices.length === 0 ? (
            <div className="p-4 text-center text-xs text-muted-foreground">No paired devices.</div>
          ) : (
            devices.map((dev) => (
              <div key={dev.id} className="flex items-center justify-between gap-4 p-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-foreground">{dev.name || dev.id}</span>
                    <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] uppercase font-mono text-muted-foreground">
                      {dev.permission}
                    </span>
                    {dev.revoked ? (
                      <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-500">
                        Revoked
                      </span>
                    ) : null}
                  </div>
                  <div className="text-[10px] text-muted-foreground">
                    {dev.lastSeenAt ? `Last active ${new Date(dev.lastSeenAt).toLocaleString()}` : `Device ID: ${dev.id}`}
                  </div>
                </div>
                {!dev.revoked ? (
                  confirmRevokeId === dev.id ? (
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        aria-label={`Confirm revoke ${dev.name || dev.id}`}
                        onClick={() => void handleRevoke(dev.id)}
                        className="h-7 rounded bg-destructive px-2 text-[11px] font-medium text-destructive-foreground hover:bg-destructive/90 transition-colors"
                      >
                        Confirm Revoke
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmRevokeId(null)}
                        className="h-7 rounded border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent transition-colors"
                      >
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      aria-label={`Revoke device ${dev.name || dev.id}`}
                      onClick={() => setConfirmRevokeId(dev.id)}
                      className="inline-flex h-7 items-center gap-1 rounded border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-destructive transition-colors"
                    >
                      <Trash2 className="size-3" />
                      Revoke
                    </button>
                  )
                ) : null}
              </div>
            ))
          )}
        </div>
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