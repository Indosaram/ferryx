import { LoaderCircle, X } from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import { pairRemoteMachine } from "../lib/pairClient";

export type PairMachineModalProps = {
  open: boolean;
  onClose: () => void;
};

export function PairMachineModal({ open, onClose }: PairMachineModalProps) {
  const [codeOrUrl, setCodeOrUrl] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setCodeOrUrl("");
      setError(null);
      setSubmitting(false);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!submitting) {
          setCodeOrUrl("");
          setError(null);
          onClose();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, submitting, onClose]);

  if (!open) return null;

  const handleClose = () => {
    if (submitting) return;
    setCodeOrUrl("");
    setError(null);
    onClose();
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmed = codeOrUrl.trim();
    if (!trimmed || submitting) return;

    setSubmitting(true);
    setError(null);

    try {
      await pairRemoteMachine({ codeOrUrl: trimmed });
      setCodeOrUrl("");
      setError(null);
      onClose();
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg || "Failed to pair remote machine");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={handleClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Pair Remote Machine"
        data-testid="pair-machine-modal"
        className="w-full max-w-md animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-sm font-semibold text-foreground">Pair Remote Machine</h2>
          <button
            type="button"
            aria-label="Close"
            onClick={handleClose}
            disabled={submitting}
            className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </header>

        <form onSubmit={handleSubmit} className="space-y-4 p-4 text-xs">
          <p className="text-muted-foreground leading-relaxed">
            Enter the 6-digit PIN or pairing link shown on your remote server (
            <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">
              ferryx pair generate
            </code>
            ).
          </p>

          {error ? (
            <div
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/10 p-2.5 text-xs text-destructive"
            >
              {error}
            </div>
          ) : null}

          <div>
            <input
              type="text"
              autoFocus
              value={codeOrUrl}
              onChange={(e) => {
                setCodeOrUrl(e.target.value);
                if (error) setError(null);
              }}
              placeholder="e.g. 123456 or https://relay.checka.cc#pair=..."
              disabled={submitting}
              data-testid="pair-machine-input"
              className="h-9 w-full rounded-md border border-input bg-background px-3 text-xs text-foreground placeholder:text-muted-foreground/50 focus:border-ring focus:outline-none disabled:opacity-50"
            />
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={handleClose}
              disabled={submitting}
              className="rounded-md px-3 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={submitting || !codeOrUrl.trim()}
              data-testid="pair-machine-submit"
              className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
            >
              {submitting ? <LoaderCircle className="size-3.5 animate-spin" /> : null}
              <span>{submitting ? "Pairing..." : "Pair Machine"}</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
