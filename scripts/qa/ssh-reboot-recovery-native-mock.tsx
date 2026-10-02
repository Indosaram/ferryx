import React from "react";
import type { TerminalSession } from "../../ui/src/lib/types";

export function NativeTerminalPane({
  sessionId,
  session,
}: {
  sessionId: string;
  session?: TerminalSession;
}) {
  return (
    <div
      data-testid="native-terminal"
      data-session-id={sessionId}
      data-backend-id={session?.backendSessionId}
      className="flex h-full w-full items-center justify-center bg-zinc-950 font-mono text-xs text-zinc-400 select-none p-6"
    >
      <div className="flex flex-col items-center gap-2 rounded border border-zinc-800 bg-zinc-900/80 p-5 text-center shadow">
        <div className="size-3 rounded-full bg-emerald-500 shadow-sm shadow-emerald-500/50" />
        <span className="font-semibold text-zinc-200">Native Terminal Surface Active</span>
        <span data-testid="native-pane-session-id" className="text-zinc-400">Pane: {sessionId}</span>
        <span data-testid="native-pane-backend-id" className="text-zinc-400">Backend: {session?.backendSessionId ?? "none"}</span>
      </div>
    </div>
  );
}
