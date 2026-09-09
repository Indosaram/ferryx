// TEMPORARY-DIAGNOSTIC: uses switchDebug's dev/explicit opt-in gate and sinks.
import { switchDebug } from "./switchDebug";

function describeTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return null;
  // Never include text, values, URLs, dynamic IDs or arbitrary attributes.
  return { tag: target.tagName, editable: target.isContentEditable,
    terminal: target.closest(".terminal-host") !== null };
}

export function shortcutContext() {
  return { platform: navigator.platform, documentFocused: document.hasFocus(),
    activeElement: describeTarget(document.activeElement) };
}

export function shortcutChord(event: KeyboardEvent) {
  // Shift-only typing and AltGr/IME text must not expose typed characters.
  if (!event.metaKey && !event.ctrlKey && !event.altKey) return null;
  return { ...shortcutContext(), target: describeTarget(event.target),
    code: event.code, keyCode: event.keyCode, meta: event.metaKey, control: event.ctrlKey,
    alt: event.altKey, shift: event.shiftKey, composing: event.isComposing,
    altGraph: event.getModifierState("AltGraph"), repeat: event.repeat,
    defaultPrevented: event.defaultPrevented, trusted: event.isTrusted, timeStamp: event.timeStamp };
}

export function installShortcutDiagnostics() {
  const receive = (event: KeyboardEvent) => {
    const chord = shortcutChord(event);
    if (chord) switchDebug("shortcut.webview.keydown", chord);
  };
  switchDebug("shortcut.webview.install", shortcutContext());
  window.addEventListener("keydown", receive, true);
  return () => {
    window.removeEventListener("keydown", receive, true);
    switchDebug("shortcut.webview.uninstall");
  };
}

export function traceShortcutAction<T>(action: string, handler: () => T, details: Record<string, unknown> = {}): T {
  switchDebug("shortcut.action.invoke", { ...details, action });
  try {
    const result = handler();
    // Do not attach a rejection handler: that would change unhandledrejection behavior.
    // This is synchronous return only; async failures retain the existing global error path.
    switchDebug("shortcut.action.return", { ...details, action, returnType: typeof result });
    return result;
  } catch (error) {
    switchDebug("shortcut.action.error", { ...details, action, errorType: error instanceof Error ? error.name : typeof error });
    throw error;
  }
}
