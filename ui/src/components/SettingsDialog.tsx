import { Keyboard, Settings2, TerminalSquare, X } from "lucide-react";
import { useEffect } from "react";

import { SHORTCUTS, isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import { useTerminalSettings } from "../lib/terminalSettings";

export type SettingsDialogProps = {
  open: boolean;
  onClose: () => void;
};

export function SettingsDialog({ open, onClose }: SettingsDialogProps) {
  const { settings, updateSettings } = useTerminalSettings();
  const isMac = isMacShortcutPlatform();

  useEffect(() => {
    if (!open) return;
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-label="Settings"
        aria-modal="true"
        className="w-full max-w-xl animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
            <Settings2 className="size-4 text-muted-foreground" />
            Settings
          </div>
          <button type="button" onClick={onClose} aria-label="Close settings" className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="size-4" />
          </button>
        </header>

        <div className="max-h-[70vh] space-y-6 overflow-y-auto p-4 scrollbar-sleek">
          <section aria-labelledby="settings-terminal-heading" className="space-y-3">
            <div className="flex items-center gap-2">
              <TerminalSquare className="size-4 text-muted-foreground" />
              <h2 id="settings-terminal-heading" className="text-sm font-semibold text-foreground">Terminal</h2>
            </div>
            <div className="grid gap-3 rounded-lg border border-border bg-background/45 p-3 sm:grid-cols-2">
              <label className="space-y-1.5 text-xs text-muted-foreground">
                <span className="block font-medium text-foreground">Font size</span>
                <input
                  aria-label="Font size"
                  type="number"
                  min={10}
                  max={24}
                  value={settings.fontSize}
                  onChange={(event) => updateSettings({ fontSize: Number(event.target.value) })}
                  className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring"
                />
                <span className="block text-[10px]">10–24 px</span>
              </label>
              <label className="space-y-1.5 text-xs text-muted-foreground">
                <span className="block font-medium text-foreground">Scrollback</span>
                <input
                  aria-label="Scrollback"
                  type="number"
                  min={1000}
                  max={100000}
                  step={1000}
                  value={settings.scrollback}
                  onChange={(event) => updateSettings({ scrollback: Number(event.target.value) })}
                  className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring"
                />
                <span className="block text-[10px]">1,000–100,000 lines</span>
              </label>
            </div>
          </section>

          <section aria-labelledby="settings-shortcuts-heading" className="space-y-3">
            <div className="flex items-center gap-2">
              <Keyboard className="size-4 text-muted-foreground" />
              <h2 id="settings-shortcuts-heading" className="text-sm font-semibold text-foreground">Shortcuts</h2>
            </div>
            <div className="overflow-hidden rounded-lg border border-border">
              {SHORTCUTS.map((shortcut) => (
                <div key={shortcut.id} className="flex items-center justify-between gap-4 border-b border-border px-3 py-2 last:border-b-0">
                  <div className="min-w-0">
                    <div className="truncate text-xs font-medium text-foreground">{shortcut.title}</div>
                    <div className="text-[10px] text-muted-foreground">{shortcut.group}</div>
                  </div>
                  <kbd className="shrink-0 rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-[10px] text-foreground">
                    {shortcutLabel(shortcut.id, isMac)}
                  </kbd>
                </div>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
