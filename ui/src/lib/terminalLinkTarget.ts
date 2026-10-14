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

export async function openRemoteFileToken(
  token: TerminalFileToken,
  opts: OpenRemoteFileTokenOptions,
): Promise<boolean> {
  const paired = opts.workspaceId.startsWith("daemon:");
  const fetched = await opts.invokeFn<{
    localPath: string;
    remotePath: string;
    byteLength: number;
  }>(
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

  return opts.openLocal({
    ...token,
    path: fetched.localPath,
  });
}
