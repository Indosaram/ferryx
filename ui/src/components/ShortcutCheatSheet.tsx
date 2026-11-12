import { Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  SHORTCUTS,
  isMacShortcutPlatform,
  shortcutLabel,
  type ShortcutOverrideMap,
  type ShortcutPreset,
} from "../lib/shortcuts";

/** Fixed reading order; covers every `ShortcutDefinition.group` value in the registry. */
const GROUPS = ["Global", "Tabs", "Workspaces", "Terminal Panes", "View"] as const;

export type ShortcutCheatSheetProps = {
  open: boolean;
  onClose: () => void;
  isMac?: boolean;
  shortcutOverrides?: ShortcutOverrideMap;
  shortcutPreset?: ShortcutPreset;
};

type SheetRow = { readonly id: string; readonly title: string; readonly label: string };

/** Read-only, searchable view of the live registry: overrides and preset shape every key label. */
export function ShortcutCheatSheet({
  open,
  onClose,
  isMac = isMacShortcutPlatform(),
  shortcutOverrides = {},
  shortcutPreset = "default",
}: ShortcutCheatSheetProps) {
  const [query, setQuery] = useState("");
  const containerRef = useRef<HTMLDivElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  // Captured during render, before commit: the search input's autoFocus moves focus into the sheet
  // during commit, so a passive effect would record the sheet itself as the return target.
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);
  if (open && !wasOpenRef.current) {
    returnFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }
  wasOpenRef.current = open;

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKeyDown, true);
    // Keyed on `open` alone: a new inline onClose must not tear the listener down mid-dialog,
    // because the cleanup is what restores focus.
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      const target = returnFocusRef.current;
      if (target?.isConnected) target.focus();
      returnFocusRef.current = null;
    };
  }, [open]);

  const trapTab = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const focusable = containerRef.current?.querySelectorAll<HTMLElement>(
      'button, input, [href], select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    if (!focusable || focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !containerRef.current?.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const groups = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return GROUPS.map((group) => {
      const rows: SheetRow[] = SHORTCUTS.filter((shortcut) => shortcut.group === group).map((shortcut) => ({
        id: shortcut.id,
        title: shortcut.title,
        label: shortcutLabel(shortcut.id, isMac, shortcutOverrides, shortcutPreset),
      }));
      return {
        group,
        rows: normalizedQuery
          ? rows.filter((row) =>
              row.title.toLowerCase().includes(normalizedQuery) || row.label.toLowerCase().includes(normalizedQuery))
          : rows,
      };
    }).filter((section) => section.rows.length > 0);
  }, [isMac, query, shortcutOverrides, shortcutPreset]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onMouseDown={onClose}>
      <div
        ref={containerRef}
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard shortcuts"
        data-testid="shortcut-cheat-sheet"
        onKeyDown={trapTab}
        className="w-full max-w-2xl animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search shortcuts"
            aria-label="Search keyboard shortcuts"
            className="h-11 min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
          />
          <button
            type="button"
            onClick={onClose}
            aria-label="Close keyboard shortcuts"
            className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="max-h-[60vh] overflow-y-auto p-2 scrollbar-sleek">
          {groups.length > 0 ? (
            groups.map((section) => (
              <section key={section.group} aria-label={section.group} className="mb-2 last:mb-0">
                <h2 className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{section.group}</h2>
                {section.rows.map((row) => (
                  <div
                    key={row.id}
                    data-testid="shortcut-cheat-sheet-row"
                    data-shortcut-id={row.id}
                    className="flex items-center justify-between gap-4 rounded-md px-2 py-1.5 text-[13px]"
                  >
                    <span className="min-w-0 truncate text-foreground">{row.title}</span>
                    {row.label ? (
                      <kbd className="shrink-0 rounded border border-border bg-muted px-1 py-px text-foreground">{row.label}</kbd>
                    ) : (
                      <span className="shrink-0 text-[11px] italic text-muted-foreground">Unbound</span>
                    )}
                  </div>
                ))}
              </section>
            ))
          ) : (
            <div className="px-3 py-8 text-center text-xs text-muted-foreground">No matching shortcuts.</div>
          )}
        </div>

        <div className="border-t border-border px-3 py-2 text-[10px] text-muted-foreground">Esc to close</div>
      </div>
    </div>
  );
}
