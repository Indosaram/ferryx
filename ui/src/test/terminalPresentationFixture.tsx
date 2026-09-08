import { useState } from "react";
import { createRoot } from "react-dom/client";
import { TerminalPane } from "../components/TerminalPane";
import { DagPaneBadge } from "../components/dag/DagPaneBadge";
import { dagStore } from "../state/dagStore";
import type { TerminalSession } from "../lib/types";
import "../index.css";
import "../settings-runtime.css";

const frames = new Map<string, HTMLElement>();
const calls: string[] = [];
const receipt = { presented: true, cursorCol: 0, cursorRow: 0, cellWidthPx: 8, cellHeightPx: 16 };
let callbackId = 0;
Object.defineProperty(window, "isTauri", { value: true });

Object.defineProperty(window, "__TAURI_INTERNALS__", {
  value: {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (command: string, args?: {
      sessionId?: string;
      bounds?: { x: number; y: number; width: number; height: number };
    }) => {
      calls.push(command);
      if (command === "cmd_native_terminal_set_bounds" && args?.sessionId && args.bounds) {
        let frame = frames.get(args.sessionId);
        if (!frame) {
          frame = document.createElement("pre");
          frame.dataset.simulatedNativeFrame = args.sessionId;
          frame.textContent = `${args.sessionId}\n\n$ run task\nReading workspace...\nChecking changes...\n\nLast complete terminal frame\n\n터미널 내용 유지 확인`;
          frames.set(args.sessionId, frame);
          document.body.appendChild(frame);
        }
        Object.assign(frame.style, {
          position: "fixed", margin: "0", padding: "16px", boxSizing: "border-box",
          zIndex: "0", overflow: "hidden", color: "#ededed", background: "var(--terminal)",
          left: `${args.bounds.x}px`, top: `${args.bounds.y}px`,
          width: `${args.bounds.width}px`, height: `${args.bounds.height}px`,
        });
      }
      if (command === "cmd_native_terminal_detach" && args?.sessionId) {
        frames.get(args.sessionId)?.remove();
        frames.delete(args.sessionId);
      }
      if (command.startsWith("plugin:event|")) return callbackId;
      return receipt;
    },
  },
});

Object.defineProperty(window, "terminalPresentationQA", {
  value: { calls, frameIds: () => [...frames.keys()] },
});
document.documentElement.classList.add("platform-macos");
document.documentElement.dataset.theme = "dark";

dagStore.applySnapshot("/fixture", {
  runId: "fixture-run", runKey: "fixture-run", rootSessionId: "fixture-agent",
  name: "Terminal presentation check", status: "running",
  startedAt: null, completedAt: null, updatedAt: null, amendCount: 0,
  nodes: [{
    id: "verify", label: "Verify terminal visibility", state: "running", dependsOn: [],
    attempt: 1, route: { kind: "unknown" }, startedAt: null, completedAt: null,
    error: null, taskId: null,
  }],
  edges: [], waves: [{ index: 0, nodeIds: ["verify"] }], criticalPath: ["verify"],
  bottlenecks: [], counts: { total: 1, completed: 0, failed: 0, cancelled: 0, skipped: 0, running: 1 },
});

function makeSession(id: string, backendSessionId: string | null): TerminalSession {
  return {
    id, backendSessionId, cwd: "/fixture", workspaceId: "fixture",
    worktree: { wsId: "fixture", slug: "main" }, agentType: "omo",
    lifecycle: backendSessionId ? "working" : "exited",
  };
}

function Fixture() {
  const [disconnected, setDisconnected] = useState(false);
  const [reconnected, setReconnected] = useState(false);
  const [stacked, setStacked] = useState(false);
  return (
    <div style={{ position: "relative", zIndex: 1, height: "100vh" }}>
      <header style={{ height: 60, padding: 12, display: "flex", gap: 16, background: "var(--background)" }}>
        <span>Browser fixture: simulated native frames</span>
        <button id="disconnect" onClick={() => setDisconnected(true)}>Disconnect</button>
        <button id="reconnect" onClick={() => { setReconnected(true); setDisconnected(false); }}>Reconnect</button>
        <button id="move" onClick={() => setStacked((value) => !value)}>Move panes</button>
        <div style={{ position: "relative", width: 50 }}>
          <DagPaneBadge projectPath="/fixture" providerSessionId="fixture-agent" />
        </div>
      </header>
      <main data-testid="terminal-layout" style={{ display: "grid", height: "calc(100% - 60px)", gridTemplateColumns: stacked ? "1fr" : "1fr 1fr 1fr", gap: 1 }}>
        {[
          makeSession("pane-a", disconnected ? null : reconnected ? "backend-reconnected" : "backend-a"),
          makeSession("pane-b", "backend-b"),
          makeSession("pane-cold", null),
        ].map((session) => (
          <div key={session.id} data-testid="pane-leaf" style={{ position: "relative", minHeight: 0 }}>
            <TerminalPane session={session} active={false} onReconnect={() => { setReconnected(true); setDisconnected(false); }} />
          </div>
        ))}
      </main>
    </div>
  );
}

const backing = document.createElement("div");
Object.assign(backing.style, { position: "fixed", inset: "0", zIndex: "-1", background: "var(--terminal)" });
document.body.appendChild(backing);
const root = document.createElement("div");
root.id = "root";
document.body.appendChild(root);
createRoot(root).render(<Fixture />);
