import { useEffect } from "react";

type ConfirmCloseTabDialogProps = {
  tabLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
};

export function ConfirmCloseTabDialog({ tabLabel, onCancel, onConfirm }: ConfirmCloseTabDialogProps) {
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
        aria-label="Close tab"
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="border-b border-border px-4 py-3 text-sm font-semibold text-foreground">Close tab</header>
        <div className="selectable space-y-4 p-4 text-xs">
          <p className="text-muted-foreground">Close “{tabLabel}”? Its terminal or browser session will be released.</p>
          <div className="flex justify-end gap-2">
            <button type="button" onClick={onCancel} className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
              Cancel
            </button>
            <button type="button" onClick={onConfirm} className="rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground">
              Close tab
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
