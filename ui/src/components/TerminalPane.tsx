import { useEffect, useRef, useState } from "react";

import { terminalHostManager } from "../lib/terminalHostManager";
import { useTerminalSettings } from "../lib/terminalSettings";
import type { TerminalSession } from "../lib/types";

type TerminalPaneProps = {
  session: TerminalSession;
  active: boolean;
  onBell?: () => void;
  onTitleChange?: (title: string) => void;
};

export function TerminalPane({ session, active, onBell, onTitleChange }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const { settings } = useTerminalSettings();

  useEffect(() => {
    terminalHostManager.applySettings(settings);
  }, [settings]);

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
        requestAnimationFrame(() => {
          instance.fitAddon.fit();
          if (active) instance.terminal.focus();
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
        requestAnimationFrame(() => {
          instance.fitAddon.fit();
          instance.terminal.focus();
        });
      }
    }
  }, [active, session.id]);

  return (
    <div className="relative h-full w-full bg-terminal overflow-hidden">
      <div ref={containerRef} className="h-full w-full" />
      {error ? (
        <div className="pointer-events-none absolute bottom-3 right-3 max-w-error rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
          {error}
        </div>
      ) : null}
    </div>
  );
}
