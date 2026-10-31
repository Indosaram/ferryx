import type { TerminalToken } from "./linkRouting";
import { stitchHardWrappedLine } from "./hardWrappedPath";

export type TerminalFileToken = Extract<TerminalToken, { type: "file" }>;

export type TerminalInvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export const TERMINAL_FILE_LINK_ACTION_EVENT = "ferryx:terminal-file-link-actions";

export interface TerminalLinkOpenOverrides {
  /** Overrides the configured file-link editor for this open. */
  editor?: "system" | "vscode" | "cursor" | "zed";
}

export interface TerminalFileLinkActionDetail {
  token: TerminalToken & { absolutePath?: string };
  open: (shiftKey: boolean, overrides?: TerminalLinkOpenOverrides) => Promise<void>;
  /** The path names a file on an SSH/paired host; local-disk actions do not apply. */
  remote?: boolean;
}

export interface TokenFromHyperlinkOptions {
  /**
   * The session runs on another host (ssh:/daemon: workspace): accept any file://
   * authority and keep the absolute pathname so it is fetched from that host.
   */
  remote?: boolean;
}

const sessionMouseTracking = new Map<string, boolean>();

export function setSessionMouseTracking(sessionId: string, enabled: boolean): void {
  sessionMouseTracking.set(sessionId, enabled);
}

export function isSessionMouseTrackingEnabled(sessionId?: string | null): boolean {
  if (!sessionId) return false;
  return sessionMouseTracking.get(sessionId) ?? false;
}

export function tokenFromHyperlink(
  uri: string,
  options: TokenFromHyperlinkOptions = {},
): TerminalToken | null {
  try {
    const url = new URL(uri);
    if (url.protocol === "http:" || url.protocol === "https:") {
      return { type: "url", target: uri };
    }
    if (url.protocol === "file:") {
      if (!options.remote && url.host !== "" && url.host !== "localhost") {
        return null;
      }
      let pathname: string;
      try {
        pathname = decodeURIComponent(url.pathname);
      } catch {
        return null;
      }
      if (/^\/[a-zA-Z]:/.test(pathname)) {
        pathname = pathname.slice(1);
      }
      return {
        type: "file",
        path: pathname,
        raw: uri,
      };
    }
    return null;
  } catch {
    return null;
  }
}

export async function readLinkLine(
  invokeFn: TerminalInvokeFn,
  sessionId: string,
  col: number,
  row: number,
  rows?: number,
  cols?: number,
): Promise<{ text: string; col: number } | null> {
  let current: { text: string; col: number; row: number };
  try {
    current = await invokeFn<{ text: string; col: number; row: number }>(
      "cmd_native_terminal_line_at",
      {
        sessionId,
        col,
        row,
      },
    );
  } catch {
    return null;
  }

  if (!current || typeof current.text !== "string") {
    return null;
  }

  if (cols === undefined || cols <= 0) {
    return { text: current.text, col: current.col };
  }

  const prevPromise =
    row > 0 && (rows === undefined || row - 1 < rows)
      ? invokeFn<{ text: string }>("cmd_native_terminal_line_at", {
          sessionId,
          col: 0,
          row: row - 1,
        }).catch(() => null)
      : Promise.resolve(null);

  const nextPromise =
    rows === undefined || row + 1 < rows
      ? invokeFn<{ text: string }>("cmd_native_terminal_line_at", {
          sessionId,
          col: 0,
          row: row + 1,
        }).catch(() => null)
      : Promise.resolve(null);

  const [prev, next] = await Promise.all([prevPromise, nextPromise]);

  return stitchHardWrappedLine(
    prev && typeof prev.text === "string" ? { text: prev.text } : null,
    { text: current.text },
    next && typeof next.text === "string" ? { text: next.text } : null,
    current.col,
    cols,
  );
}

export interface OpenRemoteFileTokenOptions {
  workspaceId: string;
  cwd?: string | null;
  /** Backend session id; forwarded to paired-host fetches only. */
  sessionId?: string | null;
  shiftKey: boolean;
  openLocal: (localToken: TerminalFileToken) => Promise<boolean>;
  invokeFn: TerminalInvokeFn;
}

