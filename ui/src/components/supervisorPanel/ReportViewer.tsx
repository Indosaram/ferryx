import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

import type { FilePreviewPayload } from "../../lib/filePreviewTypes";
import type { TranscriptMessage } from "../../lib/supervisorPanel/transcript";
import { FilePreviewText } from "../FilePreviewText";

const NO_OP = () => undefined;
const DOCUMENT_SELECTOR = '[data-testid="file-preview-markdown"], [data-testid="file-preview-source"]';
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Escape typed into the search field (or any text control) belongs to that control, not the modal. */
function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
}

/**
 * Makes every other child of `<body>` inert (and hidden from assistive tech) while the Viewer is
 * open, so focus, taps, scrolling and pull-to-refresh cannot reach the panel behind it. Restores
 * exactly what it changed.
 */
function useInertBackground(host: HTMLElement | null) {
  useLayoutEffect(() => {
    if (!host) return;
    const changed: Array<{ element: Element; inert: boolean; ariaHidden: string | null }> = [];
    for (const element of Array.from(document.body.children)) {
      if (element === host) continue;
      changed.push({ element, inert: element.hasAttribute("inert"), ariaHidden: element.getAttribute("aria-hidden") });
      element.setAttribute("inert", "");
      element.setAttribute("aria-hidden", "true");
    }
    return () => {
      for (const { element, inert, ariaHidden } of changed) {
        if (!inert) element.removeAttribute("inert");
        if (ariaHidden === null) element.removeAttribute("aria-hidden");
        else element.setAttribute("aria-hidden", ariaHidden);
      }
    };
  }, [host]);
}

/**
 * Opens one supervisor report in the app's existing read-only document renderer (rendered
 * Markdown with table of contents, search and a source toggle), in a modal "Viewer". The report
 * is the stored transcript text: nothing is re-read or copied to disk. It is portalled to
 * `<body>` so it sits above the panel's scroller and its pull-to-refresh, traps focus, keeps
 * its keys to itself, and returns focus to the control that opened it.
 */
export function ReportViewer({ message, onClose }: { message: TranscriptMessage; onClose: () => void }) {
  const [sourceMode, setSourceMode] = useState(false);
  const [host, setHost] = useState<HTMLElement | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(
    typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
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

  // The portal host is created once and removed on close.
  useLayoutEffect(() => {
    const element = document.createElement("div");
    element.setAttribute("data-testid", "report-viewer-host");
    document.body.appendChild(element);
    setHost(element);
    return () => {
      element.remove();
    };
  }, []);
  useInertBackground(host);

  // Focus returns to the control that opened the Viewer when it closes.
  useEffect(() => {
    if (!host) return;
    const back = opener.current;
    return () => {
      if (back?.isConnected) back.focus();
    };
  }, [host]);

  // After a Rendered/Source toggle the document element is new: put focus on it so End scrolls it.
  useEffect(() => {
    if (!host) return;
    const document_ = dialogRef.current?.querySelector<HTMLElement>(DOCUMENT_SELECTOR);
    if (!document_) return;
    if (!document_.hasAttribute("tabindex")) document_.tabIndex = -1;
    document_.focus();
  }, [host, sourceMode]);

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      // Modal keys belong to the modal: nothing reaches the terminal or panel below.
      event.stopPropagation();
      if (event.key === "Escape") {
        if (event.defaultPrevented || isTextEntry(event.target)) return;
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialog.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    },
    [onClose],
  );

  // Touch containment. React registers touch handlers as passive, where preventDefault is
  // ignored, so these are native non-passive listeners on the backdrop. A touch on the dim
  // backdrop itself is cancelled (it cannot scroll or pull-to-refresh what is behind), and no
  // touch inside the Viewer bubbles out to page-level handlers; touches inside the dialog are
  // not cancelled, so the report still scrolls.
  const backdropRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const backdrop = backdropRef.current;
    if (!backdrop) return;
    const contain = (event: Event) => {
      if (event.target === backdrop && event.cancelable) event.preventDefault();
      event.stopPropagation();
    };
    const options = { passive: false } as const;
    backdrop.addEventListener("touchstart", contain, options);
    backdrop.addEventListener("touchmove", contain, options);
    return () => {
      backdrop.removeEventListener("touchstart", contain);
      backdrop.removeEventListener("touchmove", contain);
    };
  }, [host]);

  if (!host) return null;
  return createPortal(
    <div
      ref={backdropRef}
      data-testid="report-viewer-backdrop"
      className="fixed inset-0 z-50 flex items-stretch justify-center bg-black/60 p-2 backdrop-blur-sm [touch-action:none] sm:p-6"
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Viewer"
        tabIndex={-1}
        data-testid="report-viewer"
        onKeyDown={onKeyDown}
        onKeyUp={(event) => event.stopPropagation()}
        className="flex min-h-0 w-full max-w-4xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl [overscroll-behavior:contain] [touch-action:pan-y] focus-visible:outline-none"
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
        <div className="min-h-0 flex-1">
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
    </div>,
    host,
  );
}
