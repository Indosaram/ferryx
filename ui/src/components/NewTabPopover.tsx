import { useRef, useEffect, useMemo, useState } from "react";
import { TerminalSquare, Globe, FileText, Smartphone, Settings, Sparkles, Workflow } from "lucide-react";
import {
  newBrowserTabUrl,
  resolveSupportedBrowserProfileId,
  supportedBrowserProfiles,
  useBrowserSettings,
} from "../lib/browserSettings";
import { formatBindingLabel, isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";

export type PopoverAgent = {
  name: string;
  command: string;
  args: string;
  enabled?: boolean;
  available?: boolean;
};

interface NewTabPopoverProps {
  open: boolean;
  onClose: () => void;
  onNewTerminal: () => void;
  onNewBrowser: (url?: string, profileId?: string) => void;
  onNewMarkdown?: () => void;
  onNewMobileEmulator?: () => void;
  onNewDag?: () => void;
  onOpenSettings?: () => void;
  agents?: PopoverAgent[];
  onLaunchAgent?: (agent: PopoverAgent) => void;
  defaultAgentId?: string | null;
}

function isUsableDefaultAgent(agent: PopoverAgent, defaultAgentId?: string | null): boolean {
  if (!defaultAgentId || defaultAgentId === "none") return false;
  if (agent.name !== defaultAgentId) return false;
  if (agent.enabled === false || agent.available === false) return false;
  return true;
}

function orderAgentsForNewTab(agents: PopoverAgent[], defaultAgentId?: string | null): PopoverAgent[] {
  const defaultIndex = agents.findIndex((agent) => isUsableDefaultAgent(agent, defaultAgentId));
  if (defaultIndex <= 0) return agents;
  const ordered = agents.slice();
  const [selected] = ordered.splice(defaultIndex, 1);
  ordered.unshift(selected);
  return ordered;
}

export function NewTabPopover({
  open,
  onClose,
  onNewTerminal,
  onNewBrowser,
  onNewMarkdown,
  onNewMobileEmulator,
  onNewDag,
  onOpenSettings,
  agents,
  onLaunchAgent,
  defaultAgentId,
}: NewTabPopoverProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const isMac = isMacShortcutPlatform();
  const { settings } = useBrowserSettings();
  const browserProfiles = useMemo(() => supportedBrowserProfiles(settings), [settings]);
  const [browserProfileId, setBrowserProfileId] = useState(() => resolveSupportedBrowserProfileId(undefined, settings));
  const launcherAgents = useMemo(
    () => (agents && agents.length > 0 ? orderAgentsForNewTab(agents, defaultAgentId) : agents),
    [agents, defaultAgentId],
  );

  useEffect(() => {
    setBrowserProfileId((current) => resolveSupportedBrowserProfileId(current, settings));
  }, [settings]);

  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      ref={containerRef}
      role="dialog"
      aria-label="New tab menu"
      className="absolute top-full left-0 mt-1 w-80 rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl z-50 p-2 text-xs select-none backdrop-blur-md"
    >
      <div className="flex flex-col gap-0.5">
        <button
          type="button"
          onClick={() => {
            onNewTerminal();
            onClose();
          }}
          className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
        >
          <div className="flex items-center gap-2.5">
            <TerminalSquare className="size-4 text-muted-foreground group-hover:text-foreground" />
            <span className="font-medium">New Terminal</span>
          </div>
          <span className="text-[11px] font-mono text-muted-foreground">
            {shortcutLabel("tab.newTerminal", isMac)}
          </span>
        </button>

        <div className="rounded-lg hover:bg-accent/30">
          <button
            type="button"
            onClick={() => {
              const profileId = browserProfileId === settings.defaultProfileId
                ? undefined
                : browserProfileId;
              onNewBrowser(newBrowserTabUrl(), profileId);
              onClose();
            }}
            className="flex w-full items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground transition-colors group"
          >
            <div className="flex items-center gap-2.5">
              <Globe className="size-4 text-muted-foreground group-hover:text-foreground" />
              <span className="font-medium">New Browser Tab</span>
            </div>
            <span className="text-[11px] font-mono text-muted-foreground">
              {shortcutLabel("tab.newBrowser", isMac)}
            </span>
          </button>
          <div className="flex items-center gap-2 px-2.5 pb-2">
            <span className="text-[10px] text-muted-foreground">Profile</span>
            <select
              aria-label="Browser profile"
              value={browserProfileId}
              onChange={(event) => setBrowserProfileId(event.target.value)}
              className="min-w-0 flex-1 rounded border border-border bg-background px-1.5 py-1 text-[10px] text-foreground outline-none focus:border-ring"
            >
              {browserProfiles.map((profile) => (
                <option key={profile.id} value={profile.id}>{profile.name} profile</option>
              ))}
            </select>
          </div>
        </div>

        {onNewDag ? (
          <button
            type="button"
            onClick={() => {
              onNewDag();
              onClose();
            }}
            className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
          >
            <div className="flex items-center gap-2.5">
              <Workflow className="size-4 text-muted-foreground group-hover:text-foreground" />
              <span className="font-medium">New DAG View</span>
            </div>
          </button>
        ) : null}

        {onNewMarkdown ? (
          <button
            type="button"
            onClick={() => {
              onNewMarkdown();
              onClose();
            }}
            className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
          >
            <div className="flex items-center gap-2.5">
              <FileText className="size-4 text-muted-foreground group-hover:text-foreground" />
              <span className="font-medium">New Markdown</span>
            </div>
            <span className="text-[11px] font-mono text-muted-foreground">
              {formatBindingLabel({ key: "m", mod: true, shift: true }, isMac)}
            </span>
          </button>
        ) : null}

        {onNewMobileEmulator ? (
          <button
            type="button"
            onClick={() => {
              onNewMobileEmulator();
              onClose();
            }}
            className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
          >
            <div className="flex items-center gap-2.5">
              <Smartphone className="size-4 text-muted-foreground group-hover:text-foreground" />
              <span className="font-medium">New Mobile Emulator</span>
            </div>
            <span className="text-[11px] font-mono text-muted-foreground">
              {formatBindingLabel({ key: "e", mod: true, alt: true, shift: true }, isMac)}
            </span>
          </button>
        ) : null}

        {launcherAgents && launcherAgents.length > 0 ? (
          <div className="flex flex-col gap-0.5 border-t border-border/60 pt-1.5 mt-0.5">
            <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60">
              AGENTS
            </div>
            {launcherAgents.map((agent) => {
              const label = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
              const isDefault = isUsableDefaultAgent(agent, defaultAgentId);
              return (
                <button
                  key={agent.name}
                  type="button"
                  onClick={() => {
                    onLaunchAgent?.(agent);
                    onClose();
                  }}
                  className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group"
                >
                  <div className="flex items-center gap-2.5">
                    <Sparkles className="size-4 text-muted-foreground group-hover:text-foreground" />
                    <span className="font-medium">{label}</span>
                  </div>
                  {isDefault ? (
                    <span className="rounded-full bg-accent px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
                      Default
                    </span>
                  ) : null}
                </button>
              );
            })}
          </div>
        ) : null}

        {onOpenSettings ? (
          <button
            type="button"
            onClick={() => {
              onOpenSettings();
              onClose();
            }}
            className="flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left text-foreground hover:bg-accent/50 transition-colors group border-t border-border/60 mt-0.5 pt-1.5"
          >
            <div className="flex items-center gap-2.5">
              <Settings className="size-4 text-muted-foreground group-hover:text-foreground" />
              <span className="font-medium">Agent settings…</span>
            </div>
          </button>
        ) : null}
      </div>
    </div>
  );
}
