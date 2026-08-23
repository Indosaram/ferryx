import { useEffect, useRef, useState } from "react";

import { terminalHostManager } from "../lib/terminalHostManager";
import { fitTerminal } from "../lib/terminalInstanceFactory";
import { useTerminalSettings } from "../lib/terminalSettings";
import type { TerminalSession } from "../lib/types";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
};

export function TerminalPane({ session, active, onBell, onTitleChange, searchOpen, onCloseSearch }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const { settings } = useTerminalSettings();

  useEffect(() => {
    return terminalHostManager.registerVisible(session.id);
  }, [session.id]);

  useEffect(() => {
    terminalHostManager.applyInstanceSettings(session.id, settings);
  }, [session.id, settings]);

  useEffect(() => {
    terminalHostManager.updateSession(session.id, session);
  }, [session]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let cancelled = false;

    void terminalHostManager
      .getOrCreate(session, active, onBell, onTitleChange)
      .then((instance) => {
        if (cancelled) return;
        if (!container.contains(instance.element)) {
          container.replaceChildren(instance.element);
        }
        if (active) {
          instance.terminal.focus();
        }

        const fontsReady = typeof document !== "undefined" ? document.fonts?.ready : undefined;
        void fontsReady?.then(() => {
          if (!cancelled && instance.element.clientWidth > 0 && instance.element.clientHeight > 0) {
            fitTerminal(instance.terminal, instance.fitAddon);
          }
        });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : "Failed to mount terminal");
      });

    return () => {
      cancelled = true;
    };
  }, [session.id]);

  useEffect(() => {
    const instance = terminalHostManager.getInstance(session.id);
    if (instance) {
      instance.active = active;
      if (active) {
        instance.terminal.focus();
      }
    }
  }, [active, session.id]);

  return (
    <div
      data-testid="terminal-pane-surface"
      className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
      style={{ backgroundColor: settings.theme.background }}
    >
      <div
        ref={containerRef}
        data-testid="terminal-mount"
        className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
        style={{ backgroundColor: settings.theme.background }}
      />
      {searchOpen ? (
        <TerminalSearchOverlay
          instance={terminalHostManager.getInstance(session.id)}
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
