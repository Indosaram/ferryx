import { GitBranch, GitCompare, Search, TerminalSquare, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

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
  onClose: () => void;
};

type PaletteCommand = { id: string; title: string; run: () => void };

export function CommandPalette({ open, worktrees, tabs, onSelectWorktree, onSelectTab, onShowChanges, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const isMac = isMacShortcutPlatform();
  const { overrides: shortcutOverrides } = useShortcutOverrides();
  const { preset: shortcutPreset } = useShortcutPreset(isMac);
  const normalizedQuery = query.trim().toLowerCase();
  const filteredWorktrees = useMemo(
    () => worktrees.filter((worktree) => worktreeSearchText(worktree).includes(normalizedQuery)),
    [normalizedQuery, worktrees],
  );
  const filteredCommands = useMemo(() => {
    const commands: PaletteCommand[] = onShowChanges ? [{ id: "show-changes", title: "Show changes", run: onShowChanges }] : [];
    return commands.filter((command) => command.title.toLowerCase().includes(normalizedQuery));
  }, [normalizedQuery, onShowChanges]);
  const filteredTabs = useMemo(
    () => tabs.filter((tab) => staticTabLabel(tab).toLowerCase().includes(normalizedQuery)),
    [normalizedQuery, tabs],
  );

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    inputRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, open]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 px-4 pt-[12vh] backdrop-blur-sm" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-label="Command palette"
        aria-modal="true"
        className="w-full max-w-2xl animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
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
                  <GitCompare className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate">{command.title}</span>
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
          {resolvedShortcuts(shortcutOverrides, shortcutPreset, isMac).map((shortcut) => (
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
