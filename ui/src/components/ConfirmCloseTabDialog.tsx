import { useEffect } from "react";

type ConfirmCloseKind = "pane" | "tab" | "batch";

type ConfirmCloseTabDialogProps = {
  tabLabel: string;
  kind?: ConfirmCloseKind;
  /** Tabs the close would remove; only meaningful for a batch close. */
  tabCount?: number;
  activeAgentCount?: number;
  /** Live backend sessions (shells or agents) the close would destroy. */
  liveSessionCount?: number;
  onCancel: () => void;
  onConfirm: () => void;
};

function titleFor(kind: ConfirmCloseKind): string {
  if (kind === "pane") return "Close pane";
  return kind === "batch" ? "Close tabs" : "Close tab";
}

export function ConfirmCloseTabDialog({
  tabLabel,
  kind = "tab",
  tabCount = 0,
  activeAgentCount = 0,
  liveSessionCount = 0,
  onCancel,
  onConfirm,
}: ConfirmCloseTabDialogProps) {
  const title = titleFor(kind);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCancel();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onCancel]);

  const subject = kind === "batch"
    ? `${tabCount === 1 ? "1 tab" : `${tabCount} tabs`} next to “${tabLabel}”`
    : `${kind === "pane" ? "pane in " : ""}“${tabLabel}”`;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onMouseDown={onCancel}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-elevation-4"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="border-b border-border px-4 py-3 text-sm font-semibold text-foreground">{title}</header>
        <div className="selectable space-y-4 p-4 text-xs">
          <p className="text-muted-foreground">Close {subject}? Its terminal or browser session will be released.</p>
          {liveSessionCount > 0 ? (
            <p className="text-muted-foreground">
              {liveSessionCount === 1 ? "1 running session will be terminated." : `${liveSessionCount} running sessions will be terminated.`}
            </p>
          ) : null}
          {activeAgentCount > 0 ? (
            <p className="text-muted-foreground">
              {activeAgentCount === 1 ? "1 running agent will be terminated." : `${activeAgentCount} running agents will be terminated.`}
            </p>
          ) : null}
          <div className="flex justify-end gap-2">
            <button type="button" onClick={onCancel} className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
              Cancel
            </button>
            <button type="button" onClick={onConfirm} className="rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground">
              {title}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
