import {
  attachTerminal,
  closeTerminal,
  listTerminalSessions,
  onTerminalLifecycle,
  onTerminalOutput,
  resizeTerminal,
  signalTerminal,
  writeTerminal,
} from "../tauri";
import { decodeBase64 } from "../terminalOutput";
import type { TerminalAttachment, TerminalTransport, Unsubscribe } from "./types";

export class TauriTerminalTransport implements TerminalTransport {
  async listSessions() {
    const sessions = await listTerminalSessions();
    return sessions.map((s) => ({
      sessionId: s.sessionId,
      worktreePath: s.worktreePath,
      daemonEpoch: s.daemonEpoch ?? null,
    }));
  }

  async attach(sessionId: string, afterSequence?: string | null): Promise<TerminalAttachment> {
    const res = await attachTerminal({ sessionId, afterSequence });
    const initialHistory = res.history ? decodeBase64(res.history) : undefined;
    return {
      sessionId: res.sessionId,
      daemonEpoch: res.daemonEpoch,
      historyStartSequence: res.historyStartSequence,
      historyEndSequence: res.historyEndSequence,
      initialHistory,
      gap: res.gap,
    };
  }

  async write(sessionId: string, data: string | Uint8Array) {
    const text = typeof data === "string" ? data : new TextDecoder().decode(data);
    await writeTerminal({ sessionId, data: text });
  }

  async resize(sessionId: string, cols: number, rows: number) {
    await resizeTerminal({ sessionId, cols, rows });
  }

  async signal(sessionId: string, signal: "interrupt" | "terminate" | "kill") {
    await signalTerminal({ sessionId, signal });
  }

  async close(sessionId: string) {
    await closeTerminal(sessionId);
  }

  onOutput(sessionId: string, listener: (data: string | Uint8Array) => void): Unsubscribe {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    onTerminalOutput((payload) => {
      if (payload.sessionId === sessionId) {
        listener(decodeBase64(payload.data));
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }

  onLifecycle(listener: (event: { sessionId: string; state: string; exitCode?: number | null }) => void): Unsubscribe {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    onTerminalLifecycle((payload) => {
      listener({
        sessionId: payload.sessionId,
        state: payload.state,
        exitCode: payload.exitCode,
      });
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }
}

export const defaultTauriTransport = new TauriTerminalTransport();
