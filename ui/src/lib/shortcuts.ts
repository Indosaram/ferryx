import { useEffect } from "react";

export type ShortcutActionId =
  | "tab.newTerminal"
  | "tab.close"
  | "tab.next"
  | "tab.previous"
  | "terminal.splitRight"
  | "terminal.splitDown"
  | "terminal.unsplit"
  | "commandPalette.open";

type ShortcutBinding = {
  key: string;
  mod?: boolean;
  control?: boolean;
  alt?: boolean;
  shift?: boolean;
};

export type ShortcutDefinition = {
  id: ShortcutActionId;
  title: string;
  group: "Tabs" | "Terminal Panes" | "Global";
  binding: ShortcutBinding;
  source: "original" | "orca-lite";
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
    id: "tab.close",
    title: "Close active tab",
    group: "Tabs",
    binding: { key: "w", mod: true },
    source: "original",
  },
  {
    id: "tab.next",
    title: "Next terminal tab",
    group: "Tabs",
    binding: { key: "PageDown", control: true },
    source: "original",
  },
  {
    id: "tab.previous",
    title: "Previous terminal tab",
    group: "Tabs",
    binding: { key: "PageUp", control: true },
    source: "original",
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
    source: "orca-lite",
  },
  {
    id: "commandPalette.open",
    title: "Open command palette",
    group: "Global",
    binding: { key: "k", mod: true },
    source: "orca-lite",
  },
] as const;

const SHORTCUT_BY_ID = new Map<ShortcutActionId, ShortcutDefinition>(SHORTCUTS.map((shortcut) => [shortcut.id, shortcut]));

export function shortcutLabel(id: ShortcutActionId, isMac = detectMacPlatform()) {
  const shortcut = SHORTCUT_BY_ID.get(id);
  if (!shortcut) return "";
  const binding = shortcut.binding;
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

export function useShortcuts(
  handlers: Partial<Record<ShortcutActionId, () => void>>,
  options: { isMac?: boolean } = {},
) {
  const isMac = options.isMac ?? detectMacPlatform();

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      for (const shortcut of SHORTCUTS) {
        const handler = handlers[shortcut.id];
        if (!handler || !matchesBinding(event, shortcut.binding, isMac)) continue;
        if (isEditableTarget(event.target) && shortcut.id !== "commandPalette.open") continue;
        event.preventDefault();
        handler();
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handlers, isMac]);
}

export function isMacShortcutPlatform() {
  return detectMacPlatform();
}

function matchesBinding(event: KeyboardEvent, binding: ShortcutBinding, isMac: boolean) {
  const expectedMeta = Boolean(binding.mod && isMac);
  const expectedControl = Boolean(binding.control || (binding.mod && !isMac));
  return (
    normalizeKey(event.key) === normalizeKey(binding.key) &&
    event.metaKey === expectedMeta &&
    event.ctrlKey === expectedControl &&
    event.altKey === Boolean(binding.alt) &&
    event.shiftKey === Boolean(binding.shift)
  );
}

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
}

function normalizeKey(key: string) {
  return key.length === 1 ? key.toLowerCase() : key;
}

function displayKey(key: string) {
  if (key === "PageDown") return "PgDn";
  if (key === "PageUp") return "PgUp";
  return key.length === 1 ? key.toUpperCase() : key;
}

function detectMacPlatform() {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform);
}
