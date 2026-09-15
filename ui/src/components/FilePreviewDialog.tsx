/**
 * Read-only file preview modal (plan task 2).
 *
 * The dialog owns presentation and focus containment only: request state,
 * generations and capability lifetime live in `lib/filePreview.ts`. Content
 * rendering is injected through `renderers` — plan tasks 3-5 own the actual
 * text/Markdown, image and video renderers, and this file deliberately ships no
 * stand-in for them. Mounting it in the desktop root is plan task 6.
 */
import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type ComponentType, type ReactElement } from "react";
import { ExternalLink, RotateCcw, X } from "lucide-react";

import { filePreviewController, type FilePreviewController, type FilePreviewNotice, type FilePreviewState } from "../lib/filePreview";
import type {
  FilePreviewErrorReason,
  FilePreviewImageProps,
  FilePreviewPayload,
  FilePreviewSource,
  FilePreviewTextProps,
  FilePreviewVideoProps,
} from "../lib/filePreviewTypes";
import { cn } from "../lib/cn";

/** Renderer injection. Text and Markdown share task 3's component contract. */
export type FilePreviewRenderSlots = {
  readonly text?: ComponentType<FilePreviewTextProps>;
  readonly markdown?: ComponentType<FilePreviewTextProps>;
  readonly image?: ComponentType<FilePreviewImageProps>;
  readonly video?: ComponentType<FilePreviewVideoProps>;
};

export type FilePreviewDialogProps = {
  readonly controller?: FilePreviewController;
  readonly renderers?: FilePreviewRenderSlots;
  /** Focus target when the originating pane is no longer mounted. */
  readonly focusFallbackSelector?: string;
};

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusableWithin(root: HTMLElement): readonly HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => element.offsetParent !== null || element.isConnected,
  );
}