/**
 * A remote (SSH or paired-host) pane cannot stat a printed path locally, so its hover
 * hint relies on shape alone. Prose such as "I/O", "n/a" or "TCP/IP" must not look
 * like a link there. Rooted paths (/, ~, ./, ../, C:\) always qualify; otherwise the
 * token needs a file extension, two separators, or a final segment that reads as a
 * name rather than an abbreviation.
 */
export function isLikelyRemoteFilePath(path: string): boolean {
  if (/^(?:[/\\~]|\.{1,2}[/\\]|[a-zA-Z]:[/\\])/.test(path)) return true;
  const segments = path.split(/[/\\]/).filter(Boolean);
  const last = segments[segments.length - 1] ?? "";
  if (/\.[a-zA-Z0-9_-]+$/.test(last) || last === "Makefile" || last === "Dockerfile") return true;
  if (segments.length >= 3) return true;
  if (segments.length < 2) return false;
  // Two bare segments: "src/lib", "bin/ls", "docs/API" are paths; "I/O", "A/B", "TCP/IP",
  // "24/7" (every segment upper-case or digits) and common slash words are prose.
  const lower = segments.map((segment) => segment.toLowerCase()).join("/");
  if (PROSE_SLASH_WORDS.has(lower)) return false;
  return !segments.every((segment) => /^[A-Z0-9]+$/.test(segment));
}

const PROSE_SLASH_WORDS = new Set(["n/a", "w/o", "and/or", "yes/no", "on/off", "he/she", "his/her", "s/he", "c/o"]);

type RemoteFetchResult = { localPath: string; remotePath: string; byteLength: number };

/** One fetch per (workspace, cwd, path) at a time: a fetch can move up to 8 MiB over SSH, so
 * repeated clicks while it runs share it instead of starting parallel downloads. */
const inFlightRemoteFetches = new Map<string, Promise<RemoteFetchResult>>();

/**
 * The base a remote file link resolves against: the session's live cwd only. The worktree
 * root is never substituted, because a relative token would then silently name a different
 * file; without a cwd the backend refuses with `remoteCwdUnknown` instead.
 */
export function remoteFetchCwd(session: { readonly cwd?: string | null } | null | undefined): string | null {
  return session?.cwd || null;
}

/** User copy for a failed remote fetch, keyed on the backend's machine reason. */
export function remoteFileFetchErrorMessage(error: unknown): string | null {
  const details = (error as { details?: { reason?: unknown; maxBytes?: unknown } | null } | null)?.details ?? {};
  switch (details.reason) {
    case "remoteFileMissing":
      return "The remote file does not exist or is not a regular file.";
    case "remoteFileTooLarge":
      return typeof details.maxBytes === "number"
        ? `Remote files larger than ${Math.round(details.maxBytes / (1024 * 1024))} MiB cannot be opened.`
        : "The remote file is too large to open.";
    case "invalidRemotePath":
      return "This link does not name a valid path on the remote host.";
    case "remoteCwdUnknown":
      return "The remote terminal's current directory is unknown, so this relative path cannot be opened. Use an absolute path.";
    case "remoteFileNotLaunchable":
      return "Files from a remote host only open in the preview or an editor, not with the system opener.";
    default:
      return null;
  }
}

export async function openRemoteFileToken(
  token: TerminalFileToken,
  opts: OpenRemoteFileTokenOptions,
): Promise<boolean> {
  const paired = opts.workspaceId.startsWith("daemon:");
  const key = [opts.workspaceId, opts.cwd ?? "", paired ? (opts.sessionId ?? "") : "", token.path].join("\0");
  let pending = inFlightRemoteFetches.get(key);
  if (!pending) {
    pending = opts.invokeFn<RemoteFetchResult>(
      paired ? "cmd_paired_file_fetch" : "cmd_remote_file_fetch",
      paired
        ? {
            workspaceId: opts.workspaceId,
            path: token.path,
            cwd: opts.cwd ?? null,
            sessionId: opts.sessionId ?? null,
          }
        : {
            workspaceId: opts.workspaceId,
            path: token.path,
            cwd: opts.cwd ?? null,
          },
    );
    inFlightRemoteFetches.set(key, pending);
    const settled = () => {
      if (inFlightRemoteFetches.get(key) === pending) inFlightRemoteFetches.delete(key);
    };
    pending.then(settled, settled);
  }
  const fetched = await pending;

  return opts.openLocal({
    ...token,
    path: fetched.localPath,
  });
}
