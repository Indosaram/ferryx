import type { TerminalToken } from "./linkRouting";
import { charIndexToGridCol, gridColToCharIndex, resolveTokenAtCol } from "./linkRouting";
import { terminalWrapsInto, tuiWrapIndent, type WrappedRow } from "./hardWrappedPath";

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

/** Rows a link may span above or below the pointer row; matches the backend URL scan bound. */
const MAX_LINK_ROWS = 8;

type LineReceipt = WrappedRow & { readonly col?: number };

/** One physical row's share of a joined link line, in grid columns. */
export interface LinkLineSegment {
  row: number;
  joinedCol: number;
  rowCol: number;
  width: number;
}

export interface LinkLine {
  /** Text of the logical line around the pointer, rows joined. */
  text: string;
  /** Pointer grid column within `text`; -1 when the pointer is on a dropped margin. */
  col: number;
  segments: LinkLineSegment[];
  /**
   * Set when `text` includes rows joined only by the TUI wrap heuristic: the same line
   * joined across terminal soft wraps alone, to fall back to when the guess is refuted.
   */
  exact?: LinkLine;
}

type JoinKind = "terminal" | "tui";
interface JoinedRow {
  row: number;
  text: string;
  /** How this row joins the row above it; null for the first row. */
  join: JoinKind | null;
  indent: number;
}

function joinKind(from: WrappedRow, to: WrappedRow, cols: number): { kind: JoinKind; indent: number } | null {
  if (terminalWrapsInto(from, to, cols)) return { kind: "terminal", indent: 0 };
  const indent = tuiWrapIndent(from, to, cols);
  return indent === null ? null : { kind: "tui", indent };
}

function buildLinkLine(rows: JoinedRow[], pointerIndex: number, pointerCol: number): LinkLine {
  let text = "";
  let col = -1;
  const segments: LinkLineSegment[] = [];
  rows.forEach((row, index) => {
    const start = row.join === "tui" ? row.indent : 0;
    const end = rows[index + 1]?.join === "tui" ? row.text.trimEnd().length : row.text.length;
    const slice = row.text.slice(start, end);
    const joinedCol = charIndexToGridCol(text, text.length);
    if (index === pointerIndex) {
      const charIndex = gridColToCharIndex(row.text, pointerCol);
      const lastRow = index === rows.length - 1;
      if (charIndex >= start && (charIndex < end || lastRow)) {
        col = joinedCol + charIndexToGridCol(slice, charIndex - start)
          + Math.max(0, pointerCol - charIndexToGridCol(row.text, row.text.length));
      }
    }
    segments.push({
      row: row.row,
      joinedCol,
      rowCol: charIndexToGridCol(row.text, start),
      width: charIndexToGridCol(slice, slice.length),
    });
    text += slice;
  });
  return { text, col, segments };
}

function toLinkLine(rows: JoinedRow[], pointerIndex: number, pointerCol: number): LinkLine {
  const line = buildLinkLine(rows, pointerIndex, pointerCol);
  if (!rows.some((row) => row.join === "tui")) return line;
  let first = pointerIndex;
  while (first > 0 && rows[first].join === "terminal") first--;
  let last = pointerIndex;
  while (last + 1 < rows.length && rows[last + 1].join === "terminal") last++;
  const exactRows = rows.slice(first, last + 1).map((row, index) => (index === 0 ? { ...row, join: null } : row));
  return { ...line, exact: buildLinkLine(exactRows, pointerIndex - first, pointerCol) };
}

/**
 * Read the logical line under the pointer: rows the terminal soft-wrapped are joined exactly,
 * and rows a TUI wrapped with its own newlines are joined as a guess (see `LinkLine.exact`).
 */
