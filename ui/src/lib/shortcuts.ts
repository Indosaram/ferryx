import { useEffect } from "react";

export type ShortcutActionId =
  | "tab.newTerminal"
  | "tab.close"
  | "tab.next"
  | "tab.previous"
  | "tab.select1"
  | "tab.select2"
  | "tab.select3"
  | "tab.select4"
  | "workspace.select1"
  | "workspace.select2"
  | "workspace.select3"
  | "workspace.select4"
  | "terminal.splitRight"
  | "terminal.splitDown"
  | "terminal.unsplit"
  | "sidebar.left.toggle"
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
  group: "Tabs" | "Terminal Panes" | "Global" | "Workspaces";
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
    id: "tab.select1",
    title: "Select terminal tab 1",
    group: "Tabs",
    binding: { key: "1", control: true },
    source: "orca-lite",
  },
  {
    id: "tab.select2",
    title: "Select terminal tab 2",
    group: "Tabs",
    binding: { key: "2", control: true },
    source: "orca-lite",
  },
  {
    id: "tab.select3",
    title: "Select terminal tab 3",
    group: "Tabs",
    binding: { key: "3", control: true },
    source: "orca-lite",
  },
  {
    id: "tab.select4",
    title: "Select terminal tab 4",
    group: "Tabs",
    binding: { key: "4", control: true },
    source: "orca-lite",
  },
  {
    id: "workspace.select1",
    title: "Select workspace 1",
    group: "Workspaces",
    binding: { key: "1", mod: true },
    source: "orca-lite",
  },
  {
    id: "workspace.select2",
    title: "Select workspace 2",
    group: "Workspaces",
    binding: { key: "2", mod: true },
    source: "orca-lite",
  },
  {
    id: "workspace.select3",
    title: "Select workspace 3",
    group: "Workspaces",
    binding: { key: "3", mod: true },
    source: "orca-lite",
  },
  {
    id: "workspace.select4",
    title: "Select workspace 4",
    group: "Workspaces",
    binding: { key: "4", mod: true },
    source: "orca-lite",
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
        if (isEditableTarget(event.target) && !isTerminalTarget(event.target) && shortcut.id !== "commandPalette.open") continue;
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

function matchesBinding(event: KeyboardEvent, binding: ShortcutBinding, isMac: boolean) {
  const expectedMeta = Boolean(binding.mod && isMac);
  const expectedControl = Boolean(binding.control || (binding.mod && !isMac));
  const matchesKey =
    (binding.key.length === 1 && (event.code === `Key${binding.key.toUpperCase()}` || event.code === `Digit${binding.key}`)) ||
    normalizeKey(event.key) === normalizeKey(binding.key);

  return (
    matchesKey &&
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

function isTerminalTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && target.closest(".terminal-host") !== null;
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
  if (/Mac|iPhone|iPad|iPod/.test(navigator.platform)) return true;
  if (typeof navigator.userAgent === "string" && /Macintosh|Mac OS X/.test(navigator.userAgent)) return true;
  if (typeof process !== "undefined" && process.platform === "darwin") return true;
  return false;
}
