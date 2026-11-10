import { useEffect } from "react";

type RestartPaneDialogProps = {
  agentName: string;
  onCancel: () => void;
  /** Restart the pane as the same agent, continuing its provider session. */
  onResume: () => void;
  /** Restart the pane as a plain shell; the agent identity is dropped. */
  onFreshShell: () => void;
  /** False when the provider session cannot be continued; only a plain shell is offered. */
  canResume: boolean;
};

/**
 * Two-step choice for restarting an agent pane. A restart replaces the process, so resuming the
 * provider session and dropping the agent are different outcomes and the user picks explicitly.
 */
export function RestartPaneDialog({ agentName, onCancel, onResume, onFreshShell, canResume }: RestartPaneDialogProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCancel();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onCancel]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onMouseDown={onCancel}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Restart pane"
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-elevation-4"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="border-b border-border px-4 py-3 text-sm font-semibold text-foreground">Restart pane</header>
        <div className="selectable space-y-4 p-4 text-xs">
          <p className="text-muted-foreground">
            This pane is running {agentName}. Restarting replaces the process in place; choose whether the agent
            session continues.
          </p>
          {canResume ? null : (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-amber-600 dark:text-amber-400" data-testid="restart-pane-drop-notice">
              Restarting as a plain shell ends this {agentName} session and drops the pane&apos;s agent
              identity. The conversation cannot be brought back from this pane.
            </p>
          )}
          <div className="flex flex-col gap-2">
            {canResume ? (
              <button
                type="button"
                data-testid="restart-pane-resume"
                onClick={onResume}
                className="rounded-md bg-primary px-3 py-2 font-semibold text-primary-foreground hover:bg-primary/90"
              >
                Restart &amp; resume {agentName}
              </button>
            ) : null}
            <button
              type="button"
              data-testid="restart-pane-shell"
              onClick={onFreshShell}
              className="rounded-md border border-border px-3 py-2 text-foreground hover:bg-accent"
            >
              {canResume ? "Restart as plain shell" : "End session and restart as plain shell"}
            </button>
          </div>
          <div className="flex justify-end">
            <button type="button" onClick={onCancel} className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
