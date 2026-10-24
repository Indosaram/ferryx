import { useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { Keyboard, Search } from "lucide-react";
import {
  SHORTCUTS,
  findConflict,
  formatBindingLabel,
  shortcutAliasesLabels,
  shortcutLabel,
  type ShortcutActionId,
  type ShortcutBinding,
  type ShortcutConflict,
  type ShortcutPreset,
} from "../../lib/shortcuts";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { SettingsHeading } from "./primitives";
import type { ShortcutsSectionProps } from "./types";

const GROUPS = ["all", "Tabs", "Workspaces", "Terminal Panes", "Global", "View"] as const;

const PRESET_DESCRIPTIONS: Record<ShortcutPreset, string> = {
  default: "Ctrl+1-9, Ctrl+Tab and Ctrl+PgUp/PgDn switch tabs; Alt+1-9 switches workspaces.",
  "terminal-first":
    "Ctrl+1-9, Ctrl+Tab and Ctrl+PgUp/PgDn go to the terminal. Alt+1-9 and Alt+PgUp/PgDn switch tabs; Alt+Shift+1-9 switches workspaces.",
};

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "OS", "AltGraph", "Hyper", "Super", "Fn", "CapsLock"]);

const CODE_KEYS: Record<string, string> = {
  BracketLeft: "[",
  BracketRight: "]",
  Equal: "=",
  Minus: "-",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  Backslash: "\\",
  Space: "Space",
};

/**
 * Turns the recorder's keydown into an abstract binding (`mod` = Cmd on macOS, Ctrl elsewhere),
 * or `null` when the keystroke is not a recordable chord. Uses the same IME and AltGr guards as
 * `matchesBinding`: composition keydowns and AltGr glyphs belong to text entry, never to a chord.
 * The physical `code` names letters, digits and punctuation so a Hangul or Option-glyph `key`
 * still records the key the user pressed.
 */
function chordFromEvent(event: KeyboardEvent, isMac: boolean): ShortcutBinding | null {
  if (event.isComposing || event.keyCode === 229 || event.key === "Process" || event.key === "Dead") return null;
  if (event.getModifierState("AltGraph")) return null;
  if (MODIFIER_KEYS.has(event.key)) return null;
  // Off macOS the Windows/Super key has no slot in the binding model.
  if (!isMac && event.metaKey) return null;

  const mod = isMac ? event.metaKey : event.ctrlKey;
  const control = isMac && event.ctrlKey;
  // A chord needs a non-Shift modifier; a bare or Shift-only key is ordinary typing.
  if (!mod && !control && !event.altKey) return null;

  let key: string;
  if (/^Key[A-Z]$/.test(event.code)) key = event.code.slice(3).toLowerCase();
  else if (/^Digit[0-9]$/.test(event.code)) key = event.code.slice(5);
  else if (CODE_KEYS[event.code]) key = CODE_KEYS[event.code];
  else if (event.key.length === 1) key = event.key.toLowerCase();
  else key = event.key;
  if (!key || key === "Unidentified") return null;

  const binding: { key: string; mod?: boolean; control?: boolean; alt?: boolean; shift?: boolean } = { key };
  if (mod) binding.mod = true;
  if (control) binding.control = true;
  if (event.altKey) binding.alt = true;
  if (event.shiftKey) binding.shift = true;
  return binding;
}

/** Off macOS `mod` is Ctrl, and a bare Ctrl+letter goes to the shell while a terminal has focus. */
function isBareControlLetter(binding: ShortcutBinding, isMac: boolean): boolean {
  if (isMac) return false;
  return Boolean((binding.mod || binding.control) && !binding.alt && !binding.shift && /^[a-zA-Z]$/.test(binding.key));
}

function conflictOwner(conflict: ShortcutConflict): string {
  return conflict.kind === "action" ? conflict.title : conflict.chord.title;
}

type RowNotice = { readonly id: ShortcutActionId; readonly kind: "conflict" | "warning"; readonly message: string };

