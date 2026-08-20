import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

import "@xterm/xterm/css/xterm.css";

import { isTauriRuntime, resizeTerminal, writeTerminal } from "../lib/tauri";
import { terminalEventBus } from "../lib/terminalEvents";
import type { TerminalSession } from "../lib/types";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
};

const TERMINAL_THEME = {
  background: "#0a0a0a",
  foreground: "#d4d4d4",
  cursor: "#e5e5e5",
  cursorAccent: "#0a0a0a",
  selectionBackground: "#52525299",
  black: "#171717",
  red: "#f87171",
  green: "#86efac",
  yellow: "#fde68a",
  blue: "#93c5fd",
  magenta: "#d8b4fe",
  cyan: "#67e8f9",
  white: "#e5e5e5",
  brightBlack: "#737373",
  brightRed: "#fca5a5",
  brightGreen: "#bbf7d0",
  brightYellow: "#fef08a",
  brightBlue: "#bfdbfe",
  brightMagenta: "#e9d5ff",
  brightCyan: "#a5f3fc",
  brightWhite: "#fafafa",
} as const;

export function TerminalPane({ session, active }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionRef = useRef(session);
  const [connectionError, setConnectionError] = useState<string | null>(null);

  sessionRef.current = session;

  useEffect(() => {
    if (active) {
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        terminalRef.current?.focus();
      });
    }
  }, [active]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const terminal = new Terminal({
      allowProposedApi: false,
      convertEol: true,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: '"SF Mono", SFMono-Regular, ui-monospace, Menlo, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      letterSpacing: 0,
      scrollback: 10_000,
      theme: TERMINAL_THEME,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(host);
    terminalRef.current = terminal;
    fitRef.current = fitAddon;

    let webglAddon: WebglAddon | null = null;
    try {
      webglAddon = new WebglAddon();
      webglAddon.onContextLoss(() => {
        webglAddon?.dispose();
        webglAddon = null;
      });
      terminal.loadAddon(webglAddon);
    } catch {
      webglAddon = null;
    }

    let disposed = false;
    let resizeFrame = 0;

    const resize = () => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        if (disposed || host.clientWidth === 0 || host.clientHeight === 0) return;
        fitAddon.fit();
        const backendSessionId = sessionRef.current.backendSessionId;
        if (!backendSessionId) return;
        void resizeTerminal({ sessionId: backendSessionId, cols: terminal.cols, rows: terminal.rows }).catch(() => undefined);
      });
    };

    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(host);

    const dataDisposable = terminal.onData((data) => {
      const backendSessionId = sessionRef.current.backendSessionId;
      if (!backendSessionId) return;
      void writeTerminal({ sessionId: backendSessionId, data }).catch((error: unknown) => {
        setConnectionError(error instanceof Error ? error.message : "Terminal write failed");
      });
    });

    const focusTerminal = () => terminal.focus();
    host.addEventListener("pointerdown", focusTerminal);

    const backendSessionId = sessionRef.current.backendSessionId;
    const unsubscribeOutput = backendSessionId
      ? terminalEventBus.subscribeOutput(backendSessionId, (text) => terminal.write(text))
      : () => undefined;

    if (!isTauriRuntime()) {
      terminal.writeln("\x1b[1;32mORCA Lite\x1b[0m  UI preview");
      terminal.writeln("\x1b[90mLaunch through Tauri to attach the PTY session.\x1b[0m");
      terminal.write("\r\n\x1b[34m~\x1b[0m \x1b[32m❯\x1b[0m ");
    }

    resize();
    if (active) terminal.focus();

    return () => {
      disposed = true;
      cancelAnimationFrame(resizeFrame);
      resizeObserver.disconnect();
      host.removeEventListener("pointerdown", focusTerminal);
      dataDisposable.dispose();
      unsubscribeOutput();
      webglAddon?.dispose();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
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
