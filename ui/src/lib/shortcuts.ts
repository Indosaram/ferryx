import { useEffect } from "react";

export type ShortcutActionId =
  | "tab.newTerminal"
  | "tab.newBrowser"
  | "tab.close"
  | "tab.next"
  | "tab.previous"
  | "tab.select1"
  | "tab.select2"
  | "tab.select3"
  | "tab.select4"
  | "tab.select5"
  | "tab.select6"
  | "tab.select7"
  | "tab.select8"
  | "tab.select9"
  | "workspace.select1"
  | "workspace.select2"
  | "workspace.select3"
  | "workspace.select4"
  | "workspace.select5"
  | "workspace.select6"
  | "workspace.select7"
  | "workspace.select8"
  | "workspace.select9"
  | "browser.focusAddress"
  | "browser.reload"
  | "browser.back"
  | "browser.forward"
  | "browser.find"
  | "terminal.splitRight"
  | "terminal.splitDown"
  | "terminal.unsplit"
  | "terminal.focusNext"
  | "terminal.focusPrevious"
  | "terminal.search"
  | "sidebar.left.toggle"
  | "commandPalette.open"
  | "settings.toggle"
  | "zoom.in"
  | "zoom.out"
  | "zoom.reset";

export type ShortcutBinding = {
  key: string;
  mod?: boolean;
  control?: boolean;
  alt?: boolean;
  shift?: boolean;
};

export type ShortcutDefinition = {
  id: ShortcutActionId;
  title: string;
  group: "Tabs" | "Terminal Panes" | "Global" | "Workspaces" | "View";
  binding: ShortcutBinding;
  aliases?: readonly ShortcutBinding[];
  source: "original" | "ferryx";
};