export async function readLinkLine(
  invokeFn: TerminalInvokeFn,
  sessionId: string,
  col: number,
  row: number,
  rows?: number,
  cols?: number,
): Promise<LinkLine | null> {
  const readRow = (target: number) =>
    invokeFn<LineReceipt & { row: number }>("cmd_native_terminal_line_at", { sessionId, col: target === row ? col : 0, row: target });

  let current: LineReceipt;
  try {
    current = await readRow(row);
  } catch {
    return null;
  }
  if (!current || typeof current.text !== "string") return null;
  const pointerCol = typeof current.col === "number" ? current.col : col;
  const pointerRow: JoinedRow = { row, text: current.text, join: null, indent: 0 };

  if (cols === undefined || cols <= 0) {
    return buildLinkLine([pointerRow], 0, pointerCol);
  }

  const readNeighbour = (target: number) =>
    target >= 0 && (rows === undefined || target < rows)
      ? readRow(target).then((receipt) => (receipt && typeof receipt.text === "string" ? receipt : null), () => null)
      : Promise.resolve(null);

  const joined: JoinedRow[] = [pointerRow];
  let below: LineReceipt = current;
  for (let offset = 1; offset <= MAX_LINK_ROWS; offset++) {
    const above = await readNeighbour(row - offset);
    const join = above && joinKind(above, below, cols);
    if (!above || !join) break;
    joined[0] = { ...joined[0], join: join.kind, indent: join.indent };
    joined.unshift({ row: row - offset, text: above.text, join: null, indent: 0 });
    below = above;
  }
  const pointerIndex = joined.length - 1;
  let previous: LineReceipt = current;
  for (let offset = 1; offset <= MAX_LINK_ROWS; offset++) {
    const next = await readNeighbour(row + offset);
    const join = next && joinKind(previous, next, cols);
    if (!next || !join) break;
    joined.push({ row: row + offset, text: next.text, join: join.kind, indent: join.indent });
    previous = next;
  }

  return toLinkLine(joined, pointerIndex, pointerCol);
}

function sameToken(a: TerminalToken | null, b: TerminalToken | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export interface LinkResolution {
  token: TerminalToken;
  /** The line the token was resolved from; its span maps back to rows through `segments`. */
  line: LinkLine;
}

/**
 * Resolve the link under the pointer. A token that only exists because of a TUI-wrap guess is
 * kept when `confirmFile` accepts its path (or for URLs, which have nothing to check);
 * otherwise the token from the exactly joined line wins.
 */
export async function resolveLinkLine(
  line: LinkLine,
  confirmFile?: (path: string) => Promise<boolean>,
): Promise<LinkResolution | null> {
  const token = resolveTokenAtCol(line.text, line.col);
  if (!line.exact) return token ? { token, line } : null;
  const exactToken = resolveTokenAtCol(line.exact.text, line.exact.col);
  const exact = exactToken ? { token: exactToken, line: line.exact } : null;
  if (!token || sameToken(token, exactToken)) return exact;
  if (token.type === "file" && confirmFile && !(await confirmFile(token.path))) return exact;
  return { token, line };
}

/** Grid cells `{ row, start, end }` (end exclusive) covered by the resolved token, per row. */
export function linkTokenSpans(resolution: LinkResolution): Array<{ row: number; start: number; end: number }> {
  const { line, token } = resolution;
  const key = JSON.stringify(token);
  const covers = (col: number) => JSON.stringify(resolveTokenAtCol(line.text, col)) === key;
  const lineWidth = charIndexToGridCol(line.text, line.text.length);
  let start = line.col;
  let end = line.col + 1;
  while (start > 0 && covers(start - 1)) start--;
  while (end < lineWidth && covers(end)) end++;
  return linkLineSpans(line, start, end);
}

/** Grid cells of the whitespace-delimited word under the pointer, for links the line cannot name. */
export function linkWordSpans(line: LinkLine): Array<{ row: number; start: number; end: number }> {
  const text = line.text;
  const pointer = gridColToCharIndex(text, line.col);
  let start = Math.min(pointer, Math.max(0, text.length - 1));
  while (start > 0 && !/\s/.test(text[start - 1])) start--;
  let end = Math.min(pointer + 1, text.length);
  while (end < text.length && !/\s/.test(text[end])) end++;
  const startCol = charIndexToGridCol(text, start);
  return linkLineSpans(line, startCol, Math.max(startCol + 1, charIndexToGridCol(text, end)));
}

function linkLineSpans(line: LinkLine, start: number, end: number): Array<{ row: number; start: number; end: number }> {
  return line.segments.flatMap((segment) => {
    const from = Math.max(start, segment.joinedCol);
    const to = Math.min(end, segment.joinedCol + segment.width);
    return from < to
      ? [{ row: segment.row, start: segment.rowCol + from - segment.joinedCol, end: segment.rowCol + to - segment.joinedCol }]
      : [];
  });
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
    freshRemoteSnapshot: true,
    remote: { workspaceId: opts.workspaceId, path: fetched.remotePath, sessionId: opts.sessionId ?? null },
  });
}
