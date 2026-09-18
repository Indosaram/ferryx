/**
 * Shared file-preview contracts (preparation segment of plan task 6).
 *
 * This module is DECLARATIONS ONLY: wire DTOs, discriminants, bounds and the
 * structured-error reader that tasks 1-5 compile against. It deliberately owns
 * no request state and performs no IPC — `ui/src/lib/filePreview.ts` (task 2)
 * owns the request lifecycle and the typed bridge, `FilePreview*.tsx` (tasks
 * 3-5) own rendering. Keep this file free of React and of `@tauri-apps/api`
 * imports so every consumer can import it without pulling a runtime in.
 *
 * Wire parity: every type here mirrors `src-tauri/src/ipc/file_preview_contract.rs`.
 * Field names are camelCase on both sides; discriminant spellings are asserted
 * against the Rust source by `filePreviewTypes.test.ts`.
 */

/** Content classes the modal can render. Nothing else is previewable. */
export const FILE_PREVIEW_KINDS = ["text", "markdown", "image", "video"] as const;
export type FilePreviewKind = (typeof FILE_PREVIEW_KINDS)[number];

/**
 * Machine-readable failure reasons carried in `IpcError.details.reason`.
 * Branch on these; never parse `message` prose.
 */
export const FILE_PREVIEW_ERROR_REASONS = [
  "MissingFile",
  "PermissionDenied",
  "NotRegularFile",
  "RemoteUnsupported",
  "TooLarge",
  "UnsupportedEncoding",
  "UnsupportedFormat",
  "FileChanged",
  "ExpiredHandle",
] as const;
export type FilePreviewErrorReason = (typeof FILE_PREVIEW_ERROR_REASONS)[number];

/** Text encodings that decode successfully; anything else is UnsupportedEncoding. */
export const FILE_PREVIEW_ENCODINGS = ["utf-8", "utf-16le", "utf-16be"] as const;
export type FilePreviewEncoding = (typeof FILE_PREVIEW_ENCODINGS)[number];

/** Tauri command names owned by `src-tauri/src/ipc/file_preview.rs` (task 1). */
export const FILE_PREVIEW_COMMANDS = {
  open: "cmd_file_preview_open",
  openChild: "cmd_file_preview_open_child",
  close: "cmd_file_preview_close",
} as const;
export type FilePreviewCommand = (typeof FILE_PREVIEW_COMMANDS)[keyof typeof FILE_PREVIEW_COMMANDS];

/** Frozen resource bounds. Backend enforces them; the frontend never bypasses a refusal. */
export const FILE_PREVIEW_LIMITS = {
  /** Text/Markdown hard ceiling: 2 MiB, reported as TooLarge with no truncation. */
  textMaxBytes: 2 * 1024 * 1024,
  /** Rendered line ceiling before TooLarge. */
  maxRenderedLines: 50_000,
  /** Compressed image ceiling: 32 MiB. */
  imageMaxBytes: 32 * 1024 * 1024,
  /** Decoded image ceiling: 40 megapixels per frame. */
  imageMaxPixels: 40_000_000,
  /** Markdown child image handles per main handle. */
  maxChildHandles: 32,
  /** Concurrent capability media requests per process before 429. */
  maxConcurrentMediaRequests: 8,
  /** HTTP streaming chunk size. */
  streamChunkBytes: 64 * 1024,
} as const;

/** Image zoom bounds for task 4 (1 = 100%). */
export const FILE_PREVIEW_ZOOM = { min: 0.1, max: 8, step: 0.1 } as const;

/** Container extensions recognised per kind. Recognition is not a codec guarantee. */
export const FILE_PREVIEW_EXTENSIONS = {
  markdown: [".md", ".markdown"],
  image: [".png", ".jpg", ".jpeg", ".gif", ".webp"],
  video: [".mp4", ".m4v", ".mov", ".webm", ".ogv"],
} as const;

/**
 * Where a preview was opened from.
 *
 * The identity triad is kept separate on purpose (AGENTS.md "Identity Triad"):
 * `leafId` is the visual pane, `sessionId` is the FRONTEND focus identity used
 * to restore focus on close, and `backendSessionId` is the daemon session whose
 * live cwd resolves relative paths. Focus restoration must never key off
 * `backendSessionId`, and path resolution must never key off `sessionId`.
 */
export type FilePreviewSource = {
  readonly leafId: string;
  readonly sessionId: string;
  readonly backendSessionId: string;
  readonly workspaceId: string | null;
};

/** 1-based Unicode scalar caret target; clamped by the renderer with visible indication. */
export type FilePreviewTarget = {
  readonly line: number;
  readonly col: number | null;
};

/** `cmd_file_preview_open` arguments. */
export type FilePreviewOpenRequest = {
  readonly path: string;
  readonly backendSessionId: string;
  readonly line: number | null;
  readonly col: number | null;
};

/** `cmd_file_preview_open_child` arguments: a Markdown-relative asset under the parent boundary. */
export type FilePreviewChildRequest = {
  readonly parentHandle: string;
  readonly relativePath: string;
};

