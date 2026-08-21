import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";

import { isTauriRuntime, resizeTerminal, writeTerminal } from "./tauri";
import { terminalEventBus } from "./terminalEvents";
import { attachWebglRenderer, loadTerminalAssets } from "./terminalRenderer";
import {
  applyTerminalSettings,
  FALLBACK_PREFERENCES,
  fetchCachedNativePreferences,
  loadTerminalSettings,
  resolveTerminalSettings,
  type EffectiveTerminalSettings,
} from "./terminalSettings";
import type { TerminalSession } from "./types";

export type TerminalInstance = {
  element: HTMLDivElement;
  terminal: Terminal;
  fitAddon: FitAddon;
  disposeWebgl: () => void;
  resizeObserver: ResizeObserver;
  disposables: Array<() => void>;
  session: TerminalSession;
  active: boolean;
};

class TerminalHostManager {
  private instances = new Map<string, TerminalInstance>();
  private pendingSpawns = new Map<string, Promise<TerminalInstance>>();

  getInstance(sessionId: string): TerminalInstance | undefined {
    return this.instances.get(sessionId);
  }

  async getOrCreate(
    session: TerminalSession,
    active: boolean,
    onBell?: () => void,
    onTitleChange?: (title: string) => void,
  ): Promise<TerminalInstance> {
    const existing = this.instances.get(session.id);
    if (existing) {
      existing.session = session;
      existing.active = active;
      return existing;
    }

    const pending = this.pendingSpawns.get(session.id);
    if (pending) return pending;

    const spawnPromise = this.createInstance(session, active, onBell, onTitleChange);
    this.pendingSpawns.set(session.id, spawnPromise);

    try {
      const instance = await spawnPromise;
      this.instances.set(session.id, instance);
      return instance;
    } finally {
      this.pendingSpawns.delete(session.id);
    }
  }

  private async createInstance(
    session: TerminalSession,
    active: boolean,
    onBell?: () => void,
    onTitleChange?: (title: string) => void,
  ): Promise<TerminalInstance> {
    const hostElement = document.createElement("div");
    hostElement.className = "terminal-host h-full w-full bg-terminal";
    hostElement.setAttribute("aria-label", `Terminal in ${session.cwd}`);

    const [latestNativePrefs, { Terminal: TerminalConstructor, FitAddon: FitAddonConstructor }] =
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
      customGlyphs: true,
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
        fitAddon.fit();
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

    const bellDispose = terminal.onBell(() => onBell?.());
    disposables.push(() => bellDispose.dispose());

    const titleDispose = terminal.onTitleChange((title) => onTitleChange?.(title));
    disposables.push(() => titleDispose.dispose());

    const focusTerminal = () => terminal.focus();
    hostElement.addEventListener("pointerdown", focusTerminal);
    disposables.push(() => hostElement.removeEventListener("pointerdown", focusTerminal));

    if (session.backendSessionId) {
      const unsubOutput = terminalEventBus.subscribeOutput(session.backendSessionId, (text) => {
        terminal.write(text);
      });
      disposables.push(unsubOutput);
    }

    if (!isTauriRuntime()) {
      terminal.writeln("\x1b[1;32mrorca\x1b[0m  UI preview");
      terminal.writeln("\x1b[90mLaunch through Tauri to attach the PTY session.\x1b[0m");
      terminal.write("\r\n\x1b[34m~\x1b[0m \x1b[32m❯\x1b[0m ");
    }

    resize();
    if (active) terminal.focus();

    return {
      element: hostElement,
      terminal,
      fitAddon,
      disposeWebgl,
      resizeObserver,
      disposables,
      session,
      active,
    };
  }

  updateSession(sessionId: string, session: TerminalSession) {
    const inst = this.instances.get(sessionId);
    if (inst) {
      const prevBackendId = inst.session.backendSessionId;
      inst.session = session;
      if (session.backendSessionId && session.backendSessionId !== prevBackendId) {
        const unsub = terminalEventBus.subscribeOutput(session.backendSessionId, (text) => {
          inst.terminal.write(text);
        });
        inst.disposables.push(unsub);
      }
    }
  }

  applySettings(settings: EffectiveTerminalSettings) {
    for (const inst of this.instances.values()) {
      applyTerminalSettings(inst.terminal, settings);
      inst.fitAddon.fit();
    }
  }

  destroy(sessionId: string) {
    const inst = this.instances.get(sessionId);
    if (inst) {
      this.instances.delete(sessionId);
      inst.resizeObserver.disconnect();
      for (const d of inst.disposables) d();
      inst.disposeWebgl();
      inst.terminal.dispose();
      inst.element.remove();
    }
  }
}

export const terminalHostManager = new TerminalHostManager();
