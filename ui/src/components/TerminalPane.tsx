import { useState } from "react";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane } from "./NativeTerminalPane";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
};

export function TerminalPane({
  session,
  active: _active,
  onBell,
  onTitleChange,
  searchOpen,
  onCloseSearch,
}: TerminalPaneProps) {
  const [error] = useState<string | null>(null);

  return (
    <div
      data-testid="terminal-pane-surface"
      className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
    >
      <NativeTerminalPane
        sessionId={session.id}
        session={session}
        onBell={onBell}
        onTitleChange={onTitleChange}
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
