import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { ChevronDown, ExternalLink, FileDown, RotateCcw } from "lucide-react";
import { toast } from "./ui/sonner";

import { FilePreviewAudio } from "./FilePreviewAudio";
import { FilePreviewCsv } from "./FilePreviewCsv";
import { FilePreviewImage } from "./FilePreviewImage";
import { FilePreviewNotebook } from "./FilePreviewNotebook";
import { FilePreviewPdf } from "./FilePreviewPdf";
import { FilePreviewText } from "./FilePreviewText";
import { FilePreviewVideo } from "./FilePreviewVideo";
import type { FilePreviewController, FilePreviewState } from "../lib/filePreview";
import { checkFilePreviewChanged } from "../lib/filePreviewCommands";
import { getFilePreview, retainFilePreview, subscribeFilePreviews } from "../lib/filePreviewTabRegistry";
import type { FilePreviewOpenRequest, FilePreviewSource } from "../lib/filePreviewTypes";
import { filePreviewErrorReason } from "../lib/filePreviewTypes";
import { failureHeadline } from "../lib/filePreviewFailureCopy";
import { extractIpcErrorMessage } from "../lib/sshHosts";
import { openPermissionsSystemSettings } from "../lib/tauri";
import { listOpenWithApps, openWithApp, printCurrentWebview, type OpenWithApps } from "../lib/fileOpenWith";
import "../styles/filePreviewPrint.css";

export type FilePreviewPaneProps = {
  previewId: string;
  path: string;
  backendSessionId: string;
  line: number | null;
  col: number | null;
  workspaceId: string | null;
  /**
   * Whether the pane is currently shown. Hidden (inactive or restored) tabs neither
   * read the file nor poll for disk changes until they are shown. Defaults to true.
   */
  visible?: boolean;
};

export function FilePreviewPane(props: FilePreviewPaneProps) {
  const visible = props.visible ?? true;
  const request: FilePreviewOpenRequest = {
    path: props.path,
    backendSessionId: props.backendSessionId,
    line: props.line,
    col: props.col,
  };
  const source: FilePreviewSource = {
    leafId: props.previewId,
    sessionId: props.previewId,
    backendSessionId: props.backendSessionId,
    workspaceId: props.workspaceId,
  };
  const controller = useSyncExternalStore(
    (listener) => {
      const unsubscribeRegistry = subscribeFilePreviews(listener);
      const unsubscribeController = getFilePreview(props.previewId)?.subscribe(listener);
      return () => {
        unsubscribeRegistry();
        unsubscribeController?.();
      };
    },
    () => getFilePreview(props.previewId),
    () => null,
  );
  useEffect(() => {
    // A hidden tab must not read its file: open lazily the first time it is shown.
    if (!visible) return;
    if (getFilePreview(props.previewId)) return;
    retainFilePreview(props.previewId, source, request);
  }, [visible, props.previewId, props.path, props.backendSessionId, props.line, props.col, props.workspaceId]);
  if (!controller) {
    return <div data-testid="file-preview-pane-missing" className="h-full w-full" />;
  }
  return (
    <FilePreviewPaneBody previewId={props.previewId} path={props.path} controller={controller} visible={visible} />
  );
}

