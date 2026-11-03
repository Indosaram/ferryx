import { useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";

import type { FilePreviewPayload } from "../../lib/filePreviewTypes";
import type { TranscriptMessage } from "../../lib/supervisorPanel/transcript";
import { FilePreviewText } from "../FilePreviewText";

const NO_OP = () => undefined;
const DOCUMENT_SELECTOR = '[data-testid="file-preview-markdown"], [data-testid="file-preview-source"]';

/**
 * Opens one supervisor report in the app's existing read-only document renderer (rendered
 * Markdown with table of contents, search and a source toggle), in a modal "Viewer". The report
 * is the stored transcript text: nothing is re-read or copied to disk.
 */
export function ReportViewer({ message, onClose }: { message: TranscriptMessage; onClose: () => void }) {
  const [sourceMode, setSourceMode] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const payload = useMemo<FilePreviewPayload>(
    () => ({
      handle: `supervisor-report-${message.id}`,
      displayName: `supervisor-report-${message.id}.md`,
      kind: "markdown",
      byteLength: new TextEncoder().encode(message.text).length,
      encoding: "utf-8",
      mediaType: null,
      mediaUrl: null,
      text: message.text,
      lineCount: message.text.split("\n").length,
      target: null,
      resolvedPath: null,
    }),
    [message],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  // The document takes focus so End, Page Down and the arrows scroll the report at once.
  useEffect(() => {
    const document = bodyRef.current?.querySelector<HTMLElement>(DOCUMENT_SELECTOR);
    if (!document) return;
    if (!document.hasAttribute("tabindex")) document.tabIndex = -1;
    document.focus();
  }, [sourceMode]);

  return (
    <div className="fixed inset-0 z-50 flex items-stretch justify-center bg-black/60 p-2 backdrop-blur-sm sm:p-6">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Viewer"
        data-testid="report-viewer"
        className="flex min-h-0 w-full max-w-4xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
      >
        <header className="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2 text-sm font-semibold text-foreground">
          <span className="min-w-0 flex-1 truncate">Viewer · Supervisor report</span>
          <button
            type="button"
            aria-label="Close Viewer"
            onClick={onClose}
            className="grid size-11 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring sm:size-8"
          >
            <X aria-hidden className="size-4" />
          </button>
        </header>
        <div ref={bodyRef} className="min-h-0 flex-1">
          <FilePreviewText
            payload={payload}
            generation={1}
            onReload={NO_OP}
            onExternalOpen={NO_OP}
            onFailure={NO_OP}
            target={null}
            sourceMode={sourceMode}
            onSourceModeChange={setSourceMode}
            markdown={null}
          />
        </div>
      </div>
    </div>
  );
}