function escapeAttributeValue(value: string): string {
  return typeof CSS !== "undefined" && typeof CSS.escape === "function"
    ? CSS.escape(value)
    : value.replace(/["\\]/g, "\\$&");
}

function focusFallback(selector: string): void {
  const fallback = document.querySelector<HTMLElement>(selector);
  if (!fallback) {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    return;
  }
  if (!fallback.hasAttribute("tabindex")) fallback.tabIndex = -1;
  fallback.focus();
}

/**
 * Restores focus to the frontend sink the preview was opened from.
 *
 * `sessionId`/`leafId` is the frontend focus identity; `backendSessionId` never
 * participates here. When the originating leaf is gone, focus goes to workspace
 * chrome — never to whatever other terminal happens to be mounted.
 */
function restoreFocus(
  previous: HTMLElement | null,
  source: FilePreviewSource | null,
  fallbackSelector: string,
): void {
  if (!source) {
    focusFallback(fallbackSelector);
    return;
  }
  const leaf = document.querySelector<HTMLElement>(`[data-leaf-id="${escapeAttributeValue(source.leafId)}"]`);
  if (!leaf) {
    focusFallback(fallbackSelector);
    return;
  }
  if (previous && previous.isConnected && leaf.contains(previous)) {
    previous.focus();
    return;
  }
  const candidate = focusableWithin(leaf)[0];
  if (candidate) {
    candidate.focus();
    return;
  }
  if (!leaf.hasAttribute("tabindex")) leaf.tabIndex = -1;
  leaf.focus();
}

function formatBytes(byteLength: number): string {
  if (byteLength < 1024) return `${byteLength} B`;
  if (byteLength < 1024 * 1024) return `${(byteLength / 1024).toFixed(1)} KiB`;
  return `${(byteLength / (1024 * 1024)).toFixed(1)} MiB`;
}

/** Machine reason to user copy. Branching is on the reason, never on backend prose. */
function failureHeadline(reason: FilePreviewErrorReason | null): string {
  switch (reason) {
    case "MissingFile":
      return "This file no longer exists.";
    case "PermissionDenied":
      return "This file cannot be read with the current permissions.";
    case "NotRegularFile":
      return "Only regular files can be previewed.";
    case "RemoteUnsupported":
      return "Files on remote sessions cannot be previewed on this machine.";
    case "TooLarge":
      return "This file is too large to preview.";
    case "UnsupportedEncoding":
      return "This file is not valid UTF-8 or BOM UTF-16 text.";
    case "UnsupportedFormat":
      return "This format cannot be previewed.";
    case "FileChanged":
      return "This file changed on disk. Reload to preview the current contents.";
    case "ExpiredHandle":
      return "This preview expired. Reload to request it again.";
    case null:
      return "This file could not be previewed.";
    default: {
      const unreachable: never = reason;
      return unreachable;
    }
  }
}

function noticeCopy(notice: FilePreviewNotice): string {
  switch (notice.kind) {
    case "child-document-unsupported":
      return `Linked documents cannot be previewed yet: “${notice.relativePath}” would have to be opened outside this document's capability boundary. Use the external editor action instead.`;
    default: {
      const unreachable: never = notice.kind;
      return unreachable;
    }
  }
}

function titleFor(state: FilePreviewState): string {
  switch (state.status) {
    case "closed":
      return "File preview";
    case "loading":
      return "Opening preview";
    case "ready":
      return state.payload.displayName;
    case "failed":
      return "Preview unavailable";
    default: {
      const unreachable: never = state;
      return unreachable;
    }
  }
}

export function FilePreviewDialog({
  controller = filePreviewController,
  renderers,
  focusFallbackSelector = "[data-workspace-chrome]",
}: FilePreviewDialogProps): ReactElement | null {
  const state = useSyncExternalStore(controller.subscribe, controller.getState, controller.getState);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const sourceRef = useRef<FilePreviewSource | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [sourceMode, setSourceMode] = useState(false);

  const isOpen = state.status !== "closed";
  const generation = state.status === "closed" ? 0 : state.generation;
  if (state.status !== "closed") sourceRef.current = state.source;

  // A new request starts as a rendered document again; the toggle is per preview.
  useEffect(() => {
    setSourceMode(false);
  }, [generation]);

  useEffect(() => {
    if (!isOpen) return;
    previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    if (dialog) (focusableWithin(dialog)[0] ?? dialog).focus();
    return () => {
      restoreFocus(previousFocusRef.current, sourceRef.current, focusFallbackSelector);
      previousFocusRef.current = null;
    };
  }, [isOpen, focusFallbackSelector]);

  const close = useCallback(() => {
    void controller.close();
  }, [controller]);

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      // Modal keys belong to the modal: nothing reaches the terminal sink below.
      event.stopPropagation();
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        return;
      }
      if (event.key !== "Tab") return;
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusables = focusableWithin(dialog);
      if (focusables.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = focusables[0]!;
      const last = focusables[focusables.length - 1]!;
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialog.contains(active))) {
        event.preventDefault();
        last.focus();
        return;
      }
      if (!event.shiftKey && (active === last || !dialog.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    },
    [close],
  );

  const stopKeys = useCallback((event: React.SyntheticEvent) => {
    event.stopPropagation();
  }, []);

  if (state.status === "closed") return null;

  const payload: FilePreviewPayload | null = state.status === "ready" ? state.payload : null;
  const rendererProps = {
    generation: state.generation,
    onReload: () => {
      void controller.reload();
    },
    onExternalOpen: () => {
      void controller.openExternal();
    },
    onFailure: (failure: Parameters<FilePreviewController["reportFailure"]>[1]) => {
      controller.reportFailure(state.generation, failure);
    },
  };

  function renderBody(open: Exclude<FilePreviewState, { status: "closed" }>): ReactElement {
    switch (open.status) {
      case "loading":
        return (
          <div
            data-testid="file-preview-loading"
            className="flex flex-1 items-center justify-center p-6 text-[12px] text-muted-foreground"
          >
            Opening preview…
          </div>
        );
      case "failed":
        return (
          <div
            data-testid="file-preview-failure"
            data-reason={open.failure.reason ?? "unknown"}
            className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center"
          >
            <p className="text-[13px] font-semibold text-foreground">{failureHeadline(open.failure.reason)}</p>
            <p className="selectable max-w-prose break-words text-[11px] text-muted-foreground">{open.failure.message}</p>
            <div className="flex flex-wrap items-center justify-center gap-2">
              <button
                type="button"
                data-testid="file-preview-reload"
                onClick={() => void controller.reload()}
                className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-[11px] text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <RotateCcw className="size-3.5" aria-hidden />
                Reload
              </button>
            </div>
          </div>
        );
      case "ready": {
        const kind = open.payload.kind;
        if (kind === "text" || kind === "markdown") {
          const TextRenderer = kind === "markdown" ? renderers?.markdown : renderers?.text;
          if (!TextRenderer) return renderMissingRenderer(open.payload);
          return (
            <TextRenderer
              {...rendererProps}
              payload={open.payload}
              target={open.payload.target}
              sourceMode={sourceMode}
              onSourceModeChange={setSourceMode}
              markdown={kind === "markdown" ? controller.markdownCapability() : null}
            />
          );
        }
        if (kind === "image") {
          const ImageRenderer = renderers?.image;
          if (!ImageRenderer) return renderMissingRenderer(open.payload);
          return <ImageRenderer {...rendererProps} payload={open.payload} />;
        }
        const VideoRenderer = renderers?.video;
        if (!VideoRenderer) return renderMissingRenderer(open.payload);
        return <VideoRenderer {...rendererProps} payload={open.payload} />;
      }
      default: {
        const unreachable: never = open;
        return unreachable;
      }
    }
  }

  function renderMissingRenderer(ready: FilePreviewPayload): ReactElement {
    return (
      <div
        data-testid="file-preview-no-renderer"
        data-kind={ready.kind}
        className="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center"
      >
        <p className="text-[13px] font-semibold text-foreground">No preview renderer is registered for {ready.kind} content.</p>
        <p className="text-[11px] text-muted-foreground">
          {formatBytes(ready.byteLength)} · open the file in your editor instead.
        </p>
      </div>
    );
  }

  return (
    <div
      data-testid="file-preview-backdrop"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-2 backdrop-blur-sm sm:p-4"
      onMouseDown={close}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={`File preview: ${titleFor(state)}`}
        data-testid="file-preview-dialog"
        tabIndex={-1}
        className={cn(
          "flex h-full max-h-[92vh] w-full max-w-4xl flex-col overflow-hidden rounded-lg border border-border bg-card shadow-2xl",
          "focus-visible:outline-none",
        )}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={onKeyDown}
        onKeyUp={stopKeys}
      >
        <header className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2">
          <div className="min-w-0 flex-1">
            <p className="truncate text-[13px] font-semibold text-foreground" title={titleFor(state)}>
              {titleFor(state)}
            </p>
            <p className="truncate text-[10px] uppercase tracking-wide text-muted-foreground">
              {payload
                ? `read-only · ${payload.kind}${payload.encoding ? ` · ${payload.encoding}` : ""} · ${formatBytes(payload.byteLength)}${
                    payload.lineCount !== null ? ` · ${payload.lineCount} lines` : ""
                  }`
                : "read-only preview"}
            </p>
          </div>
          <button
            type="button"
            data-testid="file-preview-external"
            onClick={() => void controller.openExternal()}
            className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-[11px] text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ExternalLink className="size-3.5" aria-hidden />
            Open externally
          </button>
          <button
            type="button"
            aria-label="Close preview"
            title="Close preview"
            data-testid="file-preview-close"
            onClick={close}
            className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X className="size-4" aria-hidden />
          </button>
        </header>

        {state.status === "ready" && state.notice ? (
          <p
            data-testid="file-preview-notice"
            data-notice={state.notice.kind}
            role="status"
            className="selectable border-b border-border bg-accent/40 px-3 py-2 text-[11px] text-muted-foreground"
          >
            {noticeCopy(state.notice)}
          </p>
        ) : null}

        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{renderBody(state)}</div>
      </div>
    </div>
  );
}
