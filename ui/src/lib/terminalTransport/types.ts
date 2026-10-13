export type Unsubscribe = () => void;

export type ReplayGap = {
  requestedAfterSequence: string;
  availableFromSequence: string;
};

/** Raised when output was lost (daemon replay gap or an undecodable frame) and the consumer must resync. */
export type TerminalResync = {
  reason: "gap" | "decodeError";
  requestedAfterSequence: string;
  availableFromSequence: string;
};

export type TerminalAttachment = {
  sessionId: string;
  daemonEpoch?: string | null;
  historyStartSequence?: string | null;
  historyEndSequence?: string | null;
  initialHistory?: Uint8Array;
  gap?: ReplayGap | null;
};

export interface TerminalTransport {
  listSessions(): Promise<{ sessionId: string; worktreePath?: string | null; daemonEpoch?: string | null; incarnation?: string | null; running?: boolean }[]>;
  attach(sessionId: string, afterSequence?: string | null): Promise<TerminalAttachment>;
  write(sessionId: string, data: string | Uint8Array): Promise<void> | void;
  resize(sessionId: string, cols: number, rows: number): Promise<void> | void;
  signal(sessionId: string, signal: "interrupt" | "terminate" | "kill"): Promise<void> | void;
  close(sessionId: string): Promise<void> | void;
  onOutput(sessionId: string, listener: (data: string | Uint8Array) => void): Unsubscribe;
  /** Optional: notified that output was lost (replay gap or undecodable frame) so the consumer can resync. */
  onResync?(sessionId: string, listener: (resync: TerminalResync) => void): Unsubscribe;
  onLifecycle(listener: (event: { sessionId: string; state: string; exitCode?: number | null }) => void): Unsubscribe;
}
