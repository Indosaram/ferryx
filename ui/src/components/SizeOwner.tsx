import { useEffect, useState } from "react";
import { describeTerminal, listenToThisWindow, resizeTerminal } from "../lib/tauri";

export type SizeOwnerState = { sessionId: string; owner: string; cols: number; rows: number; desktopCols?: number; desktopRows?: number };

export function isPhoneSizeOwner(owner: string | null | undefined): boolean {
  return owner != null && owner !== "desktop";
}

export function SizeOwner({ sessionId }: { sessionId: string }) {
  const [state, setState] = useState<SizeOwnerState | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    let observed = false;
    let unlisten: (() => void) | undefined;
    setState(null);
    void listenToThisWindow<SizeOwnerState>("SizeOwnerChanged", ({ payload }) => {
      if (!disposed && payload.sessionId === sessionId) {
        observed = true;
        setState(payload);
      }
    }).then(async (stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      const details = await describeTerminal(sessionId);
      if (!disposed && !observed && details) {
        setState({ sessionId, owner: details.sizeOwner ?? "desktop", cols: details.cols, rows: details.rows });
      }
    }).catch((cause: unknown) => { if (!disposed) setError(String(cause)); });
    return () => { disposed = true; unlisten?.(); };
  }, [sessionId]);
  if (!state || !isPhoneSizeOwner(state.owner)) return null;
  return <div className="absolute right-2 top-1 z-30 flex items-center gap-2 rounded bg-background px-2 py-1 text-xs">
    <span>Sized by phone</span>
    <button type="button" onClick={() => {
      // A local resize is an explicit desktop claim, even when dimensions match.
      void resizeTerminal({ sessionId, cols: state.desktopCols ?? state.cols, rows: state.desktopRows ?? state.rows })
        .catch((cause: unknown) => setError(String(cause)));
    }}>Take back</button>
    {error ? <span role="alert">{error}</span> : null}
  </div>;
}
