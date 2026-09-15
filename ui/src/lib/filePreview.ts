import { openExternalUrl } from "./browserTauri";
/**
 * File-preview request lifecycle (plan task 2).
 *
 * This module owns everything stateful about a preview request: the typed
 * bridge to the three frozen Tauri commands, the monotonic generation that
 * makes replacement and close race-safe, and the capability bookkeeping that
 * guarantees every acquired handle is released exactly once — including handles
 * whose `open` resolves after the modal already moved on.
 *
 * It renders nothing. `FilePreviewDialog.tsx` subscribes to the state, and the
 * content renderers (plan tasks 3-5) receive props from that dialog. Shared
 * entry-point wiring (`App.tsx`, `linkRouting.ts`, `NativeTerminalPane.tsx`) is
 * plan task 6 and is deliberately absent here.
 */
import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";

import { loadFileLinkEditor } from "./fileLinkSettings";
import {
  FILE_PREVIEW_COMMANDS,
  FILE_PREVIEW_LIMITS,
  filePreviewErrorReason,
  isFilePreviewErrorReason,
  type FilePreviewChildAsset,
  type FilePreviewErrorDetails,
  type FilePreviewFailure,
  type FilePreviewHandle,
  type FilePreviewMarkdownCapability,
  type FilePreviewOpenRequest,
  type FilePreviewPayload,
  type FilePreviewSource,
} from "./filePreviewTypes";

/** External-open target: always the ORIGINAL click, never a preview handle. */
export type FilePreviewExternalRequest = {
  readonly path: string;
  readonly backendSessionId: string;
  readonly line: number | null;
  readonly col: number | null;
};

export type FilePreviewDeps = {
  readonly invoke: <T>(command: string, args: Record<string, unknown>) => Promise<T>;
  readonly openExternalFile: (request: FilePreviewExternalRequest) => Promise<void>;
  readonly openExternalUrl: (url: string) => Promise<void>;
};

/**
 * Something the modal must tell the user about that is not a request failure.
 *
 * `child-document-unsupported` records the one contract gap task 2 found: the
 * frozen backend surface has `cmd_file_preview_open_child`, which returns a
 * media `FilePreviewChildAsset` only, and `cmd_file_preview_open`, which takes a
 * free path plus a backend session. Neither can open a Markdown-relative
 * DOCUMENT under the parent handle's canonical boundary. Escalating such a link
 * through `cmd_file_preview_open` would resolve it outside that boundary, so the
 * request is refused and reported instead of silently widening file authority.
 */
export type FilePreviewNotice = {
  readonly kind: "child-document-unsupported";
  readonly relativePath: string;
};

export type FilePreviewState =
  | { readonly status: "closed" }
  | {
      readonly status: "loading";
      readonly generation: number;
      readonly source: FilePreviewSource;
      readonly request: FilePreviewOpenRequest;
    }
  | {
      readonly status: "ready";
      readonly generation: number;
      readonly source: FilePreviewSource;
      readonly request: FilePreviewOpenRequest;
      readonly payload: FilePreviewPayload;
      readonly remainingChildHandles: number;
      readonly notice: FilePreviewNotice | null;
    }
  | {
      readonly status: "failed";
      readonly generation: number;
      readonly source: FilePreviewSource;
      readonly request: FilePreviewOpenRequest;
      readonly failure: FilePreviewFailure;
    };

export type FilePreviewController = {
  getState(): FilePreviewState;
  subscribe(listener: () => void): () => void;
  open(source: FilePreviewSource, request: FilePreviewOpenRequest): Promise<void>;
  reload(): Promise<void>;
  close(): Promise<void>;
  openExternal(): Promise<void>;
  markdownCapability(): FilePreviewMarkdownCapability | null;
  reportFailure(generation: number, failure: FilePreviewFailure): void;
};

/** A refusal raised by the frontend capability layer itself, carrying a machine reason when one applies. */
export class FilePreviewCapabilityError extends Error {
  readonly failure: FilePreviewFailure;
  readonly details: FilePreviewErrorDetails | null;

