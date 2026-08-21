import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

import { isTauriRuntime, resizeTerminal, writeTerminal } from "../lib/tauri";
import { terminalEventBus } from "../lib/terminalEvents";
import { attachWebglRenderer, loadTerminalAssets } from "../lib/terminalRenderer";
import {
  applyTerminalSettings,
  fetchCachedNativePreferences,
  loadTerminalSettings,
  resolveTerminalSettings,
  useTerminalSettings,
} from "../lib/terminalSettings";
import type { TerminalSession } from "../lib/types";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
};

export function TerminalPane({ session, active, onBell, onTitleChange }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionRef = useRef(session);
  const onBellRef = useRef(onBell);
  const onTitleChangeRef = useRef(onTitleChange);
  const { settings, refreshNativePreferences } = useTerminalSettings();
  const settingsRef = useRef(settings);
  const refreshPreferencesRef = useRef(refreshNativePreferences);
  const [connectionError, setConnectionError] = useState<string | null>(null);

  sessionRef.current = session;
  onBellRef.current = onBell;
  onTitleChangeRef.current = onTitleChange;
  settingsRef.current = settings;
  refreshPreferencesRef.current = refreshNativePreferences;

  useEffect(() => {
    if (active) {
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        terminalRef.current?.focus();
      });
    }
  }, [active]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    applyTerminalSettings(terminal, settings);
    requestAnimationFrame(() => fitRef.current?.fit());
  }, [
    settings.fontFamily,
    settings.fontSize,
    settings.macosOptionAsAlt,
    settings.scrollback,
    settings.cursorStyle,
    settings.theme,
  ]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const abortController = new AbortController();
    let disposed = false;
    let cleanupTerminal: (() => void) | null = null;
    setConnectionError(null);

    void initialize(host).catch((error: unknown) => {
      if (disposed) return;
      setConnectionError(error instanceof Error ? error.message : "Terminal renderer failed");
    });

    async function initialize(hostElement: HTMLDivElement) {
      let latestNativePrefs = await fetchCachedNativePreferences();
      if (disposed) return;

      if (typeof document !== "undefined" && "fonts" in document) {
        try {
          await document.fonts.ready;
        } catch {
        }
      }
      if (disposed) return;

      // Always re-resolve using the latest localSettings and nativePreferences
      const finalSettings = resolveTerminalSettings(loadTerminalSettings(), latestNativePrefs);

      const { Terminal: TerminalConstructor, FitAddon: FitAddonConstructor } = await loadTerminalAssets();
      if (disposed) return;

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
      terminalRef.current = terminal;
      fitRef.current = fitAddon;

      let disposeWebgl: () => void = () => undefined;
      void attachWebglRenderer(terminal, abortController.signal).then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }
        disposeWebgl = dispose;
      });

      let resizeFrame = 0;
      const resize = () => {
        cancelAnimationFrame(resizeFrame);
        resizeFrame = requestAnimationFrame(() => {
          if (disposed || hostElement.clientWidth === 0 || hostElement.clientHeight === 0) return;
          fitAddon.fit();
          const backendSessionId = sessionRef.current.backendSessionId;
          if (!backendSessionId) return;
          void resizeTerminal({ sessionId: backendSessionId, cols: terminal.cols, rows: terminal.rows }).catch(() => undefined);
        });
      };

      const resizeObserver = new ResizeObserver(resize);
      resizeObserver.observe(hostElement);

      const dataDisposable = terminal.onData((data) => {
        const backendSessionId = sessionRef.current.backendSessionId;
        if (!backendSessionId) return;
        void writeTerminal({ sessionId: backendSessionId, data }).catch((error: unknown) => {
          setConnectionError(error instanceof Error ? error.message : "Terminal write failed");
        });
      });

      const bellDispose = terminal.onBell(() => onBellRef.current?.());
      const titleDispose = terminal.onTitleChange((title) => onTitleChangeRef.current?.(title));

      const focusTerminal = () => terminal.focus();
      hostElement.addEventListener("pointerdown", focusTerminal);

      const backendSessionId = sessionRef.current.backendSessionId;
      const unsubscribeOutput = backendSessionId
        ? terminalEventBus.subscribeOutput(backendSessionId, (text) => terminal.write(text))
        : () => undefined;

      if (!isTauriRuntime()) {
        terminal.writeln("\x1b[1;32mrorca\x1b[0m  UI preview");
        terminal.writeln("\x1b[90mLaunch through Tauri to attach the PTY session.\x1b[0m");
        terminal.write("\r\n\x1b[34m~\x1b[0m \x1b[32m❯\x1b[0m ");
      }

      resize();
      if (active) terminal.focus();

      cleanupTerminal = () => {
        cancelAnimationFrame(resizeFrame);
        resizeObserver.disconnect();
        hostElement.removeEventListener("pointerdown", focusTerminal);
        dataDisposable.dispose();
        bellDispose.dispose();
        titleDispose.dispose();
        unsubscribeOutput();
        disposeWebgl();
        terminal.dispose();
        terminalRef.current = null;
        fitRef.current = null;
      };
    }

    return () => {
      disposed = true;
      abortController.abort();
      cleanupTerminal?.();
    };
  }, [session.id]);

  return (
    <div className="relative h-full w-full bg-terminal">
      <div ref={hostRef} className="terminal-host h-full w-full" aria-label={`Terminal in ${session.cwd}`} />
      {connectionError ? (
        <div className="pointer-events-none absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          PTY disconnected
        </div>
      ) : null}
    </div>
  );
}
