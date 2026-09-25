import { useState } from "react";
import type { BrowserDesignSnapshot, DesignFeedbackTarget } from "../lib/browserTauri";

export interface BrowserDesignFeedbackPopoverProps {
  snapshot: BrowserDesignSnapshot;
  targets: DesignFeedbackTarget[];
  sending: boolean;
  error: string | null;
  onSend: (request: { sessionId: string; memo: string }) => void;
  onCancel: () => void;
}

export function BrowserDesignFeedbackPopover({
  snapshot,
  targets,
  sending,
  error,
  onSend,
  onCancel,
}: BrowserDesignFeedbackPopoverProps) {
  const [memo, setMemo] = useState("");
  const [targetSessionId, setTargetSessionId] = useState(targets[0]?.sessionId ?? "");
  const first = snapshot.dom_elements[0];
  const summary = first
    ? `${first.tag}#${first.id} ${first.bounds[2]}x${first.bounds[3]} at (${first.bounds[0]}, ${first.bounds[1]})`
    : "capture only";
  const canSend = memo.trim().length > 0 && targetSessionId !== "" && !sending;

  return (
    <div data-testid="design-feedback-popover" className="border-t border-border/70 bg-popover px-3 py-2 text-xs shadow-lg">
      <div className="flex flex-col gap-2">
        <div className="flex items-start gap-2">
          <img
            alt="Selected element capture"
            className="max-h-24 w-auto rounded border border-border"
            src={`data:image/png;base64,${snapshot.screenshot_png_base64}`}
          />
          <div className="flex flex-col gap-1 text-muted-foreground">
            <span className="font-mono text-[11px] text-foreground">{summary}</span>
          </div>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="design-feedback-target-select" className="text-muted-foreground">
            Target
          </label>
          {targets.length === 0 ? (
            <p role="status" className="text-muted-foreground">
              No local terminal session in this workspace can receive the feedback.
            </p>
          ) : (
            <select
              id="design-feedback-target-select"
              aria-label="Delivery target"
              className="rounded border border-border bg-background px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
              value={targetSessionId}
              onChange={(e) => setTargetSessionId(e.target.value)}
            >
              {targets.map((target) => (
                <option key={target.sessionId} value={target.sessionId}>
                  {target.label}
                </option>
              ))}
            </select>
          )}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="design-feedback-memo-input" className="text-muted-foreground">
            Note
          </label>
          <textarea
            id="design-feedback-memo-input"
            aria-label="Design feedback note"
            rows={3}
            className="rounded border border-border bg-background px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            placeholder="Describe what needs to change..."
            value={memo}
            onChange={(e) => setMemo(e.target.value)}
          />
        </div>

        {error ? (
          <div role="alert" className="text-destructive">
            {error}
          </div>
        ) : null}

        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            className="rounded border border-border px-2 py-1 text-xs hover:bg-muted"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded border border-border bg-primary px-2 py-1 text-xs text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
            disabled={!canSend}
            onClick={() => onSend({ sessionId: targetSessionId, memo: memo.trim() })}
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
