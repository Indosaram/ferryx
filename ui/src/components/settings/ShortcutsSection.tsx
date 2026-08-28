import { useState } from "react";
import { Keyboard, Search } from "lucide-react";
import { SHORTCUTS, shortcutAliasesLabels, shortcutLabel } from "../../lib/shortcuts";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { SettingsHeading } from "./primitives";
import type { ShortcutsSectionProps } from "./types";

const GROUPS = ["all", "Tabs", "Workspaces", "Terminal Panes", "Global", "View"] as const;

export function ShortcutsSection({ isMac }: ShortcutsSectionProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedGroup, setSelectedGroup] = useState<string>("all");

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
        description="Only registered modifier chords are intercepted by the shell. Ordinary terminal typing and Ctrl-C remain with the terminal."
      />
      <h2 id="settings-shortcuts-heading" className="sr-only">Keyboard Shortcuts</h2>

      <div className="mb-3 space-y-2">
        <div className="relative flex items-center">
          <Search className="pointer-events-none absolute left-2.5 size-3.5 text-muted-foreground z-10" />
          <Input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search shortcuts (e.g. tab, split, zoom, workspace)..."
            aria-label="Search keyboard shortcuts"
            className="h-8 bg-background pl-8 pr-3 text-xs"
          />
        </div>
        <div className="flex flex-wrap gap-1">
          {GROUPS.map((group) => {
            const isSelected = selectedGroup === group;
            return (
              <Button
                key={group}
                type="button"
                size="sm"
                variant={isSelected ? "default" : "secondary"}
                onClick={() => setSelectedGroup(group)}
                className={`h-6 rounded px-2 text-[11px] font-medium shadow-none ${
                  isSelected
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground"
                }`}
              >
                {group === "all" ? "All" : group}
              </Button>
            );
          })}
        </div>
      </div>

      <div className="border-y border-border">
        {filteredShortcuts.length > 0 ? (
          filteredShortcuts.map((shortcut) => {
            const aliases = shortcutAliasesLabels(shortcut.id, isMac);
            return (
              <div
                key={shortcut.id}
                className="flex min-h-10 items-center justify-between gap-4 border-b border-border px-1 last:border-b-0"
              >
                <div className="min-w-0 py-2">
                  <div className="truncate text-[12px] font-medium text-foreground">{shortcut.title}</div>
                  <div className="text-[10px] text-muted-foreground">{shortcut.group}</div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <kbd className="rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground">
                    {shortcutLabel(shortcut.id, isMac)}
                  </kbd>
                  {aliases.map((alias) => (
                    <kbd
                      key={alias}
                      className="rounded border border-border/60 bg-muted/40 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                    >
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

export const ShortcutSettings = ShortcutsSection;
