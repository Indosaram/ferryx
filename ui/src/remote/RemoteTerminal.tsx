import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import React, { useEffect, useRef, useState } from "react";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { attachWebglRenderer, loadTerminalAssets } from "../lib/terminalRenderer";
import { applyTerminalSettings, useTerminalSettings } from "../lib/terminalSettings";

type RemoteTerminalProps = {
  sessionId: string;
  token: string;
  title?: string;
  onBack: () => void;
};

export const RemoteTerminal: React.FC<RemoteTerminalProps> = ({
  sessionId,
  token,
  title,
  onBack,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [connected, setConnected] = useState(false);
  const { settings, refreshNativePreferences } = useTerminalSettings();
  const settingsRef = useRef(settings);

  settingsRef.current = settings;

  useEffect(() => {
    void refreshNativePreferences();
  }, [refreshNativePreferences]);

  useEffect(() => {
    if (!containerRef.current) return;

    let terminalInstance: Terminal | null = null;
    let cleanupWebgl: (() => void) | null = null;
    let onDataDisposable: { dispose: () => void } | null = null;
    let isDisposed = false;
    const abortController = new AbortController();

    loadTerminalAssets()
      .then(async ({ Terminal: TerminalCtor, FitAddon: FitAddonCtor }) => {
        if (isDisposed || !containerRef.current) return;

        const term = new TerminalCtor({
          cursorBlink: true,
          fontFamily: settingsRef.current.fontFamily,
          fontSize: settingsRef.current.fontSize,
          macOptionIsMeta: settingsRef.current.macosOptionAsAlt,
          scrollback: settingsRef.current.scrollback,
          cursorStyle: settingsRef.current.cursorStyle,
          allowProposedApi: false,
          customGlyphs: true,
          convertEol: true,
          lineHeight: 1.0,
          letterSpacing: 0,
          theme: {
            background: settingsRef.current.theme.background,
            foreground: settingsRef.current.theme.foreground,
            cursor: settingsRef.current.theme.cursor,
            cursorAccent: settingsRef.current.theme.cursorAccent,
            selectionBackground: settingsRef.current.theme.selectionBackground,
            selectionForeground: settingsRef.current.theme.selectionForeground,
            black: settingsRef.current.theme.black,
            red: settingsRef.current.theme.red,
            green: settingsRef.current.theme.green,
            yellow: settingsRef.current.theme.yellow,
            blue: settingsRef.current.theme.blue,
            magenta: settingsRef.current.theme.magenta,
            cyan: settingsRef.current.theme.cyan,
            white: settingsRef.current.theme.white,
            brightBlack: settingsRef.current.theme.brightBlack,
            brightRed: settingsRef.current.theme.brightRed,
            brightGreen: settingsRef.current.theme.brightGreen,
            brightYellow: settingsRef.current.theme.brightYellow,
            brightBlue: settingsRef.current.theme.brightBlue,
            brightMagenta: settingsRef.current.theme.brightMagenta,
            brightCyan: settingsRef.current.theme.brightCyan,
            brightWhite: settingsRef.current.theme.brightWhite,
            extendedAnsi: settingsRef.current.theme.extendedAnsi,
          },
          allowTransparency: true,
        });

        const fitAddon = new FitAddonCtor();
        term.loadAddon(fitAddon);
        term.open(containerRef.current);
        cleanupWebgl = await attachWebglRenderer(term, abortController.signal);

        if (isDisposed) {
          term.dispose();
          cleanupWebgl?.();
          return;
        }

        // Defer initial fit to requestAnimationFrame with dimension guard
        requestAnimationFrame(() => {
          if (containerRef.current && containerRef.current.clientWidth > 0) {
            fitAddon.fit();
          }
        });

        termRef.current = term;
        fitRef.current = fitAddon;
        terminalInstance = term;

        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        const wsUrl = `${protocol}//${window.location.host}/api/v1/terminal/${sessionId}?token=${encodeURIComponent(token)}`;

        const ws = new WebSocket(wsUrl);
        ws.binaryType = "arraybuffer";
        wsRef.current = ws;

        ws.onopen = () => {
          setConnected(true);
          if (containerRef.current && containerRef.current.clientWidth > 0) {
            fitAddon.fit();
            ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
          }
        };

        ws.onclose = () => {
          setConnected(false);
        };

        ws.onmessage = (event) => {
          if (typeof event.data === "string") {
            term.write(event.data);
          } else {
            term.write(new Uint8Array(event.data));
          }
        };

        onDataDisposable = term.onData((data) => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(new TextEncoder().encode(data));
          }
        });
      })
      .catch(() => {});

    // Coalesced viewport adaptation using requestAnimationFrame
    let rAfId: number | null = null;
    const scheduleResize = () => {
      if (rAfId !== null) cancelAnimationFrame(rAfId);
      rAfId = requestAnimationFrame(() => {
        rAfId = null;
        if (window.visualViewport && rootRef.current) {
          rootRef.current.style.height = `${window.visualViewport.height}px`;
        }
        if (fitRef.current && containerRef.current && containerRef.current.clientWidth > 0 && termRef.current) {
          fitRef.current.fit();
          if (wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(
              JSON.stringify({
                type: "resize",
                cols: termRef.current.cols,
                rows: termRef.current.rows,
              })
            );
          }
        }
      });
    };

    window.addEventListener("resize", scheduleResize);
    if (window.visualViewport) {
      window.visualViewport.addEventListener("resize", scheduleResize);
    }

    return () => {
      isDisposed = true;
      abortController.abort();
      if (rAfId !== null) cancelAnimationFrame(rAfId);
      window.removeEventListener("resize", scheduleResize);
      if (window.visualViewport) {
        window.visualViewport.removeEventListener("resize", scheduleResize);
      }
      if (onDataDisposable) {
        onDataDisposable.dispose();
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
      if (cleanupWebgl) {
        cleanupWebgl();
      }
      if (terminalInstance) {
        terminalInstance.dispose();
      }
    };
  }, [sessionId, token]);

  // Synchronize settings changes dynamically
  useEffect(() => {
    if (termRef.current) {
      applyTerminalSettings(termRef.current, settings);
      fitRef.current?.fit();
    }
  }, [settings]);

  const sendKey = (key: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    const enc = new TextEncoder();
    if (key === "ctrl-c") {
      ws.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
    } else if (key === "tab") {
      ws.send(enc.encode("\t"));
    } else if (key === "esc") {
      ws.send(enc.encode("\x1b"));
    } else if (key === "up") {
      ws.send(enc.encode("\x1b[A"));
    } else if (key === "down") {
      ws.send(enc.encode("\x1b[B"));
    } else if (key === "left") {
      ws.send(enc.encode("\x1b[D"));
    } else if (key === "right") {
      ws.send(enc.encode("\x1b[C"));
    } else if (key === "ctrl-d") {
      ws.send(enc.encode("\x04"));
    } else if (key === "ctrl-z") {
      ws.send(enc.encode("\x1a"));
    } else if (key === "pageup") {
      ws.send(enc.encode("\x1b[5~"));
    } else if (key === "pagedown") {
      ws.send(enc.encode("\x1b[6~"));
    } else if (key === "home") {
      ws.send(enc.encode("\x1b[H"));
    } else if (key === "end") {
      ws.send(enc.encode("\x1b[F"));
    } else if (key.startsWith("alt-")) {
      const char = key.replace("alt-", "");
      ws.send(enc.encode("\x1b" + char));
    } else if (key.startsWith("ctrl-")) {
      const char = key.replace("ctrl-", "");
      if (char.length === 1) {
        const code = char.toUpperCase().charCodeAt(0) - 64;
        if (code >= 1 && code <= 26) {
          ws.send(new Uint8Array([code]));
        }
      }
    } else {
      ws.send(enc.encode(key));
    }
  };

  return (
    <div ref={rootRef} className="flex flex-col h-[100dvh] bg-terminal text-foreground overflow-hidden">
      {/* High-polish Orca Topbar */}
      <div className="flex justify-between items-center px-3 py-1.5 bg-card border-b border-border shrink-0">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onBack}
            className="text-xs px-2 py-1 bg-secondary hover:bg-secondary/80 text-secondary-foreground border border-border rounded font-medium transition-colors"
          >
            ← Workspace
          </button>
          <span className="font-medium text-xs text-foreground truncate max-w-[180px]">
            {title || `Terminal (${sessionId.substring(0, 8)})`}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <span
            className={`w-2 h-2 rounded-full ${
              connected ? "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)]" : "bg-amber-500 animate-pulse"
            }`}
          />
          <span className="text-[10px] font-mono text-muted-foreground">
            {connected ? "Live" : "Connecting"}
          </span>
        </div>
      </div>

      {/* xterm.js Terminal Container */}
      <div ref={containerRef} className="flex-1 p-1 bg-terminal overflow-hidden" />

      {/* Touch-First Mobile KeyDock */}
      <MobileKeyDock onSendKey={sendKey} />
    </div>
  );
};
