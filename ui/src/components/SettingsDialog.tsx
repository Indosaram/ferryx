import { useEffect, useState } from "react";
import {
  ArrowLeft,
  Bell,
  Bot,
  FolderGit2,
  Globe,
  Keyboard,
  Palette,
  Radio,
  Settings2,
  TerminalSquare,
} from "lucide-react";

import { isMacShortcutPlatform } from "../lib/shortcuts";
import { useTerminalSettings } from "../lib/terminalSettings";
import { AgentsSection } from "./settings/AgentsSection";
import { AppearanceSection } from "./settings/AppearanceSection";
import { BrowserSection } from "./settings/BrowserSection";
import { GeneralSection } from "./settings/GeneralSection";
import { NotificationsSection } from "./settings/NotificationsSection";
import { RemoteAccessSection } from "./settings/RemoteAccessSection";
import { ShortcutsSection } from "./settings/ShortcutsSection";
import { TerminalSection } from "./settings/TerminalSection";
import type { RegisteredProject, SectionId, Worktree } from "./settings/types";
import { WorkspaceSection } from "./settings/WorkspaceSection";

export type SettingsDialogProps = {
  open: boolean;
  onClose: () => void;
  projects?: RegisteredProject[];
  activeProjectId?: string;
  activeWorktree?: Worktree | null;
  onSelectProject?: (project: RegisteredProject) => void;
  onAddProject?: () => void;
  onAddWorktree?: () => void;
};

export function SettingsDialog({
  open,
  ...props
}: SettingsDialogProps) {
  if (!open) return null;
  return <SettingsDialogBody {...props} />;
}

type SettingsDialogBodyProps = Omit<SettingsDialogProps, "open">;

function SettingsDialogBody({
  onClose,
  projects = [],
  activeProjectId,
  activeWorktree = null,
  onSelectProject,
  onAddProject,
  onAddWorktree,
}: SettingsDialogBodyProps) {
  const { settings, nativePreferences, updateSettings, refreshNativePreferences } = useTerminalSettings();
  const [section, setSection] = useState<SectionId>("general");
  const isMac = isMacShortcutPlatform();

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [onClose]);

  const hasLocalTerminalOverride =
    settings.fontFamilySource === "local" ||
    settings.macosOptionAsAltSource === "local" ||
    settings.fontSizeSource === "local";
  let terminalSource = "Built-in defaults";
  if (hasLocalTerminalOverride) {
    terminalSource = "Local override";
  } else if (nativePreferences.source === "ghostty" && nativePreferences.status === "imported") {
    terminalSource = "Ghostty · Imported";
  } else if (nativePreferences.status === "malformed") {
    terminalSource = "Ghostty · Malformed · Safe defaults";
  }

  return (
    <div
      role="dialog"
      aria-label="Settings"
      aria-modal={false}
      className="fixed inset-0 z-50 flex overflow-hidden bg-background text-foreground"
    >
      <aside data-testid="settings-nav" className="flex w-[280px] shrink-0 flex-col border-r border-border bg-background">
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
            <NavButton active={section === "agents"} icon={<Bot />} label="Agents" onClick={() => setSection("agents")} />
            <NavButton active={section === "browser"} icon={<Globe />} label="Browser" onClick={() => setSection("browser")} />
            <NavButton active={section === "notifications"} icon={<Bell />} label="Notifications" onClick={() => setSection("notifications")} />
            <NavButton active={section === "remote"} icon={<Radio />} label="Remote Access" onClick={() => setSection("remote")} />
          </nav>
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto bg-background scrollbar-sleek">
        <div data-tauri-drag-region className="drag-region h-titlebar shrink-0 border-b border-border/70" />
        <div className="mx-auto w-full max-w-[896px] px-8 pb-16 pt-10">
          {section === "general" ? <GeneralSection /> : null}
          {section === "appearance" ? <AppearanceSection /> : null}
          {section === "terminal" ? (
            <TerminalSection
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
                updateSettings({ fontFamily: null, macosOptionAsAlt: null, fontSize: null });
                void refreshNativePreferences();
              }}
            />
          ) : null}
          {section === "shortcuts" ? <ShortcutsSection isMac={isMac} /> : null}
          {section === "workspace" ? (
            <WorkspaceSection
              projects={projects}
              activeProjectId={activeProjectId}
              activeWorktree={activeWorktree}
              onSelectProject={onSelectProject}
              onAddProject={onAddProject}
              onAddWorktree={onAddWorktree}
            />
          ) : null}
          {section === "agents" ? <AgentsSection /> : null}
          {section === "browser" ? <BrowserSection /> : null}
          {section === "notifications" ? <NotificationsSection /> : null}
          {section === "remote" ? <RemoteAccessSection /> : null}
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

export { GeneralSection, GeneralSection as GeneralSettings, SoftwareUpdateCard, CliLauncherCard } from "./settings/GeneralSection";
export { AppearanceSection, AppearanceSection as AppearanceSettings } from "./settings/AppearanceSection";
export { TerminalSection, TerminalSection as TerminalSettings } from "./settings/TerminalSection";
export { ShortcutsSection, ShortcutsSection as ShortcutSettings } from "./settings/ShortcutsSection";
export { WorkspaceSection, WorkspaceSection as WorkspaceSettings } from "./settings/WorkspaceSection";
export { AgentsSection, AgentsSection as AgentsSettings } from "./settings/AgentsSection";
export { BrowserSection, BrowserSection as BrowserSettings, BrowserSection as BrowserSettingsPanel } from "./settings/BrowserSection";
export { NotificationsSection, NotificationsSection as NotificationSettings } from "./settings/NotificationsSection";
export { RemoteAccessSection, RemoteAccessSection as RemoteAccessSettings } from "./settings/RemoteAccessSection";
