import { AppWindow, GitBranch, GitCompare, Palette, Search, TerminalSquare, X, type LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";

import { cycleAppearanceTheme, openAppearanceSurface } from "../lib/appearanceCommands";
import type { useAppearanceSettings } from "../lib/appearanceSettings";
import { isMacShortcutPlatform, resolvedShortcuts, shortcutLabel } from "../lib/shortcuts";
import { useShortcutOverrides, useShortcutPreset } from "../lib/shortcutOverrides";
import { staticTabLabel } from "../lib/tabTitle";
import type { WorkspaceTab, Worktree } from "../lib/types";

export type CommandPaletteProps = {
  open: boolean;
  worktrees: Worktree[];
  tabs: WorkspaceTab[];
  onSelectWorktree: (worktree: Worktree) => void;
  onSelectTab: (tabId: string) => void;
  /** Present when the active worktree can show its Git changes. */
  onShowChanges?: () => void;
  /** Present only in the desktop shell, where a second window can be opened. */
  onNewWindow?: () => void;
  /** Installed appearance store and the shell's appearance surface; absent on unsupported surfaces. */
  appearance?: Pick<ReturnType<typeof useAppearanceSettings>, "settings" | "updateSettings"> & { onOpen: () => void };
  onClose: () => void;
};

type PaletteCommand = { id: string; title: string; run: () => void; icon: LucideIcon };

const focusableSelector = 'button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export function CommandPalette({ open, worktrees, tabs, onSelectWorktree, onSelectTab, onShowChanges, onNewWindow, appearance, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const isMac = isMacShortcutPlatform();
  const { overrides: shortcutOverrides } = useShortcutOverrides();
  const { preset: shortcutPreset } = useShortcutPreset(isMac);
  const normalizedQuery = query.trim().toLowerCase();
  const filteredWorktrees = useMemo(
    () => worktrees.filter((worktree) => worktreeSearchText(worktree).includes(normalizedQuery)),
    [normalizedQuery, worktrees],
  );
  const filteredCommands = useMemo(() => {
    const commands: PaletteCommand[] = [];
    if (onShowChanges) commands.push({ id: "show-changes", title: "Show changes", run: onShowChanges, icon: GitCompare });
    if (onNewWindow) commands.push({ id: "window.new", title: "New Window", run: onNewWindow, icon: AppWindow });
    if (appearance) {
      commands.push(
        { id: "appearance.cycleTheme", title: "Cycle theme", run: () => cycleAppearanceTheme(appearance.settings, appearance.updateSettings), icon: Palette },
        { id: "appearance.openSettings", title: "Change appearance…", run: () => openAppearanceSurface(appearance.onOpen), icon: Palette },
        ...appearance.settings.customThemes.map((theme) => ({
          id: `appearance.theme.${theme.id}`,
          title: `Theme: ${theme.name}`,
          run: () => { appearance.updateSettings({ activeCustomThemeId: theme.id }); },
          icon: Palette,
        })),
      );
    }
    return commands.filter((command) => command.title.toLowerCase().includes(normalizedQuery));
  }, [appearance, normalizedQuery, onNewWindow, onShowChanges]);
  const filteredTabs = useMemo(
    () => tabs.filter((tab) => staticTabLabel(tab).toLowerCase().includes(normalizedQuery)),
    [normalizedQuery, tabs],
  );

  // Remember the control that opened the palette, move focus into the search field, and hand
  // focus back on close or unmount so the opener is never left pointing at removed DOM.
  useEffect(() => {
    if (!open) return;
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    inputRef.current?.focus();
    return () => {
      const opener = openerRef.current;
      openerRef.current = null;
      if (opener?.isConnected) opener.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, open]);

  // aria-modal promises focus stays inside: wrap Tab / Shift+Tab at the palette edges.
  const handleDialogKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusables = Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector));
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 px-4 pt-[12vh] backdrop-blur-sm" onMouseDown={onClose}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-label="Command palette"
        aria-modal="true"
        className="w-full max-w-2xl animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={handleDialogKeyDown}
      >
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label="Search commands"
            placeholder="Search worktrees and terminal tabs"
            className="h-11 min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
          />
          <button type="button" onClick={onClose} aria-label="Close command palette" className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="size-4" />
          </button>
        </div>

        <div className="max-h-[55vh] overflow-y-auto p-2 scrollbar-sleek">
          {filteredCommands.length > 0 ? (
            <section aria-label="Commands" className="mb-2">
              <h2 className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Commands</h2>
              {filteredCommands.map((command) => (
                <button
                  key={command.id}
                  type="button"
                  data-command-id={command.id}
                  onClick={() => {
                    command.run();
                    onClose();
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm text-foreground hover:bg-accent"
                >
                  <command.icon className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate">{command.title}</span>
                  {command.id === "appearance.cycleTheme" || command.id === "appearance.openSettings" ? (
                    <kbd className="rounded border border-border bg-muted px-1 py-px text-[10px] text-foreground">
                      {shortcutLabel(command.id, isMac, shortcutOverrides, shortcutPreset)}
                    </kbd>
                  ) : null}
                </button>
              ))}
            </section>
          ) : null}

          {filteredWorktrees.length > 0 ? (
            <section aria-label="Worktrees" className="mb-2">
              <h2 className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Worktrees</h2>
              {filteredWorktrees.map((worktree) => (
                <button
                  key={worktree.path}
                  type="button"
                  onClick={() => {
                    onSelectWorktree(worktree);
                    onClose();
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-accent"
                >
                  <GitBranch className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-foreground">{worktreeName(worktree)}</span>
                    <span className="block truncate font-mono text-[10px] text-muted-foreground">{branchName(worktree)}</span>
                  </span>
                </button>
              ))}
            </section>
          ) : null}

          {filteredTabs.length > 0 ? (
            <section aria-label="Terminal tabs">
              <h2 className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Terminal tabs</h2>
              {filteredTabs.map((tab) => (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => {
                    onSelectTab(tab.id);
                    onClose();
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm text-foreground hover:bg-accent"
                >
                  <TerminalSquare className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate">{staticTabLabel(tab)}</span>
                </button>
              ))}
            </section>
          ) : null}

          {filteredCommands.length === 0 && filteredWorktrees.length === 0 && filteredTabs.length === 0 ? (
            <div className="px-3 py-8 text-center text-xs text-muted-foreground">No matching worktrees or terminal tabs.</div>
          ) : null}
        </div>

        <div className="flex flex-wrap gap-x-3 gap-y-1 border-t border-border px-3 py-2 text-[10px] text-muted-foreground">
          {resolvedShortcuts(shortcutOverrides, shortcutPreset, isMac)
            .filter((shortcut) => appearance || !shortcut.id.startsWith("appearance."))
            .map((shortcut) => (
            <span key={shortcut.id} className="inline-flex items-center gap-1">
              <span>{shortcut.title}</span>
              <kbd className="rounded border border-border bg-muted px-1 py-px text-foreground">{shortcutLabel(shortcut.id, isMac, shortcutOverrides, shortcutPreset)}</kbd>
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

function branchName(worktree: Worktree) {
  return worktree.branch?.replace(/^refs\/heads\//, "") ?? "detached HEAD";
}

function worktreeName(worktree: Worktree) {
  if (worktree.path === ".") return "main";
  return worktree.path.split(/[\\/]/).filter(Boolean).at(-1) ?? worktree.path;
}

function worktreeSearchText(worktree: Worktree) {
  return `${worktreeName(worktree)} ${branchName(worktree)} ${worktree.path}`.toLowerCase();
}
