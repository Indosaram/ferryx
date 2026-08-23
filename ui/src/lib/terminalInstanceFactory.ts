import type { FitAddon } from "@xterm/addon-fit";
import type { SearchAddon } from "@xterm/addon-search";
import type { Terminal } from "@xterm/xterm";

import { isTauriRuntime, resizeTerminal, writeTerminal } from "./tauri";
import { attachScheduledOutputSubscription } from "./terminalOutputScheduler";
import { attachWebglRenderer, loadTerminalAssets } from "./terminalRenderer";
import {
  FALLBACK_PREFERENCES,
  fetchCachedNativePreferences,
  loadTerminalSettings,
  resolveTerminalSettings,
} from "./terminalSettings";
import type { TerminalSession } from "./types";

export type TerminalInstance = {
  element: HTMLDivElement;
  terminal: Terminal;
  fitAddon: FitAddon;
  searchAddon: SearchAddon;
  disposeWebgl: () => void;
  resizeObserver: ResizeObserver;
  disposables: Array<() => void>;
  session: TerminalSession;
  active: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
  unsubscribeOutput?: () => void;
};

export type CreateTerminalInstanceOptions = {
  session: TerminalSession;
  active: boolean;
  isVisible: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
  getInstance: (sessionId: string) => TerminalInstance | undefined;
};

type TerminalViewport = {
  readonly buffer: {
    readonly active: {
      readonly baseY: number;
      readonly viewportY: number;
    };
  };
  scrollToBottom(): void;
};

export function fitTerminal(terminal: TerminalViewport, fitAddon: Pick<FitAddon, "fit">): void {
  const wasAtBottom = terminal.buffer.active.viewportY === terminal.buffer.active.baseY;
  fitAddon.fit();
  if (wasAtBottom) {
    terminal.scrollToBottom();
  }
}

export function disposeTerminalInstance(inst: TerminalInstance): void {
  inst.resizeObserver.disconnect();
  inst.unsubscribeOutput?.();
  inst.unsubscribeOutput = undefined;
  for (const d of inst.disposables) d();
  inst.searchAddon.dispose();
  inst.disposeWebgl();
  inst.terminal.dispose();
  inst.element.remove();
}

export async function createTerminalInstance({
  session,
  active,
  isVisible,
  onBell,
  onTitleChange,
  getInstance,
}: CreateTerminalInstanceOptions): Promise<TerminalInstance> {
  const hostElement = document.createElement("div");
  hostElement.className = "terminal-host h-full w-full bg-terminal";
  hostElement.setAttribute("aria-label", `Terminal in ${session.cwd}`);
  Object.assign(hostElement.style, {
    position: "absolute",
    inset: "0px",
    width: "100%",
    height: "100%",
    minWidth: "0px",
    minHeight: "0px",
    overflow: "hidden",
  });

  const [latestNativePrefs, { Terminal: TerminalConstructor, FitAddon: FitAddonConstructor, Unicode11Addon, SearchAddon: SearchAddonConstructor }] =
    await Promise.all([
      Promise.race([
        fetchCachedNativePreferences(),
        new Promise<typeof FALLBACK_PREFERENCES>((resolve) =>
          setTimeout(() => resolve(FALLBACK_PREFERENCES), 200),
        ),
      ]),
      loadTerminalAssets(),
    ]);

  const finalSettings = resolveTerminalSettings(loadTerminalSettings(), latestNativePrefs);

  const terminal = new TerminalConstructor({
    allowProposedApi: false,
    customGlyphs: false,
    convertEol: true,
    cursorBlink: true,
    cursorStyle: finalSettings.cursorStyle ?? "block",
    fontFamily: finalSettings.fontFamily,
    fontSize: finalSettings.fontSize,
    lineHeight: 1.0,
    letterSpacing: 0,
    macOptionIsMeta: finalSettings.macosOptionAsAlt,
    scrollback: finalSettings.scrollback,
    theme: finalSettings.theme,
  });
  const fitAddon = new FitAddonConstructor();
  terminal.loadAddon(fitAddon);
  const searchAddon = new SearchAddonConstructor();
  terminal.loadAddon(searchAddon);
  if (Unicode11Addon) {
    try {
      const unicode11Addon = new Unicode11Addon();
      terminal.loadAddon(unicode11Addon);
      terminal.unicode.activeVersion = "11";
    } catch {
    }
  }
  terminal.open(hostElement);

  let disposeWebgl: () => void = () => undefined;
  void attachWebglRenderer(terminal).then((dispose) => {
    disposeWebgl = dispose;
  });

  let resizeFrame = 0;
  const resize = () => {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(() => {
      if (hostElement.clientWidth === 0 || hostElement.clientHeight === 0) return;
      fitTerminal(terminal, fitAddon);
      const backendSessionId = session.backendSessionId;
      if (!backendSessionId) return;
      void resizeTerminal({ sessionId: backendSessionId, cols: terminal.cols, rows: terminal.rows }).catch(() => undefined);
    });
  };

  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(hostElement);

  const disposables: Array<() => void> = [];

  const dataDisposable = terminal.onData((data) => {
    const backendSessionId = session.backendSessionId;
    if (!backendSessionId) return;
    void writeTerminal({ sessionId: backendSessionId, data }).catch(() => undefined);
  });
  disposables.push(() => dataDisposable.dispose());

  const titleDisposable = terminal.onTitleChange((title) => {
    getInstance(session.id)?.onTitleChange?.(title);
  });
  disposables.push(() => titleDisposable.dispose());

  const bellDisposable = terminal.onBell(() => {
    getInstance(session.id)?.onBell?.();
  });
  disposables.push(() => bellDisposable.dispose());

  const focusTerminal = () => terminal.focus();
  hostElement.addEventListener("pointerdown", focusTerminal);
  disposables.push(() => hostElement.removeEventListener("pointerdown", focusTerminal));

  let unsubscribeOutput: (() => void) | undefined;
  if (isVisible && session.backendSessionId) {
    unsubscribeOutput = attachScheduledOutputSubscription(session.backendSessionId, terminal, {
      initialSequence: session.lastOutputSequence,
      daemonEpoch: session.daemonEpoch,
      onSequenceUpdate: (seq, epoch) => {
        session.lastOutputSequence = seq;
        if (epoch) session.daemonEpoch = epoch;
      },
      onGap: () => {
        session.lastOutputSequence = null;
      },
    });
  }

  if (!isTauriRuntime()) {
    terminal.writeln("\x1b[1;32mFerryx\x1b[0m  UI preview");
    terminal.writeln("\x1b[90mLaunch through Tauri to attach the PTY session.\x1b[0m");
    terminal.write("\r\n\x1b[34m~\x1b[0m \x1b[32m❯\x1b[0m ");
  }

  resize();
  if (active) terminal.focus();

  return {
    element: hostElement,
    terminal,
    fitAddon,
    searchAddon,
    disposeWebgl,
    resizeObserver,
    disposables,
    session,
    active,
    onBell,
    onTitleChange,
    unsubscribeOutput,
  };
}