  constructor(failure: FilePreviewFailure) {
    super(failure.message);
    this.name = "FilePreviewCapabilityError";
    this.failure = failure;
    this.details = failure.details;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Reads a rejected preview call into the modal's failure shape. Never branches on prose. */
export function toFilePreviewFailure(error: unknown, fallbackMessage: string): FilePreviewFailure {
  if (error instanceof FilePreviewCapabilityError) return error.failure;
  const reason = filePreviewErrorReason(error);
  const message = isRecord(error) && typeof error.message === "string" && error.message.length > 0
    ? error.message
    : error instanceof Error && error.message.length > 0
      ? error.message
      : fallbackMessage;
  const rawDetails = isRecord(error) ? error.details : null;
  const details = isRecord(rawDetails) && isFilePreviewErrorReason(rawDetails.reason)
    ? (rawDetails as unknown as FilePreviewErrorDetails)
    : null;
  return { reason, message, details };
}

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

function defaultInvoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    return Promise.reject(
      new FilePreviewCapabilityError({
        reason: null,
        message: "File previews are only available in the desktop app.",
        details: null,
      }),
    );
  }
  return tauriInvoke<T>(command, args);
}

async function defaultOpenExternalFile(request: FilePreviewExternalRequest): Promise<void> {
  if (!isTauri()) {
    throw new FilePreviewCapabilityError({
      reason: null,
      message: "External opening is only available in the desktop app.",
      details: null,
    });
  }
  // The same command the terminal file link uses, with the original click's
  // identity: the backend session resolves the cwd, the editor preference picks
  // the application. No preview handle is involved.
  await tauriInvoke<boolean>("cmd_open_file_path", {
    path: request.path,
    sessionId: request.backendSessionId,
    editor: loadFileLinkEditor(),
    line: request.line ?? undefined,
    col: request.col ?? undefined,
  });
}

async function defaultOpenExternalUrl(url: string): Promise<void> {

  await openExternalUrl(url);
}

