import { useState } from "react";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane } from "./NativeTerminalPane";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";
import { DagPaneBadge } from "./dag/DagPaneBadge";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
};

export function TerminalPane({
  session,
  active: _active,
  searchOpen,
  onCloseSearch,
}: TerminalPaneProps) {
  const [error] = useState<string | null>(null);

  return (
    <div
      data-testid="terminal-pane-surface"
      className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
    >
      <DagPaneBadge projectPath={session.worktreePath ?? session.cwd} />
      <NativeTerminalPane
        sessionId={session.id}
        session={session}
      />
      {searchOpen ? (
        <TerminalSearchOverlay
          sessionId={session.backendSessionId ?? session.id}
          onClose={onCloseSearch ?? (() => undefined)}
        />
      ) : null}
      {error ? (
        <div className="pointer-events-none absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          {error}
        </div>
      ) : null}
    </div>
  );
}
