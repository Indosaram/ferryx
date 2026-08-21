export type Unsubscribe = () => void;

export type TerminalAttachment = {
  sessionId: string;
  initialHistory?: Uint8Array;
};

export interface TerminalTransport {
  listSessions(): Promise<{ sessionId: string; worktreePath?: string | null }[]>;
  attach(sessionId: string): Promise<TerminalAttachment>;
  write(sessionId: string, data: string | Uint8Array): Promise<void> | void;
  resize(sessionId: string, cols: number, rows: number): Promise<void> | void;
  signal(sessionId: string, signal: "interrupt" | "terminate" | "kill"): Promise<void> | void;
  close(sessionId: string): Promise<void> | void;
  onOutput(sessionId: string, listener: (data: string | Uint8Array) => void): Unsubscribe;
  onLifecycle(listener: (event: { sessionId: string; state: string; exitCode?: number | null }) => void): Unsubscribe;
}