export const SHORTCUTS: readonly ShortcutDefinition[] = [
  {
    id: "tab.newTerminal",
    title: "New terminal tab",
    group: "Tabs",
    binding: { key: "t", mod: true },
    source: "original",
  },
  {
    id: "tab.newBrowser",
    title: "New browser tab",
    group: "Tabs",
    binding: { key: "b", mod: true, shift: true },
    source: "original",
  },
  {
    id: "tab.close",
    title: "Close active tab",
    group: "Tabs",
    binding: { key: "w", mod: true },
    source: "original",
  },
  {
    id: "browser.focusAddress",
    title: "Focus browser address bar",
    group: "Tabs",
    binding: { key: "l", mod: true },
    source: "ferryx",
  },
  {
    id: "browser.reload",
    title: "Reload browser tab",
    group: "Tabs",
    binding: { key: "r", mod: true },
    source: "ferryx",
  },
  {
    id: "browser.back",
    title: "Browser back",
    group: "Tabs",
    binding: { key: "[", mod: true },
    aliases: [{ key: "ArrowLeft", alt: true }],
    source: "ferryx",
  },
  {
    id: "browser.forward",
    title: "Browser forward",
    group: "Tabs",
    binding: { key: "]", mod: true },
    aliases: [{ key: "ArrowRight", alt: true }],
    source: "ferryx",
  },
  {
    id: "browser.find",
    title: "Find in browser page",
    group: "Tabs",
    binding: { key: "f", mod: true },
    source: "ferryx",
  },
  {
    id: "tab.next",
    title: "Next terminal tab",
    group: "Tabs",
    binding: { key: "]", mod: true, shift: true },
    aliases: [
      { key: "PageDown", control: true },
      { key: "Tab", control: true },
    ],
    source: "original",
  },
  {
    id: "tab.previous",
    title: "Previous terminal tab",
    group: "Tabs",
    binding: { key: "[", mod: true, shift: true },
    aliases: [
      { key: "PageUp", control: true },
      { key: "Tab", control: true, shift: true },
    ],
    source: "original",
  },
  {
    id: "tab.select1",
    title: "Select terminal tab 1",
    group: "Tabs",
    binding: { key: "1", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select2",
    title: "Select terminal tab 2",
    group: "Tabs",
    binding: { key: "2", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select3",
    title: "Select terminal tab 3",
    group: "Tabs",
    binding: { key: "3", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select4",
    title: "Select terminal tab 4",
    group: "Tabs",
    binding: { key: "4", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select5",
    title: "Select terminal tab 5",
    group: "Tabs",
    binding: { key: "5", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select6",
    title: "Select terminal tab 6",
    group: "Tabs",
    binding: { key: "6", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select7",
    title: "Select terminal tab 7",
    group: "Tabs",
    binding: { key: "7", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select8",
    title: "Select terminal tab 8",
    group: "Tabs",
    binding: { key: "8", control: true },
    source: "ferryx",
  },
  {
    id: "tab.select9",
    title: "Select terminal tab 9",
    group: "Tabs",
    binding: { key: "9", control: true },
    source: "ferryx",
  },
  {
    id: "workspace.select1",
    title: "Select workspace 1",
    group: "Workspaces",
    binding: { key: "1", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select2",
    title: "Select workspace 2",
    group: "Workspaces",
    binding: { key: "2", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select3",
    title: "Select workspace 3",
    group: "Workspaces",
    binding: { key: "3", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select4",
    title: "Select workspace 4",
    group: "Workspaces",
    binding: { key: "4", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select5",
    title: "Select workspace 5",
    group: "Workspaces",
    binding: { key: "5", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select6",
    title: "Select workspace 6",
    group: "Workspaces",
    binding: { key: "6", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select7",
    title: "Select workspace 7",
    group: "Workspaces",
    binding: { key: "7", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select8",
    title: "Select workspace 8",
    group: "Workspaces",
    binding: { key: "8", mod: true },
    source: "ferryx",
  },
  {
    id: "workspace.select9",
    title: "Select workspace 9",
    group: "Workspaces",
    binding: { key: "9", mod: true },
    source: "ferryx",
  },
  {
    id: "terminal.splitRight",
    title: "Split terminal right",
    group: "Terminal Panes",
    binding: { key: "d", mod: true },
    source: "original",
  },
  {
    id: "terminal.splitDown",
    title: "Split terminal down",
    group: "Terminal Panes",
    binding: { key: "d", mod: true, shift: true },
    source: "original",
  },
  {
    id: "terminal.unsplit",
    title: "Close split view",
    group: "Terminal Panes",
    binding: { key: "d", mod: true, alt: true },
    aliases: [{ key: "w", mod: true, shift: true }],
    source: "ferryx",
  },
  {
    id: "terminal.focusNext",
    title: "Focus next terminal pane",
    group: "Terminal Panes",
    binding: { key: "]", mod: true },
    source: "original",
  },
  {
    id: "terminal.focusPrevious",
    title: "Focus previous terminal pane",
    group: "Terminal Panes",
    binding: { key: "[", mod: true },
    source: "original",
  },
  {
    id: "terminal.search",
    title: "Find in terminal",
    group: "Terminal Panes",
    binding: { key: "f", mod: true },
    source: "original",
  },
  {
    id: "sidebar.left.toggle",
    title: "Toggle sidebar",
    group: "Global",
    binding: { key: "b", mod: true },
    source: "original",
  },
  {
    id: "commandPalette.open",
    title: "Open command palette",
    group: "Global",
    binding: { key: "k", mod: true },
    aliases: [{ key: "p", mod: true }],
    source: "ferryx",
  },
  {
    id: "settings.toggle",
    title: "Toggle settings",
    group: "Global",
    binding: { key: ",", mod: true },
    source: "original",
  },
  {
    id: "zoom.in",
    title: "Zoom in terminal",
    group: "View",
    binding: { key: "=", mod: true },
    aliases: [{ key: "+", mod: true, shift: true }],
    source: "original",
  },
  {
    id: "zoom.out",
    title: "Zoom out terminal",
    group: "View",
    binding: { key: "-", mod: true },
    source: "original",
  },
  {
    id: "zoom.reset",
    title: "Reset terminal zoom",
    group: "View",
    binding: { key: "0", mod: true },
    source: "original",
  },
] as const;

const SHORTCUT_BY_ID = new Map<ShortcutActionId, ShortcutDefinition>(SHORTCUTS.map((shortcut) => [shortcut.id, shortcut]));

export function formatBindingLabel(binding: ShortcutBinding, isMac = detectMacPlatform()): string {
  const key = displayKey(binding.key);
  if (isMac) {
    return `${binding.mod ? "⌘" : ""}${binding.control ? "⌃" : ""}${binding.alt ? "⌥" : ""}${binding.shift ? "⇧" : ""}${key}`;
  }

  return [
    binding.mod || binding.control ? "Ctrl" : null,
    binding.alt ? "Alt" : null,
    binding.shift ? "Shift" : null,
    key,
  ]
    .filter(Boolean)
    .join("+");
}

export function shortcutLabel(id: ShortcutActionId, isMac = detectMacPlatform()): string {
  const shortcut = SHORTCUT_BY_ID.get(id);
  if (!shortcut) return "";
  return formatBindingLabel(shortcut.binding, isMac);
}

export function shortcutAliasesLabels(id: ShortcutActionId, isMac = detectMacPlatform()): string[] {
  const shortcut = SHORTCUT_BY_ID.get(id);
  if (!shortcut?.aliases?.length) return [];
  return shortcut.aliases.map((alias) => formatBindingLabel(alias, isMac));
}

export function useShortcuts(
  handlers: Partial<Record<ShortcutActionId, () => void>>,
  options: { isMac?: boolean } = {},
) {
  const isMac = options.isMac ?? detectMacPlatform();

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      for (const shortcut of SHORTCUTS) {
        const handler = handlers[shortcut.id];
        if (!handler) continue;

        const matches =
          matchesBinding(event, shortcut.binding, isMac) ||
          Boolean(shortcut.aliases?.some((alias) => matchesBinding(event, alias, isMac)));

        if (!matches) continue;
        if (
          isEditableTarget(event.target) &&
          !isTerminalTarget(event.target) &&
          shortcut.id !== "commandPalette.open" &&
          !shortcut.id.startsWith("browser.") &&
          shortcut.id !== "settings.toggle"
        )
          continue;
        event.preventDefault();
        handler();
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [handlers, isMac]);
}

export function isMacShortcutPlatform() {
  return detectMacPlatform();
}

export function matchesBinding(event: KeyboardEvent, binding: ShortcutBinding, isMac: boolean): boolean {
  // Keydowns owned by an active IME composition must never match an app chord:
  // the physical `code` still reflects the shortcut key, but the keystroke belongs
  // to text conversion. WebKit signals this with legacy keyCode 229 and/or a
  // `Process`/`Dead` key value; `isComposing` covers spec-compliant engines.
  if (event.isComposing || event.keyCode === 229 || event.key === "Process" || event.key === "Dead") {
    return false;
  }

  // AltGr (Windows/Linux) reports ctrlKey+altKey while producing a typed glyph.
  // getModifierState("AltGraph") owns the keystroke for text entry, so it must
  // never match an app chord even though a real Ctrl+Alt chord has it inactive.
  if (event.getModifierState("AltGraph")) {
    return false;
  }

  const expectedMeta = Boolean(binding.mod && isMac);
  const expectedControl = Boolean(binding.control || (binding.mod && !isMac));
  const expectedAlt = Boolean(binding.alt);
  const expectedShift = Boolean(binding.shift);

  if (event.metaKey !== expectedMeta) return false;
  if (event.ctrlKey !== expectedControl) return false;
  if (event.altKey !== expectedAlt) return false;
  if (event.shiftKey !== expectedShift) return false;

  const key = binding.key;
  const ekey = event.key;
  const ecode = event.code;

  if (key.toLowerCase() === "tab") {
    return ecode === "Tab" || ekey === "Tab";
  }
  if (key === "PageDown") {
    return ecode === "PageDown" || ekey === "PageDown";
  }
  if (key === "PageUp") {
    return ecode === "PageUp" || ekey === "PageUp";
  }
  if (key === "]") {
    return ecode === "BracketRight" || ekey === "]" || ekey === "}";
  }
  if (key === "[") {
    return ecode === "BracketLeft" || ekey === "[" || ekey === "{";
  }
  if (key === "=" || key === "+") {
    return ecode === "Equal" || ecode === "NumpadAdd" || ekey === "=" || ekey === "+";
  }
  if (key === "-") {
    return ecode === "Minus" || ecode === "NumpadSubtract" || ekey === "-" || ekey === "_";
  }
  if (key === ",") {
    return ecode === "Comma" || ekey === "," || ekey === "<";
  }
  if (/^[0-9]$/.test(key)) {
    return ecode === `Digit${key}` || ecode === `Numpad${key}` || ekey === key;
  }
  if (key.length === 1 && /^[a-zA-Z]$/.test(key)) {
    return ecode === `Key${key.toUpperCase()}` || ekey.toLowerCase() === key.toLowerCase();
  }

  return normalizeKey(ekey) === normalizeKey(key);
}

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
}

function isTerminalTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && target.closest(".terminal-host") !== null;
}

function normalizeKey(key: string) {
  return key.length === 1 ? key.toLowerCase() : key;
}

function displayKey(key: string) {
  if (key === "PageDown") return "PgDn";
  if (key === "PageUp") return "PgUp";
  if (key === "Tab") return "Tab";
  return key.length === 1 ? key.toUpperCase() : key;
}

function detectMacPlatform() {
  if (typeof navigator === "undefined") return false;
  if (/Mac|iPhone|iPad|iPod/.test(navigator.platform)) return true;
  if (typeof navigator.userAgent === "string" && /Macintosh|Mac OS X/.test(navigator.userAgent)) return true;
  if (typeof process !== "undefined" && process.platform === "darwin") return true;
  return false;
}
