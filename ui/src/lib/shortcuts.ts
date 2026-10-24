import { useEffect } from "react";
import { classifyNavigatorPlatform, getRuntimeProcessPlatform } from "./platform";
import { switchDebug } from "./switchDebug";
import { shortcutChord, shortcutContext, traceShortcutAction } from "./shortcutDiagnostics";

// TEMPORARY-DIAGNOSTIC: distinguish concurrent hooks and StrictMode remounts.
let shortcutRegistration = 0;

export type ShortcutActionId =
  | "tab.newTerminal"
  | "tab.newBrowser"
  | "tab.reopenClosed"
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
  | "browser.hardReload"
  | "browser.back"
  | "browser.forward"
  | "browser.find"
  | "terminal.splitRight"
  | "terminal.splitDown"
  | "terminal.unsplit"
  | "terminal.focusNext"
  | "terminal.focusPrevious"
  | "terminal.focusLeft"
  | "terminal.focusRight"
  | "terminal.focusUp"
  | "terminal.focusDown"
  | "terminal.search"
  | "terminal.copyMode"
  | "sidebar.left.toggle"
  | "project.add"
  | "commandPalette.open"
  | "settings.toggle"
  | "notifications.toggle"
  | "zoom.in"
  | "zoom.out"
  | "zoom.reset";

