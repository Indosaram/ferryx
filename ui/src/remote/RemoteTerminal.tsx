import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import React, { useEffect, useRef } from "react";

type RemoteTerminalProps = {
  sessionId: string;
  token: string;
  onBack: () => void;
};

export const RemoteTerminal: React.FC<RemoteTerminalProps> = ({ sessionId, token, onBack }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      theme: {
        background: "#000000",
        foreground: "#d4d4d4",
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    termRef.current = term;
    fitRef.current = fitAddon;

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/api/v1/terminal/${sessionId}?token=${encodeURIComponent(token)}`;

    const ws = new WebSocket(wsUrl);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onmessage = (event) => {
      if (typeof event.data === "string") {
        term.write(event.data);
      } else {
        term.write(new Uint8Array(event.data));
      }
    };

    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(data));
      }
    });

    const handleResize = () => {
      if (fitRef.current && ws.readyState === WebSocket.OPEN) {
        fitRef.current.fit();
        ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
      }
    };

    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      ws.close();
      term.dispose();
    };
  }, [sessionId, token]);

  const sendKey = (type: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    const enc = new TextEncoder();
    if (type === "ctrl-c") {
      ws.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
    } else if (type === "tab") {
      ws.send(enc.encode("\t"));
    } else if (type === "esc") {
      ws.send(enc.encode("\x1b"));
    } else if (type === "up") {
      ws.send(enc.encode("\x1b[A"));
    } else if (type === "down") {
      ws.send(enc.encode("\x1b[B"));
    } else if (type === "left") {
      ws.send(enc.encode("\x1b[D"));
    } else if (type === "right") {
      ws.send(enc.encode("\x1b[C"));
    } else if (type === "ctrl-d") {
      ws.send(enc.encode("\x04"));
    }
  };

  return (
    <div className="flex flex-col h-screen bg-black overflow-hidden">
      <div className="flex justify-between items-center px-4 py-2 bg-neutral-900 border-b border-neutral-800">
        <button
          onClick={onBack}
          className="text-xs px-2 py-1 bg-neutral-800 hover:bg-neutral-700 text-neutral-300 rounded"
        >
          ← Back
        </button>
        <span className="font-mono text-xs text-neutral-400">{sessionId.substring(0, 8)}</span>
        <span className="text-[10px] px-1.5 py-0.5 bg-emerald-950 text-emerald-400 border border-emerald-800 rounded">
          Connected
        </span>
      </div>

      <div ref={containerRef} className="flex-1 p-1 overflow-hidden" />

      {/* Mobile Special Key Bar */}
      <div className="flex gap-2 p-2 bg-neutral-900 border-t border-neutral-800 overflow-x-auto select-none">
        <button onClick={() => sendKey("ctrl-c")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          Ctrl-C
        </button>
        <button onClick={() => sendKey("tab")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          Tab
        </button>
        <button onClick={() => sendKey("esc")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          Esc
        </button>
        <button onClick={() => sendKey("up")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          ↑
        </button>
        <button onClick={() => sendKey("down")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          ↓
        </button>
        <button onClick={() => sendKey("left")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          ←
        </button>
        <button onClick={() => sendKey("right")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          →
        </button>
        <button onClick={() => sendKey("ctrl-d")} className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 active:bg-blue-600 text-neutral-200 text-xs font-mono font-bold rounded">
          Ctrl-D
        </button>
      </div>
    </div>
  );
};
