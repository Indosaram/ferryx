import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  Bell,
  Bot,
  Globe,
  Keyboard,
  Palette,
  Radio,
  Settings2,
  Shield,
  TerminalSquare,
} from "lucide-react";

import { isMacShortcutPlatform } from "../lib/shortcuts";
import { useTerminalSettings } from "../lib/terminalSettings";
import { AgentsSection } from "./settings/AgentsSection";
import { AppearanceSection } from "./settings/AppearanceSection";
import { BrowserSection } from "./settings/BrowserSection";
import { GeneralSection } from "./settings/GeneralSection";
import { NotificationsSection } from "./settings/NotificationsSection";
import { PermissionsSection } from "./settings/PermissionsSection";
import { RemoteAccessSection } from "./settings/RemoteAccessSection";
import { ShortcutsSection } from "./settings/ShortcutsSection";
import { TerminalSection } from "./settings/TerminalSection";
import type { SectionId } from "./settings/types";

export type SettingsDialogProps = {
  open: boolean;
  onClose: () => void;
  initialSection?: SectionId;
};

export function SettingsDialog({
  open,
  ...props
}: SettingsDialogProps) {
  if (!open) return null;
  return <SettingsDialogBody {...props} />;
}

type SettingsDialogBodyProps = Omit<SettingsDialogProps, "open">;

const VALID_SECTIONS: readonly SectionId[] = [
  "general",
  "appearance",
  "terminal",
  "shortcuts",
  "agents",
  "browser",
  "notifications",
  "remote",
  "permissions",
];

function sanitizeSectionId(candidate: unknown): SectionId {
  if (typeof candidate === "string" && VALID_SECTIONS.includes(candidate as SectionId)) {
    return candidate as SectionId;
  }
  return "general";
}

function SettingsDialogBody({ onClose, initialSection }: SettingsDialogBodyProps) {
  const { settings, localSettings, nativePreferences, updateSettings, refreshNativePreferences } = useTerminalSettings();
  const [section, setSection] = useState<SectionId>(sanitizeSectionId(initialSection));
  const isMac = isMacShortcutPlatform();
  const backButtonRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    backButtonRef.current?.focus();
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (event.defaultPrevented) return;

      const target = event.target;
      if (target instanceof Element) {
        if (target.closest('[data-state="open"], [role="listbox"]')) {
          return;
        }
        if (
          (target instanceof HTMLInputElement &&
            (target.type === "search" || target.getAttribute("role") === "searchbox")) ||
          target.closest('[role="search"]')
        ) {
          return;
        }
        if (document.querySelector('[role="listbox"], [role="dialog"][data-state="open"]')) {
          return;
        }
      }

      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const hasLocalTerminalOverride =
    settings.fontFamilySource === "local" ||
    settings.macosOptionAsAltSource === "local" ||
    settings.fontSizeSource === "local" ||
    localSettings.shell !== null;
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
            ref={backButtonRef}
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
            <NavButton active={section === "agents"} icon={<Bot />} label="Agents" onClick={() => setSection("agents")} />
            <NavButton active={section === "browser"} icon={<Globe />} label="Browser" onClick={() => setSection("browser")} />
            <NavButton active={section === "notifications"} icon={<Bell />} label="Notifications" onClick={() => setSection("notifications")} />
            <NavButton active={section === "permissions"} icon={<Shield />} label="Permissions" onClick={() => setSection("permissions")} />
            <NavButton active={section === "remote"} icon={<Radio />} label="Remote Access" onClick={() => setSection("remote")} />
          </nav>
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto bg-background scrollbar-sleek">
        <div data-tauri-drag-region className="drag-region h-titlebar shrink-0 border-b border-border/70" />
        <div className="selectable mx-auto w-full max-w-[896px] px-8 pb-16 pt-10">
          {section === "general" ? <GeneralSection /> : null}
          {section === "appearance" ? <AppearanceSection /> : null}
          {section === "terminal" ? (
            <TerminalSection
              fontFamily={settings.fontFamily}
              fontSize={settings.fontSize}
              macosOptionAsAlt={settings.macosOptionAsAlt}
              shell={localSettings.shell}
              source={terminalSource}
              sourcePath={nativePreferences.sourcePath}
              onFontFamily={(fontFamily) => updateSettings({ fontFamily })}
              onFontSize={(fontSize) => updateSettings({ fontSize })}
              onOptionAsAlt={(macosOptionAsAlt) => updateSettings({ macosOptionAsAlt })}
              onShell={(shell) => updateSettings({ shell })}
              onUseImported={() => {
                updateSettings({ fontFamily: null, macosOptionAsAlt: null, fontSize: null, shell: null });
                void refreshNativePreferences();
              }}
            />
          ) : null}
          {section === "shortcuts" ? <ShortcutsSection isMac={isMac} /> : null}
          {section === "agents" ? <AgentsSection /> : null}
          {section === "browser" ? <BrowserSection /> : null}
          {section === "notifications" ? <NotificationsSection /> : null}
          {section === "permissions" ? <PermissionsSection /> : null}
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
      aria-current={active ? "page" : undefined}
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
export { AgentsSection, AgentsSection as AgentsSettings } from "./settings/AgentsSection";
export { BrowserSection, BrowserSection as BrowserSettings, BrowserSection as BrowserSettingsPanel } from "./settings/BrowserSection";
export { NotificationsSection, NotificationsSection as NotificationSettings } from "./settings/NotificationsSection";
export { RemoteAccessSection, RemoteAccessSection as RemoteAccessSettings } from "./settings/RemoteAccessSection";