export function createFilePreviewController(deps: Partial<FilePreviewDeps> = {}): FilePreviewController {
  const invoke = deps.invoke ?? defaultInvoke;
  const openExternalFile = deps.openExternalFile ?? defaultOpenExternalFile;
  const openExternalUrl = deps.openExternalUrl ?? defaultOpenExternalUrl;

  let generation = 0;
  let state: FilePreviewState = { status: "closed" };
  const listeners = new Set<() => void>();

  /** Handles acquired for the CURRENT generation, plus the set already released. */
  let mainHandle: FilePreviewHandle | null = null;
  let childHandles: FilePreviewHandle[] = [];
  const released = new Set<FilePreviewHandle>();

  function emit(): void {
    for (const listener of [...listeners]) listener();
  }

  function setState(next: FilePreviewState): void {
    state = next;
    emit();
  }

  /** Idempotent release: a handle is closed at most once per acquisition. */
  function release(handle: FilePreviewHandle): void {
    if (released.has(handle)) return;
    released.add(handle);
    void invoke(FILE_PREVIEW_COMMANDS.close, { handle }).catch(() => {
      // close is idempotent backend-side; an unknown or expired handle is not a
      // user-visible failure and must not resurrect a dismissed modal.
    });
  }

  /**
   * Starts a new generation: every capability of the previous one is released
   * and any in-flight reply from it becomes ignorable.
   */
  function beginGeneration(): number {
    generation += 1;
    const stale = [...(mainHandle ? [mainHandle] : []), ...childHandles];
    mainHandle = null;
    childHandles = [];
    for (const handle of stale) release(handle);
    return generation;
  }

  async function runOpen(source: FilePreviewSource, request: FilePreviewOpenRequest): Promise<void> {
    const current = beginGeneration();
    setState({ status: "loading", generation: current, source, request });
    try {
      const payload = await invoke<FilePreviewPayload>(FILE_PREVIEW_COMMANDS.open, {
        path: request.path,
        backendSessionId: request.backendSessionId,
        line: request.line,
        col: request.col,
      });
      if (current !== generation) {
        // Late reply from a superseded or closed request: release it and never
        // show it.
        release(payload.handle);
        return;
      }
      mainHandle = payload.handle;
      setState({
        status: "ready",
        generation: current,
        source,
        request,
        payload,
        remainingChildHandles: FILE_PREVIEW_LIMITS.maxChildHandles,
        notice: null,
      });
    } catch (error) {
      if (current !== generation) return;
      setState({
        status: "failed",
        generation: current,
        source,
        request,
        failure: toFilePreviewFailure(error, "The file could not be previewed."),
      });
    }
  }

  function markdownCapability(): FilePreviewMarkdownCapability | null {
    const snapshot = state;
    if (snapshot.status !== "ready" || snapshot.payload.kind !== "markdown") return null;
    const parentHandle = snapshot.payload.handle;
    const capabilityGeneration = snapshot.generation;

    return {
      remainingChildHandles: snapshot.remainingChildHandles,
      requestImage: async (relativePath: string): Promise<FilePreviewChildAsset> => {
        if (capabilityGeneration !== generation) {
          throw new FilePreviewCapabilityError({
            reason: "ExpiredHandle",
            message: "This preview was replaced before the image could load.",
            details: null,
          });
        }
        const live = state;
        if (live.status !== "ready" || live.remainingChildHandles <= 0) {
          throw new FilePreviewCapabilityError({
            reason: null,
            message: `This document already used its ${FILE_PREVIEW_LIMITS.maxChildHandles} image capabilities.`,
            details: null,
          });
        }
        const asset = await invoke<FilePreviewChildAsset>(FILE_PREVIEW_COMMANDS.openChild, {
          parentHandle,
          relativePath,
        });
        if (capabilityGeneration !== generation) {
          release(asset.handle);
          throw new FilePreviewCapabilityError({
            reason: "ExpiredHandle",
            message: "This preview was replaced before the image could load.",
            details: null,
          });
        }
        childHandles = [...childHandles, asset.handle];
        const after = state;
        if (after.status === "ready" && after.generation === capabilityGeneration) {
          setState({ ...after, remainingChildHandles: after.remainingChildHandles - 1 });
        }
        return asset;
      },
      requestDocument: (relativePath: string): void => {
        if (capabilityGeneration !== generation) return;
        const live = state;
        if (live.status !== "ready" || live.generation !== capabilityGeneration) return;
        // Containment over convenience: no backend command opens a document
        // under the parent handle's boundary, so the request is reported, not
        // rerouted through the unbounded open command.
        setState({ ...live, notice: { kind: "child-document-unsupported", relativePath } });
      },
      openExternalUrl: (url: string): void => {
        if (!isHttpUrl(url)) return;
        void openExternalUrl(url).catch(() => {
          // Anchor activation is best effort; a failed hand-off must not take
          // the document down.
        });
      },
    };
  }

  return {
    getState: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    open: (source, request) => runOpen(source, request),
    reload: async () => {
      const snapshot = state;
      if (snapshot.status === "closed") return;
      await runOpen(snapshot.source, snapshot.request);
    },
    close: async () => {
      beginGeneration();
      if (state.status !== "closed") setState({ status: "closed" });
    },
    openExternal: async () => {
      const snapshot = state;
      if (snapshot.status === "closed") return;
      await openExternalFile({
        path: snapshot.request.path,
        backendSessionId: snapshot.source.backendSessionId,
        line: snapshot.request.line,
        col: snapshot.request.col,
      });
    },
    markdownCapability,
    reportFailure: (reportedGeneration, failure) => {
      const snapshot = state;
      if (snapshot.status === "closed") return;
      if (reportedGeneration !== snapshot.generation) return;
      setState({
        status: "failed",
        generation: snapshot.generation,
        source: snapshot.source,
        request: snapshot.request,
        failure,
      });
    },
  };
}

/** The one preview lifecycle per desktop root. Task 6 mounts the dialog against it. */
export const filePreviewController: FilePreviewController = createFilePreviewController();