/** `cmd_file_preview_close` arguments. Idempotent for unknown/expired handles. */
export type FilePreviewCloseRequest = {
  readonly handle: string;
};

/**
 * Opaque capability handle. Carries no filesystem path and is bound to the
 * creating desktop window.
 */
export type FilePreviewHandle = string;

/** `cmd_file_preview_open` result. */
export type FilePreviewPayload = {
  readonly handle: FilePreviewHandle;
  /** File name for display only — never a resolvable path. */
  readonly displayName: string;
  readonly kind: FilePreviewKind;
  readonly byteLength: number;
  /** Non-null for text/markdown only. */
  readonly encoding: FilePreviewEncoding | null;
  /** Allowlisted MIME for media kinds, null for text/markdown. */
  readonly mediaType: string | null;
  /** Loopback capability URL for image/video, null for text/markdown. */
  readonly mediaUrl: string | null;
  /** Bounded decoded document for text/markdown, null for media. */
  readonly text: string | null;
  /** Rendered line count for text/markdown, null for media. */
  readonly lineCount: number | null;
  /** Clamped caret target when the request carried line/col. */
  readonly target: FilePreviewTarget | null;
};

/** `cmd_file_preview_open_child` result: a child capability with no document body. */
export type FilePreviewChildAsset = {
  readonly handle: FilePreviewHandle;
  readonly displayName: string;
  readonly kind: FilePreviewKind;
  readonly byteLength: number;
  readonly mediaType: string;
  readonly mediaUrl: string;
};

/** Structured `details` payload of a preview `IpcError`. */
export type FilePreviewErrorDetails = {
  readonly reason: FilePreviewErrorReason;
  readonly displayName?: string;
  readonly byteLength?: number;
  readonly limit?: number;
};

/** Everything the modal knows about one failed request. */
export type FilePreviewFailure = {
  readonly reason: FilePreviewErrorReason | null;
  readonly message: string;
  readonly details: FilePreviewErrorDetails | null;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/**
 * Reads the machine reason out of a rejected preview IPC call.
 *
 * Returns `null` when the error carries no preview reason, so callers fall back
 * to a generic failure state instead of branching on prose.
 */
export function filePreviewErrorReason(error: unknown): FilePreviewErrorReason | null {
  if (!isRecord(error)) return null;
  const details = error.details;
  if (!isRecord(details)) return null;
  const reason = details.reason;
  return isFilePreviewErrorReason(reason) ? reason : null;
}

/** Type guard for a value that is one of the nine frozen reasons. */
export function isFilePreviewErrorReason(value: unknown): value is FilePreviewErrorReason {
  return (
    typeof value === "string" &&
    (FILE_PREVIEW_ERROR_REASONS as readonly string[]).includes(value)
  );
}

/**
 * Capability API handed to the Markdown renderer (task 3) by the modal (task 2).
 *
 * The renderer never invokes Tauri itself: relative images resolve to child
 * capability URLs under the canonical Markdown parent directory, and relative
 * documents become explicit new preview requests. Remote images, raw HTML,
 * and non-http(s) anchor schemes are rejected before this API is reached.
 */
export type FilePreviewMarkdownCapability = {
  /** Resolves a Markdown-relative image to a child capability URL, or rejects with a reason. */
  readonly requestImage: (relativePath: string) => Promise<FilePreviewChildAsset>;
  /** Escalates a Markdown-relative document link into a new top-level preview request. */
  readonly requestDocument: (relativePath: string) => void;
  /** Opens an http(s) anchor in the external browser; never navigates the modal. */
  readonly openExternalUrl: (url: string) => void;
  /** Remaining child image handles for this parent (starts at `FILE_PREVIEW_LIMITS.maxChildHandles`). */
  readonly remainingChildHandles: number;
};

/** Props every renderer receives from `FilePreviewDialog`. */
export type FilePreviewRendererProps = {
  readonly payload: FilePreviewPayload;
  /** Monotonic request generation; late events from an older generation are ignored. */
  readonly generation: number;
  /** Re-acquires the original file under the same checks (used after FileChanged). */
  readonly onReload: () => void;
  /** Explicit external open of the ORIGINAL path/session/line/col, never preview recursion. */
  readonly onExternalOpen: () => void;
  /** Renderer-detected failure (decode/media error) surfaced to the modal. */
  readonly onFailure: (failure: FilePreviewFailure) => void;
};

/** Task 3 props: read-only text and Markdown. */
export type FilePreviewTextProps = FilePreviewRendererProps & {
  readonly target: FilePreviewTarget | null;
  /** Markdown only: rendered document vs read-only source. Text always renders source. */
  readonly sourceMode: boolean;
  readonly onSourceModeChange: (sourceMode: boolean) => void;
  /** Present for `kind === "markdown"` only. */
  readonly markdown: FilePreviewMarkdownCapability | null;
};

/** Task 4 props: bounded image viewport. */
export type FilePreviewImageProps = FilePreviewRendererProps;

/** Task 5 props: native video element lifecycle. */
export type FilePreviewVideoProps = FilePreviewRendererProps;