function FilePreviewPaneBody({
  previewId,
  path,
  controller,
  visible,
}: {
  previewId: string;
  path: string;
  controller: FilePreviewController;
  visible: boolean;
}) {
  const state = useSyncExternalStore(controller.subscribe, controller.getState, controller.getState);
  const [sourceMode, setSourceMode] = useState(false);
  const [hasDiskChange, setHasDiskChange] = useState(false);
  const [openWithOpen, setOpenWithOpen] = useState(false);
  const [loadedForPath, setLoadedForPath] = useState<string | null>(null);
  const [appsData, setAppsData] = useState<OpenWithApps>({ apps: [], supportsChooser: false });
  const [isLoadingApps, setIsLoadingApps] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const openWithTriggerRef = useRef<HTMLButtonElement | null>(null);
  const openWithMenuRef = useRef<HTMLDivElement | null>(null);
  const hiddenWhileReadyRef = useRef(false);

  useEffect(() => {
    setHasDiskChange(false);
  }, [state.status === "closed" ? 0 : state.generation]);

  useEffect(() => {
    if (state.status !== "ready") return;
    // Only the shown tab listens for focus/visibility, so N open tabs cost one check per focus.
    if (!visible) {
      hiddenWhileReadyRef.current = true;
      return;
    }
    const handle = state.payload.handle;

    const onCheckChange = () => {
      checkFilePreviewChanged(handle)
        .then((changed: boolean) => {
          if (changed) {
            setHasDiskChange(true);
          }
        })
        .catch((error: unknown) => {
          // A tab can outlive its backend capability (15 min idle lease). Once the
          // handle is revoked every later read 404s, so reopen instead of leaving a
          // dead view that never reports changes again.
          if (filePreviewErrorReason(error) === "ExpiredHandle") void controller.reload();
        });
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        onCheckChange();
      }
    };

    window.addEventListener("focus", onCheckChange);
    document.addEventListener("visibilitychange", handleVisibilityChange);

    // The file may have changed while this tab was hidden: check once on reveal.
    if (hiddenWhileReadyRef.current) {
      hiddenWhileReadyRef.current = false;
      onCheckChange();
    }

    return () => {
      window.removeEventListener("focus", onCheckChange);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [state.status === "ready" ? state.payload.handle : null, controller, visible]);

  const filePath =
    state.status === "ready"
      ? ((state.payload as { resolvedPath?: string | null }).resolvedPath ?? path)
      : path;

  useEffect(() => {
    if (!openWithOpen) return;

    const handlePointerDown = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setOpenWithOpen(false);
      }
    };

    document.addEventListener("mousedown", handlePointerDown);

    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
    };
  }, [openWithOpen]);

  // Opening the menu moves focus into it, as a menu button should.
  useEffect(() => {
    if (!openWithOpen) return;
    openWithMenuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [openWithOpen]);

  const handleOpenWithKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (!openWithOpen) return;
      if (event.key === "Escape") {
        // Escape belongs to the menu here, not to the pane or tab around it.
        event.preventDefault();
        event.stopPropagation();
        setOpenWithOpen(false);
        openWithTriggerRef.current?.focus();
        return;
      }
      const items = Array.from(
        openWithMenuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [],
      );
      if (items.length === 0) return;
      const current = items.indexOf(document.activeElement as HTMLElement);
      let next: number;
      if (event.key === "ArrowDown") next = current < 0 ? 0 : (current + 1) % items.length;
      else if (event.key === "ArrowUp") next = current < 0 ? items.length - 1 : (current - 1 + items.length) % items.length;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = items.length - 1;
      else return;
      event.preventDefault();
      items[next]?.focus();
    },
    [openWithOpen],
  );

  const handleToggleOpenWith = useCallback(() => {
    const next = !openWithOpen;
    setOpenWithOpen(next);
    if (next && loadedForPath !== filePath) {
      setIsLoadingApps(true);
      listOpenWithApps(filePath)
        .then((data) => {
          setAppsData(data);
          setLoadedForPath(filePath);
        })
        .catch((error) => {
          toast.error(extractIpcErrorMessage(error, "Could not open the file."));
          setAppsData({ apps: [], supportsChooser: false });
          setLoadedForPath(filePath);
        })
        .finally(() => {
          setIsLoadingApps(false);
        });
    }
  }, [openWithOpen, filePath, loadedForPath]);

  const handleSelectApp = useCallback(
    async (appId: string) => {
      setOpenWithOpen(false);
      try {
        await openWithApp(filePath, appId);
      } catch (error) {
        toast.error(extractIpcErrorMessage(error, "Could not open the file."));
      }
    },
    [filePath],
  );

  const handleOpenDefault = useCallback(async () => {
    setOpenWithOpen(false);
    try {
      await controller.openExternal();
    } catch (error) {
      toast.error(extractIpcErrorMessage(error, "Could not open the file."));
    }
  }, [controller]);

  const handleExportPdf = useCallback(async () => {
    try {
      await printCurrentWebview();
    } catch (error) {
      toast.error(extractIpcErrorMessage(error, "Could not print preview."));
    }
  }, []);

  // Every hook above runs on every render: the closed state returns only here,
  // so a closed <-> loading transition never changes the hook order.
  if (state.status === "closed") {
    return <div data-testid="file-preview-pane-pending" data-status="closed" className="h-full w-full" />;
  }
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

  return (
    <div data-testid="file-preview-pane" data-preview-id={previewId} className="flex h-full min-h-0 w-full flex-col bg-background">
      <div className="flex h-8 shrink-0 items-center justify-between border-b border-border px-2">
        <span
          data-testid="file-preview-path"
          title={filePath}
          className="truncate text-[11px] text-muted-foreground mr-2"
        >
          {filePath}
        </span>
        <div className="flex items-center gap-2 shrink-0">
          {state.status === "ready" && state.payload.kind === "markdown" && (
            <button
              type="button"
              data-testid="file-preview-export-pdf"
              className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
              onClick={handleExportPdf}
            >
              <FileDown className="size-3" aria-hidden />
              Export PDF
            </button>
          )}
          {state.status === "ready" && (
            <button
              type="button"
              data-testid="file-preview-header-reload"
              className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
              onClick={() => void controller.reload()}
            >
              <RotateCcw className="size-3" aria-hidden />
              Reload
            </button>
          )}
          <button
            type="button"
            data-testid="file-preview-open-external"
            className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
            onClick={() => void controller.openExternal()}
          >
            <ExternalLink className="size-3" aria-hidden />
            Open externally
          </button>
          <div className="relative inline-block" ref={menuRef} onKeyDown={handleOpenWithKeyDown}>
            <button
              ref={openWithTriggerRef}
              type="button"
              data-testid="file-preview-open-with"
              aria-haspopup="menu"
              aria-expanded={openWithOpen}
              className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
              onClick={handleToggleOpenWith}
            >
              <span>Open with</span>
              <ChevronDown className="size-3" aria-hidden />
            </button>
            {openWithOpen && (
              <div
                ref={openWithMenuRef}
                role="menu"
                aria-label="Open with"
                className="absolute right-0 top-full z-50 mt-1 min-w-[180px] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md"
              >
                <button
                  type="button"
                  role="menuitem"
                  className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-[12px] hover:bg-accent hover:text-accent-foreground"
                  onClick={handleOpenDefault}
                >
                  Default app
                </button>
                {isLoadingApps ? (
                  <div
                    role="presentation"
                    className="px-2 py-1.5 text-[12px] text-muted-foreground"
                  >
                    Loading…
                  </div>
                ) : (
                  <>
                    {appsData.apps.map((app) => (
                      <button
                        key={app.id}
                        type="button"
                        role="menuitem"
                        className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-[12px] hover:bg-accent hover:text-accent-foreground"
                        onClick={() => handleSelectApp(app.id)}
                      >
                        {app.name}
                      </button>
                    ))}
                    {appsData.apps.length === 0 && (
                      <div
                        role="presentation"
                        className="px-2 py-1.5 text-[12px] text-muted-foreground"
                      >
                        No other apps found
                      </div>
                    )}
                    {appsData.supportsChooser && (
                      <button
                        type="button"
                        role="menuitem"
                        className="flex w-full items-center rounded-sm border-t border-border mt-1 pt-1.5 px-2 py-1.5 text-left text-[12px] hover:bg-accent hover:text-accent-foreground"
                        onClick={() => handleSelectApp("chooser")}
                      >
                        Choose application...
                      </button>
                    )}
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
      {hasDiskChange && state.status === "ready" && (
        <div
          data-testid="file-preview-changed-banner"
          className="flex h-8 shrink-0 items-center justify-between border-b border-amber-500/20 bg-amber-500/10 px-3 text-[12px] text-amber-200"
        >
          <span>This file changed on disk.</span>
          <button
            type="button"
            data-testid="file-preview-changed-reload"
            className="inline-flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-amber-200 hover:bg-amber-500/20"
            onClick={() => void controller.reload()}
          >
            <RotateCcw className="size-3" aria-hidden />
            Reload
          </button>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-hidden">
        {state.status === "failed" ? (
          <div data-testid="file-preview-failure" data-reason={state.failure.reason ?? "unknown"} className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
            <p className="text-[13px] font-semibold text-foreground">{failureHeadline(state.failure.reason)}</p>
            <p className="selectable max-w-prose break-words text-[11px] text-muted-foreground">{state.failure.message}</p>
            {state.failure.details?.fullDiskAccessRequired === true ? (
              <div data-testid="file-preview-fda-explainer" className="flex max-w-prose flex-col items-center gap-2 rounded-md border border-border bg-muted/20 px-4 py-3">
                <p className="text-[12px] text-foreground">
                  macOS blocked this read because the file is in a protected folder. Turn on Full Disk Access for Ferryx, then reload.
                </p>
                <button type="button" data-testid="file-preview-open-fda-settings" className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-[11px]" onClick={() => void openPermissionsSystemSettings("full_disk_access").catch(() => {})}>
                  <ExternalLink className="size-3.5" aria-hidden />
                  Open System Settings
                </button>
              </div>
            ) : null}
            <button type="button" data-testid="file-preview-reload" className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-[11px]" onClick={() => void controller.reload()}>
              <RotateCcw className="size-3.5" aria-hidden />
              Reload
            </button>
          </div>
        ) : state.status === "loading" ? (
          <div data-testid="file-preview-loading" className="flex h-full items-center justify-center text-[12px] text-muted-foreground">Opening preview…</div>
        ) : (
          <ReadyPreview state={state} controller={controller} sourceMode={sourceMode} onSourceModeChange={setSourceMode} rendererProps={rendererProps} />
        )}
      </div>
    </div>
  );
}

function ReadyPreview({
  state,
  controller,
  sourceMode,
  onSourceModeChange,
  rendererProps,
}: {
  state: Extract<FilePreviewState, { status: "ready" }>;
  controller: FilePreviewController;
  sourceMode: boolean;
  onSourceModeChange: (sourceMode: boolean) => void;
  rendererProps: {
    generation: number;
    onReload: () => void;
    onExternalOpen: () => void;
    onFailure: (failure: Parameters<FilePreviewController["reportFailure"]>[1]) => void;
  };
}) {
  const payload = state.payload;
  if (payload.kind === "text" && payload.text !== null) {
    const lowerName = payload.displayName.toLowerCase();
    if (lowerName.endsWith(".csv") || lowerName.endsWith(".tsv")) {
      return <FilePreviewCsv text={payload.text} displayName={payload.displayName} />;
    }
    if (lowerName.endsWith(".ipynb")) {
      return <FilePreviewNotebook text={payload.text} />;
    }
  }
  if (payload.kind === "text" || payload.kind === "markdown") {
    const preview = (
      <FilePreviewText
        {...rendererProps}
        payload={payload}
        target={payload.target}
        sourceMode={sourceMode}
        onSourceModeChange={onSourceModeChange}
        markdown={payload.kind === "markdown" ? controller.markdownCapability() : null}
      />
    );
    if (payload.kind === "markdown") {
      return (
        <div data-file-preview-print-root className="h-full w-full">
          {preview}
        </div>
      );
    }
    return preview;
  }
  if (payload.kind === "image") return <FilePreviewImage {...rendererProps} payload={payload} />;
  if (payload.kind === "audio") return <FilePreviewAudio {...rendererProps} payload={payload} />;
  if (payload.kind === "pdf") return <FilePreviewPdf {...rendererProps} payload={payload} />;
  return <FilePreviewVideo {...rendererProps} payload={payload} />;
}