export function ShortcutsSection({
  isMac,
  overrides,
  preset = "default",
  onChangePreset,
  onChangeOverride,
  onResetAll,
}: ShortcutsSectionProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedGroup, setSelectedGroup] = useState<string>("all");
  const [recordingId, setRecordingId] = useState<ShortcutActionId | null>(null);
  const [notice, setNotice] = useState<RowNotice | null>(null);

  const normalizedQuery = searchQuery.trim().toLowerCase();
  const hasOverrides = Object.keys(overrides).length > 0;

  const filteredShortcuts = SHORTCUTS.filter((shortcut) => {
    if (selectedGroup !== "all" && shortcut.group !== selectedGroup) return false;
    if (!normalizedQuery) return true;
    return (
      shortcut.title.toLowerCase().includes(normalizedQuery) ||
      shortcut.group.toLowerCase().includes(normalizedQuery) ||
      shortcut.id.toLowerCase().includes(normalizedQuery)
    );
  });

  const startRecording = (id: ShortcutActionId) => {
    setNotice(null);
    setRecordingId(id);
  };

  const handleRecorderKeyDown = (id: ShortcutActionId, event: ReactKeyboardEvent<HTMLInputElement>) => {
    // The recorder owns every keystroke it receives: nothing reaches the dialog's Escape handler
    // or the browser's default action while a chord is being captured.
    event.preventDefault();
    event.stopPropagation();
    const native = event.nativeEvent;
    if (native.key === "Escape" && !native.metaKey && !native.ctrlKey && !native.altKey && !native.shiftKey) {
      setRecordingId(null);
      return;
    }
    const binding = chordFromEvent(native, isMac);
    if (!binding) return;

    const conflict = findConflict(id, binding, overrides, isMac, preset);
    if (conflict) {
      setNotice({
        id,
        kind: "conflict",
        message: `${formatBindingLabel(binding, isMac)} is already used by ${conflictOwner(conflict)}.`,
      });
      return;
    }
    onChangeOverride(id, binding);
    setRecordingId(null);
    setNotice(
      isBareControlLetter(binding, isMac)
        ? {
            id,
            kind: "warning",
            message: `${formatBindingLabel(binding, isMac)} is a bare Ctrl+letter: while a terminal has focus it goes to the shell instead of this action.`,
          }
        : null,
    );
  };

  return (
    <section aria-labelledby="settings-shortcuts-heading">
      <SettingsHeading
        icon={<Keyboard />}
        title="Keyboard Shortcuts"
        description="Only registered modifier chords are intercepted by the shell. Ordinary terminal typing and Ctrl-C remain with the terminal."
      />
      <h2 id="settings-shortcuts-heading" className="sr-only">Keyboard Shortcuts</h2>

      {!isMac && onChangePreset ? (
        <div className="mb-3">
          <Label htmlFor="shortcut-preset" className="mb-1.5 block text-[11px] font-medium">
            Shortcut preset
          </Label>
          <select
            id="shortcut-preset"
            value={preset}
            onChange={(event) => {
              const next = event.target.value === "terminal-first" ? "terminal-first" : "default";
              setRecordingId(null);
              setNotice(null);
              onChangePreset(next);
            }}
            className="h-8 w-full rounded-md border border-input bg-background px-3 text-[11px] focus:outline-none focus:ring-1 focus:ring-ring"
          >
            <option value="default">Default</option>
            <option value="terminal-first">Terminal first</option>
          </select>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {PRESET_DESCRIPTIONS[preset]} Your custom bindings apply on top of either preset.
          </p>
        </div>
      ) : null}

      <div className="mb-3 space-y-2">
        <div className="flex items-center gap-2">
          <div className="relative flex flex-1 items-center">
            <Search className="pointer-events-none absolute left-2.5 size-3.5 text-muted-foreground z-10" />
            <Input
              type="search"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search shortcuts (e.g. tab, split, zoom, workspace)..."
              aria-label="Search keyboard shortcuts"
              className="h-8 bg-background pl-8 pr-3 text-[11px]"
            />
          </div>
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={!hasOverrides}
            onClick={() => {
              setRecordingId(null);
              setNotice(null);
              onResetAll();
            }}
            className="h-8 px-2 text-[11px]"
          >
            Reset all
          </Button>
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
            const label = shortcutLabel(shortcut.id, isMac, overrides, preset);
            const aliases = shortcutAliasesLabels(shortcut.id, isMac, overrides, preset);
            const overridden = Object.prototype.hasOwnProperty.call(overrides, shortcut.id);
            const recording = recordingId === shortcut.id;
            const rowNotice = notice?.id === shortcut.id ? notice : null;
            return (
              <div
                key={shortcut.id}
                data-shortcut-id={shortcut.id}
                className="min-h-10 border-b border-border px-1 last:border-b-0"
              >
                <div className="flex min-h-10 items-center justify-between gap-4">
                  <div className="min-w-0 py-2">
                    <div className="truncate text-[13px] font-medium text-foreground">{shortcut.title}</div>
                    <div className="text-[11px] text-muted-foreground">{shortcut.group}</div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1.5">
                    {recording ? (
                      <Input
                        autoFocus
                        readOnly
                        data-shortcut-recorder=""
                        aria-label={`Record shortcut for ${shortcut.title}`}
                        value=""
                        placeholder="Press a chord, Esc to cancel"
                        onKeyDown={(event) => handleRecorderKeyDown(shortcut.id, event)}
                        onBlur={() => setRecordingId(null)}
                        className="h-6 w-44 bg-background px-2 text-[11px]"
                      />
                    ) : label ? (
                      <>
                        <kbd className="rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[11px] text-foreground">
                          {label}
                        </kbd>
                        {aliases.map((alias) => (
                          <kbd
                            key={alias}
                            className="rounded border border-border/60 bg-muted/40 px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground"
                          >
                            {alias}
                          </kbd>
                        ))}
                      </>
                    ) : (
                      <span className="text-[11px] italic text-muted-foreground">Unbound</span>
                    )}
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      aria-label={`Edit shortcut for ${shortcut.title}`}
                      onClick={() => startRecording(shortcut.id)}
                      className="h-6 px-1.5 text-[11px]"
                    >
                      Edit
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      aria-label={`Unbind shortcut for ${shortcut.title}`}
                      disabled={!label}
                      onClick={() => {
                        setRecordingId(null);
                        setNotice(null);
                        onChangeOverride(shortcut.id, null);
                      }}
                      className="h-6 px-1.5 text-[11px]"
                    >
                      Unbind
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      aria-label={`Reset shortcut for ${shortcut.title}`}
                      disabled={!overridden}
                      onClick={() => {
                        setRecordingId(null);
                        setNotice(null);
                        onChangeOverride(shortcut.id, undefined);
                      }}
                      className="h-6 px-1.5 text-[11px]"
                    >
                      Reset
                    </Button>
                  </div>
                </div>
                {rowNotice ? (
                  <div
                    role={rowNotice.kind === "conflict" ? "alert" : "status"}
                    className={`pb-2 text-[11px] ${rowNotice.kind === "conflict" ? "text-destructive" : "text-muted-foreground"}`}
                  >
                    {rowNotice.message}
                  </div>
                ) : null}
              </div>
            );
          })
        ) : (
          <div className="py-8 text-center text-[12px] text-muted-foreground">
            No keyboard shortcuts matching &quot;{searchQuery}&quot;.
          </div>
        )}
      </div>
    </section>
  );
}

export const ShortcutSettings = ShortcutsSection;