export type ShortcutBinding = {
  key: string;
  mod?: boolean;
  control?: boolean;
  alt?: boolean;
  shift?: boolean;
  readonly nonMac?: Partial<Omit<ShortcutBinding, "nonMac">>;
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
    id: "tab.reopenClosed",
    title: "Reopen closed tab",
    group: "Tabs",
    binding: { key: "t", mod: true, shift: true },
    source: "ferryx",
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
    id: "browser.hardReload",
    title: "Reload browser tab bypassing the cache",
    group: "Tabs",
    binding: { key: "r", mod: true, shift: true },
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
    binding: { key: "1", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select2",
    title: "Select workspace 2",
    group: "Workspaces",
    binding: { key: "2", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select3",
    title: "Select workspace 3",
    group: "Workspaces",
    binding: { key: "3", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select4",
    title: "Select workspace 4",
    group: "Workspaces",
    binding: { key: "4", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select5",
    title: "Select workspace 5",
    group: "Workspaces",
    binding: { key: "5", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select6",
    title: "Select workspace 6",
    group: "Workspaces",
    binding: { key: "6", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select7",
    title: "Select workspace 7",
    group: "Workspaces",
    binding: { key: "7", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select8",
    title: "Select workspace 8",
    group: "Workspaces",
    binding: { key: "8", mod: true, nonMac: { mod: false, alt: true } },
    source: "ferryx",
  },
  {
    id: "workspace.select9",
    title: "Select workspace 9",
    group: "Workspaces",
    binding: { key: "9", mod: true, nonMac: { mod: false, alt: true } },
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
    id: "terminal.focusLeft",
    title: "Focus terminal pane to the left",
    group: "Terminal Panes",
    binding: { key: "ArrowLeft", mod: true, alt: true },
    source: "ferryx",
  },
  {
    id: "terminal.focusRight",
    title: "Focus terminal pane to the right",
    group: "Terminal Panes",
    binding: { key: "ArrowRight", mod: true, alt: true },
    source: "ferryx",
  },
  {
    id: "terminal.focusUp",
    title: "Focus terminal pane above",
    group: "Terminal Panes",
    binding: { key: "ArrowUp", mod: true, alt: true },
    source: "ferryx",
  },
  {
    id: "terminal.focusDown",
    title: "Focus terminal pane below",
    group: "Terminal Panes",
    binding: { key: "ArrowDown", mod: true, alt: true },
    source: "ferryx",
  },
  {
    id: "terminal.search",
    title: "Find in terminal",
    group: "Terminal Panes",
    binding: { key: "f", mod: true },
    source: "original",
  },
  {
    id: "terminal.copyMode",
    title: "Toggle terminal copy mode",
    group: "Terminal Panes",
    // Mod+Shift+Space (Cmd on macOS, Ctrl elsewhere); the earlier Mod+Shift+X chord stays an alias.
    binding: { key: "Space", mod: true, shift: true },
    aliases: [{ key: "x", mod: true, shift: true }],
    source: "ferryx",
  },
  {
    id: "sidebar.left.toggle",
    title: "Toggle sidebar",
    group: "Global",
    binding: { key: "b", mod: true },
    source: "original",
  },
  {
    id: "project.add",
    title: "Add project",
    group: "Global",
    binding: { key: "o", mod: true },
    aliases: [{ key: "n", mod: true }],
    source: "ferryx",
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
    id: "notifications.toggle",
    title: "Toggle notification center",
    group: "Global",
    binding: { key: "n", mod: true, shift: true },
    aliases: [{ key: "i", mod: true, shift: true }],
    source: "ferryx",
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

/** Sparse user remaps: a binding replaces the primary chord and drops aliases; `null` unbinds. */
export type ShortcutOverrideMap = Partial<Record<ShortcutActionId, ShortcutBinding | null>>;

const NO_OVERRIDES: ShortcutOverrideMap = Object.freeze({});

/**
 * Named binding sets picked in Settings. `default` is the registry above. `terminal-first`
 * (Windows/Linux only; macOS always resolves `default`) leaves Ctrl+digit, Ctrl+Tab,
 * Ctrl+Shift+Tab and Ctrl+PageUp/PageDown to the terminal and moves those actions to Alt-based
 * chords under the same action ids. Sparse user overrides apply on top of either preset.
 */
export type ShortcutPreset = "default" | "terminal-first";

export const SHORTCUT_PRESETS: readonly ShortcutPreset[] = ["default", "terminal-first"];

type PresetBindings = { readonly binding: ShortcutBinding; readonly aliases?: readonly ShortcutBinding[] };

const PRESET_DIGITS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"] as const;

/**
 * The `terminal-first` replacements; ids not listed keep their default bindings. The Ctrl chords
 * stay as aliases so they still work outside a terminal; inside `.terminal-host` the yield
 * predicate hands them to the PTY instead.
 */
export const TERMINAL_FIRST_BINDINGS: Readonly<Partial<Record<ShortcutActionId, PresetBindings>>> = {
  ...Object.fromEntries(PRESET_DIGITS.map((digit) => [
    `tab.select${digit}`,
    { binding: { key: digit, alt: true }, aliases: [{ key: digit, control: true }] },
  ])),
  // Alt+digit now selects tabs, so workspaces move to Alt+Shift+digit.
  ...Object.fromEntries(PRESET_DIGITS.map((digit) => [`workspace.select${digit}`, { binding: { key: digit, alt: true, shift: true } }])),
  "tab.next": {
    binding: { key: "PageDown", alt: true },
    aliases: [{ key: "]", mod: true, shift: true }, { key: "PageDown", control: true }, { key: "Tab", control: true }],
  },
  "tab.previous": {
    binding: { key: "PageUp", alt: true },
    aliases: [{ key: "[", mod: true, shift: true }, { key: "PageUp", control: true }, { key: "Tab", control: true, shift: true }],
  },
};

/** The preset's replacement table, or null when the platform/preset keeps the defaults. */
export function shortcutPresetLayer(
  preset: ShortcutPreset,
  isMac: boolean,
): Readonly<Partial<Record<ShortcutActionId, PresetBindings>>> | null {
  if (isMac || preset !== "terminal-first") return null;
  return TERMINAL_FIRST_BINDINGS;
}

/** The live registry: defaults with the preset, then user overrides, applied and unbound actions removed. */
export function resolvedShortcuts(
  overrides: ShortcutOverrideMap = NO_OVERRIDES,
  preset: ShortcutPreset = "default",
  isMac = detectMacPlatform(),
): readonly ShortcutDefinition[] {
  const layer = shortcutPresetLayer(preset, isMac);
  if (!layer && Object.keys(overrides).length === 0) return SHORTCUTS;
  const resolved: ShortcutDefinition[] = [];
  for (const original of SHORTCUTS) {
    const replacement = layer?.[original.id];
    let shortcut: ShortcutDefinition = original;
    if (replacement) {
      const { aliases: _presetAliases, ...base } = original;
      shortcut = replacement.aliases
        ? { ...base, binding: replacement.binding, aliases: replacement.aliases }
        : { ...base, binding: replacement.binding };
    }
    if (!Object.prototype.hasOwnProperty.call(overrides, shortcut.id)) {
      resolved.push(shortcut);
      continue;
    }
    const binding = overrides[shortcut.id];
    if (!binding) continue;
    const { aliases: _aliases, ...rest } = shortcut;
    resolved.push({ ...rest, binding });
  }
  return resolved;
}

function resolvedShortcut(
  id: ShortcutActionId,
  overrides: ShortcutOverrideMap,
  preset: ShortcutPreset,
  isMac: boolean,
): ShortcutDefinition | undefined {
  return resolvedShortcuts(overrides, preset, isMac).find((shortcut) => shortcut.id === id);
}

export type ReservedChord = {
  readonly id: string;
  readonly title: string;
  readonly binding: ShortcutBinding;
  readonly platform: "mac" | "nonMac" | "all";
};

/**
 * Chords no action may be remapped onto: the terminal clipboard chords handled in
 * NativeTerminalPane (isPasteShortcut / isCopyShortcut) and the predefined macOS app-menu
 * items (Quit, Hide, Minimize). These stay fixed and are not themselves remappable.
 */
export const RESERVED_CHORDS: readonly ReservedChord[] = [
  { id: "clipboard.copy", title: "Copy", binding: { key: "c", mod: true }, platform: "mac" },
  { id: "clipboard.paste", title: "Paste", binding: { key: "v", mod: true }, platform: "mac" },
  { id: "clipboard.pasteShift", title: "Paste", binding: { key: "v", mod: true, shift: true }, platform: "mac" },
  { id: "clipboard.copyTerminal", title: "Copy", binding: { key: "c", control: true, shift: true }, platform: "all" },
  { id: "clipboard.pasteTerminal", title: "Paste", binding: { key: "v", control: true }, platform: "nonMac" },
  { id: "clipboard.pasteTerminalShift", title: "Paste", binding: { key: "v", control: true, shift: true }, platform: "nonMac" },
  { id: "clipboard.pasteInsert", title: "Paste", binding: { key: "Insert", shift: true }, platform: "all" },
  { id: "menu.quit", title: "Quit Ferryx", binding: { key: "q", mod: true }, platform: "mac" },
  { id: "menu.hide", title: "Hide Ferryx", binding: { key: "h", mod: true }, platform: "mac" },
  { id: "menu.minimize", title: "Minimize", binding: { key: "m", mod: true }, platform: "mac" },
];

export type ShortcutConflict =
  | { readonly kind: "action"; readonly actionId: ShortcutActionId; readonly title: string }
  | { readonly kind: "reserved"; readonly chord: ReservedChord };

/** Terminal actions App disables while a browser surface owns shortcuts, so they may share browser chords. */
const BROWSER_EXCLUSIVE_TERMINAL_ACTIONS = new Set<ShortcutActionId>(["terminal.focusNext", "terminal.focusPrevious", "terminal.search"]);

function surfacesAreExclusive(a: ShortcutActionId, b: ShortcutActionId): boolean {
  return (a.startsWith("browser.") && BROWSER_EXCLUSIVE_TERMINAL_ACTIONS.has(b))
    || (b.startsWith("browser.") && BROWSER_EXCLUSIVE_TERMINAL_ACTIONS.has(a));
}

function effectiveChord(rawBinding: ShortcutBinding, isMac: boolean): string {
  const binding = resolveBinding(rawBinding, isMac);
  const key = binding.key === "+" ? "=" : normalizeKey(binding.key);
  return [
    binding.mod && isMac ? "meta" : "",
    binding.control || (binding.mod && !isMac) ? "ctrl" : "",
    binding.alt ? "alt" : "",
    binding.shift ? "shift" : "",
    key,
  ].join("|");
}

/**
 * Who already owns `binding` if `actionId` were bound to it: another live action (primary or
 * alias, with `overrides` applied) or a reserved clipboard/menu chord. `null` means it is free.
 */
export function findConflict(
  actionId: ShortcutActionId,
  binding: ShortcutBinding,
  overrides: ShortcutOverrideMap = NO_OVERRIDES,
  isMac = detectMacPlatform(),
  preset: ShortcutPreset = "default",
): ShortcutConflict | null {
  const chord = effectiveChord(binding, isMac);
  const reserved = RESERVED_CHORDS.find((candidate) =>
    (candidate.platform === "all" || (candidate.platform === "mac") === isMac)
    && effectiveChord(candidate.binding, isMac) === chord);
  if (reserved) return { kind: "reserved", chord: reserved };
  for (const shortcut of resolvedShortcuts(overrides, preset, isMac)) {
    if (shortcut.id === actionId || surfacesAreExclusive(actionId, shortcut.id)) continue;
    if ([shortcut.binding, ...(shortcut.aliases ?? [])].some((candidate) => effectiveChord(candidate, isMac) === chord)) {
      return { kind: "action", actionId: shortcut.id, title: shortcut.title };
    }
  }
  return null;
}

export function resolveBinding(binding: ShortcutBinding, isMac: boolean): ShortcutBinding {
  const { nonMac, ...base } = binding;
  return isMac ? base : { ...base, ...nonMac };
}

export function formatBindingLabel(rawBinding: ShortcutBinding, isMac = detectMacPlatform()): string {
  const binding = resolveBinding(rawBinding, isMac);
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

export function shortcutLabel(
  id: ShortcutActionId,
  isMac = detectMacPlatform(),
  overrides: ShortcutOverrideMap = NO_OVERRIDES,
  preset: ShortcutPreset = "default",
): string {
  const shortcut = resolvedShortcut(id, overrides, preset, isMac);
  if (!shortcut) return "";
  return formatBindingLabel(shortcut.binding, isMac);
}

export function shortcutAliasesLabels(
  id: ShortcutActionId,
  isMac = detectMacPlatform(),
  overrides: ShortcutOverrideMap = NO_OVERRIDES,
  preset: ShortcutPreset = "default",
): string[] {
  const shortcut = resolvedShortcut(id, overrides, preset, isMac);
  if (!shortcut?.aliases?.length) return [];
  return shortcut.aliases.map((alias) => formatBindingLabel(alias, isMac));
}

export function useShortcuts(
  handlers: Partial<Record<ShortcutActionId, () => void>>,
  options: { isMac?: boolean; overrides?: ShortcutOverrideMap; preset?: ShortcutPreset } = {},
) {
  const isMac = options.isMac ?? detectMacPlatform();
  const overrides = options.overrides ?? NO_OVERRIDES;
  const preset = options.preset ?? "default";

  useEffect(() => {
    const registration = ++shortcutRegistration;
    const shortcuts = resolvedShortcuts(overrides, preset, isMac);
    switchDebug("shortcut.hook.register", { registration, isMac, ...shortcutContext(),
      enabled: shortcuts.filter(({ id }) => handlers[id]).map(({ id }) => id),
      disabled: shortcuts.filter(({ id }) => !handlers[id]).map(({ id }) => id) });
    const handleKeyDown = (event: KeyboardEvent) => {
      const chord = shortcutChord(event);
      const trace = (stage: string, details: Record<string, unknown> = {}) => {
        if (chord) switchDebug(`shortcut.hook.${stage}`, { registration, isMac, ...chord, ...details });
      };
      trace("receipt");
      // The Settings shortcut recorder captures raw chords; dispatching app actions while the
      // user records (e.g. Mod+, closing Settings, Mod+K opening the palette) would hijack it.
      if (event.target instanceof Element && event.target.closest("[data-shortcut-recorder]")) {
        trace("reject", { reason: "shortcut-recorder" });
        return;
      }
      for (const shortcut of shortcuts) {
        const handler = handlers[shortcut.id];
        if (!handler) {
          trace("reject", { action: shortcut.id, reason: "handler-disabled" });
          continue;
        }

        const matches =
          matchesBinding(event, shortcut.binding, isMac) ||
          Boolean(shortcut.aliases?.some((alias) => matchesBinding(event, alias, isMac)));

        if (!matches) {
          const reason = event.isComposing || event.keyCode === 229 || event.key === "Process" || event.key === "Dead"
            ? "ime" : event.getModifierState("AltGraph") ? "alt-graph" : "binding-mismatch";
          trace("reject", { action: shortcut.id, reason, binding: shortcut.binding, aliases: shortcut.aliases });
          continue;
        }
        trace("match", { action: shortcut.id });
        if (shouldYieldControlLetterToTerminal(event, isMac, preset)) {
          trace("reject", { action: shortcut.id, reason: "terminal-control-letter" });
          continue;
        }
        if (
          isEditableTarget(event.target) &&
          !isTerminalTarget(event.target) &&
          shortcut.id !== "commandPalette.open" &&
          !shortcut.id.startsWith("browser.") &&
          shortcut.id !== "settings.toggle" &&
          shortcut.id !== "notifications.toggle"
        ) {
          trace("reject", { action: shortcut.id, reason: "editable-target" });
          continue;
        }
        event.preventDefault();
        traceShortcutAction(shortcut.id, handler, { registration, source: "keydown" });
        return;
      }
      trace("unhandled");
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      switchDebug("shortcut.hook.unregister", { registration });
    };
  }, [handlers, isMac, overrides, preset]);
}

export function isMacShortcutPlatform() {
  return detectMacPlatform();
}

/** Whether this platform's webview exposes a native cache-bypassing reload: macOS WKWebView
 * `reloadFromOrigin` and Linux WebKitGTK `reload_bypass_cache`. This build carries no WebView2
 * binding, so Windows keeps the hard-reload affordance hidden instead of failing the request. */
export function isHardReloadSupportedPlatform() {
  if (typeof process !== "undefined" && typeof process.platform === "string") {
    return process.platform === "darwin" || process.platform === "linux";
  }
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod|Linux|Android|X11/.test(`${navigator.userAgent ?? ""} ${navigator.platform ?? ""}`);
}

export /**
 * True when `event.key` carries no latin character the matcher could compare against,
 * so the physical `event.code` is the only usable signal.
 *
 * This is the Hangul/IME case: a Korean 2-set layout reports `key: "ㅍ"` for physical V,
 * and without the `code` fallback every shortcut would die while the user is in Hangul mode.
 *
 * It deliberately does NOT hold for a latin remap such as Dvorak, where `key` is already
 * the character the user typed. Falling back to `code` there matches the wrong binding:
 * Dvorak's `,` sits on physical QWERTY-W, so Cmd+, would match `tab.close` (key "w",
 * declared before `settings.toggle`) and close the user's tab instead of opening Settings.
 */
function isLayoutTransparent(eventKey: string): boolean {
  return eventKey.length !== 1 || !/^[a-zA-Z0-9,.\-=[\]<>_+{}]$/.test(eventKey);
}

export function matchesBinding(event: KeyboardEvent, rawBinding: ShortcutBinding, isMac: boolean): boolean {
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

  const binding = resolveBinding(rawBinding, isMac);
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
  if (key === "Space") {
    return ecode === "Space" || ekey === " " || ekey === "Space" || ekey === "Spacebar";
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
    return ekey === "," || ekey === "<" || (isLayoutTransparent(ekey) && ecode === "Comma");
  }
  if (/^[0-9]$/.test(key)) {
    return ecode === `Digit${key}` || ecode === `Numpad${key}` || ekey === key;
  }
  if (key.length === 1 && /^[a-zA-Z]$/.test(key)) {
    return (
      ekey.toLowerCase() === key.toLowerCase() ||
      (isLayoutTransparent(ekey) && ecode === `Key${key.toUpperCase()}`)
    );
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

/**
 * A bare Ctrl+<letter> belongs to the shell's readline namespace on Windows/Linux
 * (Ctrl+W deletes a word, Ctrl+R reverse-searches, Ctrl+A jumps to line start), and
 * off macOS `mod` resolves to Ctrl, so the app must not claim it while the terminal
 * owns focus — the keystroke has to reach the shell instead of being prevented.
 *
 * Only the bare chords yield: a chord carrying another modifier (Ctrl+Shift+*), the
 * non-letter chords (Ctrl+digit, Ctrl+Tab, Ctrl+PageUp/PageDown) and every macOS
 * chord stay claimed, because there `mod` is Cmd and this predicate is never true.
 *
 * The `terminal-first` preset widens this off macOS: Ctrl+digit, Ctrl+Tab, Ctrl+Shift+Tab and
 * Ctrl+PageUp/PageDown also reach the terminal, even when a user override binds them.
 */
export function shouldYieldControlLetterToTerminal(
  event: KeyboardEvent,
  isMac: boolean,
  preset: ShortcutPreset = "default",
): boolean {
  if (isMac) return false;
  if (!isTerminalTarget(event.target)) return false;
  if (!event.ctrlKey || event.metaKey || event.altKey) return false;
  if (preset === "terminal-first" && isTerminalFirstControlChord(event)) return true;
  if (event.shiftKey) return false;
  return /^[a-zA-Z]$/.test(event.key);
}

/** Ctrl+digit, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageUp/PageDown; the caller already checked Ctrl. */
function isTerminalFirstControlChord(event: KeyboardEvent): boolean {
  if (event.code === "Tab" || event.key === "Tab") return true;
  if (event.shiftKey) return false;
  if (/^(Digit|Numpad)[0-9]$/.test(event.code) || /^[0-9]$/.test(event.key)) return true;
  return event.key === "PageUp" || event.key === "PageDown" || event.code === "PageUp" || event.code === "PageDown";
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
  // iPadOS masquerades as macOS: it reports platform "MacIntel" and a Macintosh user
  // agent, and only the touch-point count separates it from a real Mac (which is 0),
  // so an iPad with a hardware keyboard must not resolve `mod` to Cmd.
  if (/iPhone|iPad|iPod/.test(navigator.platform)) return true;
  if (classifyNavigatorPlatform(navigator) === "macos" && (navigator.maxTouchPoints ?? 0) === 0) return true;
  return getRuntimeProcessPlatform() === "macos";
}
