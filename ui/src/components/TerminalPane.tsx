import { useEffect, useRef, useState } from "react";

import { terminalHostManager, type TerminalInstance } from "../lib/terminalHostManager";
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

function fitMountedTerminal(container: HTMLDivElement, instance: TerminalInstance) {
  if (!container.isConnected || !container.contains(instance.element)) return false;
  const rect = container.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return false;

  Object.assign(instance.element.style, {
    position: "absolute",
    inset: "0px",
    width: "100%",
    height: "100%",
    minWidth: "0px",
    minHeight: "0px",
    overflow: "hidden",
  });
  instance.fitAddon.fit();
  return true;
}

export function TerminalPane({ session, active, onBell, onTitleChange, searchOpen, onCloseSearch }: TerminalPaneProps) {
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
    let firstFitFrame = 0;
    let secondFitFrame = 0;
    let resizeObserver: ResizeObserver | null = null;

    const cancelScheduledFit = () => {
      if (firstFitFrame) cancelAnimationFrame(firstFitFrame);
      if (secondFitFrame) cancelAnimationFrame(secondFitFrame);
      firstFitFrame = 0;
      secondFitFrame = 0;
    };

    const scheduleStableFit = (instance: TerminalInstance, focus = false) => {
      cancelScheduledFit();
      firstFitFrame = requestAnimationFrame(() => {
        firstFitFrame = 0;
        if (cancelled) return;
        const fitted = fitMountedTerminal(container, instance);
        if (focus && fitted) instance.terminal.focus();
        secondFitFrame = requestAnimationFrame(() => {
          secondFitFrame = 0;
          if (cancelled) return;
          fitMountedTerminal(container, instance);
        });
      });
    };

    void terminalHostManager
      .getOrCreate(session, active, onBell, onTitleChange)
      .then((instance) => {
        if (cancelled) return;
        if (!container.contains(instance.element)) {
          container.replaceChildren(instance.element);
        }

        resizeObserver = new ResizeObserver(() => scheduleStableFit(instance));
        resizeObserver.observe(container);
        scheduleStableFit(instance, active);

        const fontsReady = typeof document !== "undefined" ? document.fonts?.ready : undefined;
        void fontsReady?.then(() => {
          if (!cancelled) scheduleStableFit(instance);
        });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : "Failed to mount terminal");
      });

    return () => {
      cancelled = true;
      resizeObserver?.disconnect();
      cancelScheduledFit();
    };
  }, [session.id]);

  useEffect(() => {
    const instance = terminalHostManager.getInstance(session.id);
    const container = containerRef.current;
    if (instance) {
      instance.active = active;
      if (active && container) {
        requestAnimationFrame(() => {
          if (fitMountedTerminal(container, instance)) instance.terminal.focus();
        });
      }
    }
  }, [active, session.id]);

  return (
    <div className="relative h-full w-full min-h-0 min-w-0 overflow-hidden bg-terminal">
      <div ref={containerRef} data-testid="terminal-mount" className="relative h-full w-full min-h-0 min-w-0 overflow-hidden" />
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
