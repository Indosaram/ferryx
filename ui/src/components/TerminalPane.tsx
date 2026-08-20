import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

import "@xterm/xterm/css/xterm.css";

type TerminalOutputPayload = {
  session_id: string;
  data: string;
};

type TerminalPaneProps = {
  cwd: string;
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

export function TerminalPane({ cwd, active }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const activeRef = useRef(active);
  const [connectionError, setConnectionError] = useState<string | null>(null);

  useEffect(() => {
    activeRef.current = active;
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

    try {
      const webglAddon = new WebglAddon();
      webglAddon.onContextLoss(() => webglAddon.dispose());
      terminal.loadAddon(webglAddon);
    } catch {
      // Canvas rendering remains available when WebGL is unavailable.
    }

    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let resizeFrame = 0;

    const resizeTerminal = () => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        if (disposed || host.clientWidth === 0 || host.clientHeight === 0) return;
        fitAddon.fit();
        const sessionId = sessionIdRef.current;
        if (sessionId && isTauri()) {
          void invoke("cmd_terminal_resize", {
            sessionId,
            cols: terminal.cols,
            rows: terminal.rows,
          }).catch(() => undefined);
        }
      });
    };

    const resizeObserver = new ResizeObserver(resizeTerminal);
    resizeObserver.observe(host);

    const dataDisposable = terminal.onData((data) => {
      const sessionId = sessionIdRef.current;
      if (!sessionId || !isTauri()) return;
      void invoke("cmd_terminal_write", { sessionId, data }).catch((error: unknown) => {
        setConnectionError(String(error));
      });
    });

    const focusTerminal = () => terminal.focus();
    host.addEventListener("pointerdown", focusTerminal);

    async function connect() {
      resizeTerminal();
      if (!isTauri()) {
        terminal.writeln("\x1b[1;32momo bridge\x1b[0m  UI preview");
        terminal.writeln("\x1b[90mLaunch through Tauri to attach the portable-pty session.\x1b[0m");
        terminal.write("\r\n\x1b[34m~\x1b[0m \x1b[32m❯\x1b[0m ");
        return;
      }

      try {
        const sessionId = await invoke<string>("cmd_terminal_spawn", {
          request: {
            cwd,
            cols: terminal.cols,
            rows: terminal.rows,
            command: null,
          },
        });
        if (disposed) {
          await invoke("cmd_terminal_close", { sessionId }).catch(() => undefined);
          return;
        }

        sessionIdRef.current = sessionId;
        unlisten = await listen<TerminalOutputPayload>(`terminal_output:${sessionId}`, (event) => {
          terminal.write(event.payload.data);
        });
        resizeTerminal();
        if (activeRef.current) terminal.focus();
      } catch (error) {
        const message = String(error);
        setConnectionError(message);
        terminal.writeln(`\x1b[31mFailed to start terminal: ${message}\x1b[0m`);
      }
    }

    void connect();

    return () => {
      disposed = true;
      cancelAnimationFrame(resizeFrame);
      resizeObserver.disconnect();
      host.removeEventListener("pointerdown", focusTerminal);
      dataDisposable.dispose();
      unlisten?.();
      const sessionId = sessionIdRef.current;
      sessionIdRef.current = null;
      if (sessionId && isTauri()) {
        void invoke("cmd_terminal_close", { sessionId }).catch(() => undefined);
      }
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [cwd]);

  return (
    <div className="relative h-full w-full bg-terminal">
      <div ref={hostRef} className="terminal-host h-full w-full" aria-label={`Terminal in ${cwd}`} />
      {connectionError ? (
        <div className="pointer-events-none absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          PTY disconnected
        </div>
      ) : null}
    </div>
  );
}
