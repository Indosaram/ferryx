import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { formatBindingLabel, isMacShortcutPlatform, matchesBinding, SHORTCUTS, type ShortcutActionId } from "../lib/shortcuts";

export type ShortcutHintContext = {
  readonly tabIds: readonly string[];
  readonly closeTabId: string | null;
  readonly worktrees: readonly { readonly path: string; readonly workspaceId?: string }[];
};

export type ShortcutHintsProps = {
  readonly getContext: () => ShortcutHintContext;
  readonly enabledActions: readonly ShortcutActionId[];
  readonly isMac?: boolean;
};

type Hint = {
  readonly action: ShortcutActionId;
  readonly label: string;
  readonly target: string;
  readonly left: number;
  readonly top: number;
  readonly width: number;
};

export function ShortcutHints(props: ShortcutHintsProps) {
  const latest = useRef(props);
  latest.current = props;
  const [hints, setHints] = useState<readonly Hint[]>([]);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let shown = false;
    let composing = false;
    let modifiers = "";
    const clear = () => {
      clearTimeout(timer);
      timer = undefined;
      shown = false;
      modifiers = "";
      setHints((current) => current.length ? [] : current);
    };
    const show = (event: KeyboardEvent) => {
      const { getContext, enabledActions, isMac = isMacShortcutPlatform() } = latest.current;
      const context = getContext();
      const enabled = new Set(enabledActions);
      const focus = document.activeElement;
      const editing = focus instanceof HTMLElement
        && (focus.isContentEditable || focus.matches("input, textarea, select"))
        && !focus.closest(".terminal-host");
      const dialogs = document.querySelectorAll<HTMLElement>('[role="dialog"]');
      const dialog = dialogs.item(dialogs.length - 1);
      const targets = document.querySelectorAll<HTMLElement>(
        "[data-shortcut], [data-tab-dnd-id], [data-shortcut-worktree-path], [data-shortcut-close-tab]",
      );
      const next: Hint[] = [];
      for (const target of targets) {
        if (target.closest('[data-shortcut-scope-active="false"], [hidden], [inert], [aria-hidden="true"]')) continue;
        if (target.matches(':disabled, [aria-disabled="true"]') || (dialog && !dialog.contains(target))) continue;
        const rect = target.getBoundingClientRect();
        if (!rect.width || !rect.height || rect.bottom <= 0 || rect.top >= innerHeight || rect.right <= 0 || rect.left >= innerWidth) continue;
        let clipped = false;
        for (let parent: HTMLElement | null = target; parent; parent = parent.parentElement) {
          const style = getComputedStyle(parent);
          if (style.display === "none" || style.visibility === "hidden") { clipped = true; break; }
          if (parent !== target && /hidden|clip|auto|scroll/.test(`${style.overflowX} ${style.overflowY}`)) {
            const bounds = parent.getBoundingClientRect();
            if (rect.right <= bounds.left || rect.left >= bounds.right || rect.bottom <= bounds.top || rect.top >= bounds.bottom) {
              clipped = true;
              break;
            }
          }
        }
        if (clipped) continue;
        const actions = (target.dataset.shortcut ?? "").split(" ");
        if (target.dataset.tabDndId) {
          const index = context.tabIds.indexOf(target.dataset.tabDndId);
          if (index >= 0 && index < 9) actions.push(`tab.select${index + 1}`);
        }
        if (target.dataset.shortcutCloseTab === context.closeTabId) actions.push("tab.close");
        if (target.dataset.shortcutWorktreePath) {
          const index = context.worktrees.findIndex((row) =>
            row.path === target.dataset.shortcutWorktreePath
            && (row.workspaceId ?? "") === (target.dataset.shortcutWorkspaceId ?? ""),
          );
          if (index >= 0 && index < 9) actions.push(`workspace.select${index + 1}`);
        }
        for (const shortcut of SHORTCUTS) {
          if (!enabled.has(shortcut.id) || !actions.includes(shortcut.id)) continue;
          if (editing && shortcut.id !== "settings.toggle" && shortcut.id !== "commandPalette.open" && !shortcut.id.startsWith("browser.")) continue;
          const binding = [shortcut.binding, ...(shortcut.aliases ?? [])].find((candidate) =>
            (!event.metaKey || Boolean(candidate.mod && isMac))
            && (!event.ctrlKey || Boolean(candidate.control || (candidate.mod && !isMac)))
            && (!event.altKey || Boolean(candidate.alt))
            && (!event.shiftKey || Boolean(candidate.shift)),
          );
          if (!binding) continue;
          const chord = new KeyboardEvent("keydown", {
            key: binding.key, metaKey: Boolean(binding.mod && isMac),
            ctrlKey: Boolean(binding.control || (binding.mod && !isMac)),
            altKey: Boolean(binding.alt), shiftKey: Boolean(binding.shift),
          });
          const winner = SHORTCUTS.find((candidate) => enabled.has(candidate.id)
            && [candidate.binding, ...(candidate.aliases ?? [])].some((keys) => matchesBinding(chord, keys, isMac)));
          if (winner?.id !== shortcut.id) continue;
          const label = formatBindingLabel(binding, isMac);
          const width = label.length * 7 + 16;
          const left = Math.max(4, Math.min(rect.right - width, innerWidth - width - 4));
          let top = Math.max(4, Math.min(rect.top, innerHeight - 24));
          // Adjacent compact controls can be narrower than their labels.
          while (next.some((hint) => left < hint.left + hint.width + 4 && left + width + 4 > hint.left && Math.abs(top - hint.top) < 24)) {
            top += 24;
          }
          if (top > innerHeight - 24) continue;
          next.push({ action: shortcut.id, label, target: target.getAttribute("aria-label") ?? target.textContent ?? "", left, top, width });
        }
      }
      shown = true;
      setHints(next);
    };
    const update = (event: KeyboardEvent) => {
      if (composing || event.isComposing || event.keyCode === 229 || event.getModifierState("AltGraph")
        || !["Meta", "Control", "Alt", "Shift"].includes(event.key)) {
        clear();
        return;
      }
      const isMac = latest.current.isMac ?? isMacShortcutPlatform();
      if ((!event.ctrlKey && !event.altKey && !(isMac && event.metaKey)) || (!isMac && event.metaKey)) {
        clear();
        return;
      }
      const next = `${event.metaKey}:${event.ctrlKey}:${event.altKey}:${event.shiftKey}`;
      if (next === modifiers) return;
      modifiers = next;
      clearTimeout(timer);
      if (shown) show(event);
      else timer = setTimeout(() => { timer = undefined; show(event); }, 300);
    };
    const startComposition = () => { composing = true; clear(); };
    const endComposition = () => { composing = false; };
    window.addEventListener("keydown", update, true);
    window.addEventListener("keyup", update, true);
    window.addEventListener("blur", clear);
    window.addEventListener("pointerdown", clear, true);
    window.addEventListener("resize", clear);
    window.addEventListener("scroll", clear, true);
    window.addEventListener("compositionstart", startComposition, true);
    window.addEventListener("compositionend", endComposition, true);
    document.addEventListener("visibilitychange", clear);
    // Layout changes dismiss rather than leave labels at stale target coordinates.
    const observer = new MutationObserver((records) => {
      if (records.some((record) => record.target instanceof Element && !record.target.closest("[data-shortcut-hints]"))) clear();
    });
    observer.observe(document.body, {
      childList: true, subtree: true, attributes: true,
      attributeFilter: ["disabled", "hidden", "aria-hidden", "aria-disabled", "inert", "data-shortcut-scope-active", "data-tab-index"],
    });
    return () => {
      clearTimeout(timer);
      observer.disconnect();
      window.removeEventListener("keydown", update, true);
      window.removeEventListener("keyup", update, true);
      window.removeEventListener("blur", clear);
      window.removeEventListener("pointerdown", clear, true);
      window.removeEventListener("resize", clear);
      window.removeEventListener("scroll", clear, true);
      window.removeEventListener("compositionstart", startComposition, true);
      window.removeEventListener("compositionend", endComposition, true);
      document.removeEventListener("visibilitychange", clear);
    };
  }, []);

  return createPortal(
    <div data-shortcut-hints aria-hidden="true" className="pointer-events-none fixed inset-0 z-50">
      {hints.map((hint, index) => (
        <kbd
          key={`${hint.action}:${index}`}
          data-shortcut-hint={hint.action}
          data-shortcut-target={hint.target}
          className="absolute truncate rounded border border-ring bg-popover px-1.5 py-0.5 text-center font-mono text-[11px] font-semibold leading-4 text-popover-foreground shadow-sm"
          style={{ left: hint.left, top: hint.top, width: hint.width }}
        >{hint.label}</kbd>
      ))}
    </div>,
    document.body,
  );
}
