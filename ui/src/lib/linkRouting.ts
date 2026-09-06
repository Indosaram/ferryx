import { invoke, isTauri } from "@tauri-apps/api/core";
import { isHttpUrl, loadBrowserSettings } from "./browserSettings";
import { openExternalUrl } from "./browserTauri";

export const TERMINAL_LINK_ACTION_EVENT = "ferryx:terminal-link-actions";

export type LinkRoutingSource = "app" | "terminal" | "browser-popup" | "markdown" | "editor";
export type LinkDestination = "builtin" | "external";

export type TerminalToken =
  | { readonly type: "url"; readonly target: string }
  | {
      readonly type: "file";
      readonly path: string;
      readonly line?: number;
      readonly col?: number;
      readonly raw: string;
    };

export type LinkRoutingOptions = {
  shiftKey?: boolean;
  source?: LinkRoutingSource;
  destination?: LinkDestination;
};

export type TerminalLinkActionRequest = {
  url: string;
};

type BuiltInBrowserOpener = (url: string) => void | Promise<void>;
let builtInBrowserOpener: BuiltInBrowserOpener | null = null;

export function registerBuiltInBrowserLinkOpener(opener: BuiltInBrowserOpener): () => void {
  builtInBrowserOpener = opener;
  return () => {
    if (builtInBrowserOpener === opener) builtInBrowserOpener = null;
  };
}

export async function routeHttpLink(url: string, options: LinkRoutingOptions = {}): Promise<LinkDestination> {
  const normalized = url.trim();
  if (!isHttpUrl(normalized)) throw new Error("Only http(s) links can be opened by the browser router.");

  const settings = loadBrowserSettings();
  const destination = options.destination
    ?? (options.shiftKey && settings.shiftOpensSystemBrowser
      ? "external"
      : settings.openLinksInBuiltInBrowser && builtInBrowserOpener
        ? "builtin"
        : "external");

  if (destination === "builtin") {
    if (!builtInBrowserOpener) {
      await openExternalUrl(normalized);
      return "external";
    }
    await builtInBrowserOpener(normalized);
    return "builtin";
  }

  await openExternalUrl(normalized);
  return "external";
}

export async function requestTerminalLinkOpen(url: string, shiftKey = false): Promise<"chooser" | LinkDestination> {
  const settings = loadBrowserSettings();
  if (shiftKey && settings.shiftOpensSystemBrowser) {
    return routeHttpLink(url, { shiftKey: true, source: "terminal" });
  }
  if (!settings.showTerminalLinkActions) {
    return routeHttpLink(url, { source: "terminal" });
  }
  if (typeof window === "undefined") {
    return routeHttpLink(url, { source: "terminal" });
  }
  window.dispatchEvent(new CustomEvent<TerminalLinkActionRequest>(TERMINAL_LINK_ACTION_EVENT, {
    detail: { url },
  }));
  return "chooser";
}

const KNOWN_EXTENSIONS = new Set([
  "ts", "tsx", "js", "jsx", "mjs", "cjs", "rs", "py", "go", "json", "json5",
  "toml", "yaml", "yml", "md", "mdx", "css", "scss", "html", "sh", "bash",
  "zsh", "lock", "txt", "c", "cpp", "cc", "h", "hpp", "svg", "png", "jpg",
  "jpeg", "gif", "env", "conf", "config", "log", "sql", "diff", "patch",
]);

function isEastAsianWide(code: number): boolean {
  return (
    (code >= 0x1100 && code <= 0x115f) || // Hangul Jamo
    (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) || // CJK Radicals, Symbols, Ideographs
    (code >= 0xac00 && code <= 0xd7a3) || // Hangul Syllables
    (code >= 0xf900 && code <= 0xfaff) || // CJK Compatibility Ideographs
    (code >= 0xfe10 && code <= 0xfe19) || // Vertical forms
    (code >= 0xfe30 && code <= 0xfe6f) || // CJK Compatibility Forms
    (code >= 0xff00 && code <= 0xff60) || // Fullwidth Forms
    (code >= 0xffe0 && code <= 0xffe6)
  );
}

/**
 * Maps a visual terminal grid column to a UTF-16 character index in the line,
 * accounting for 2-column East Asian wide characters.
 */
export function gridColToCharIndex(line: string, gridCol: number): number {
  let currentCol = 0;
  for (let i = 0; i < line.length; i++) {
    if (currentCol >= gridCol) {
      return i;
    }
    const code = line.charCodeAt(i);
    currentCol += isEastAsianWide(code) ? 2 : 1;
  }
  return line.length;
}

function isIpAddress(str: string): boolean {
  return /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(?::\d+)?$/.test(str);
}

function isNumericOrVersion(str: string): boolean {
  return /^[vV]?\d+(?:\.\d+)+(?::\d+)?$/.test(str);
}

function parseFilePathCandidate(raw: string): TerminalToken | null {
  const trimmed = raw.trim();
  if (isIpAddress(trimmed) || isNumericOrVersion(trimmed)) {
    return null;
  }

  // Parse :line:col or :line suffix
  const match = trimmed.match(/^(.*?):(\d+)(?::(\d+))?$/);
  const pathPart = match ? match[1] : trimmed;
  const line = match ? parseInt(match[2], 10) : undefined;
  const col = match && match[3] ? parseInt(match[3], 10) : undefined;

  // Single file name without slash must have a known file extension or filename
  if (!pathPart.includes("/") && !pathPart.includes("\\")) {
    const extMatch = pathPart.match(/\.([a-zA-Z0-9_-]+)$/);
    if (!extMatch || !KNOWN_EXTENSIONS.has(extMatch[1].toLowerCase())) {
      if (
        pathPart !== "Dockerfile" &&
        pathPart !== "Makefile" &&
        pathPart !== "Cargo.lock"
      ) {
        return null;
      }
    }
  }

  return {
    type: "file",
    path: pathPart,
    line: Number.isFinite(line) ? line : undefined,
    col: col !== undefined && Number.isFinite(col) ? col : undefined,
    raw: trimmed,
  };
}

/**
 * Resolves a URL or file path token from a terminal line given the visual grid column index.
 * Matches:
 * - Web URLs: https://example.com/foo (supports balanced parentheses)
 * - Quoted paths: 'my folder/file.ts:10', "path with spaces/file.rs"
 * - Parenthesized paths: (/Users/user/index.js:15:3)
 * - Absolute paths: /Users/name/repo/file.ts:42:10
 * - Relative paths: src/components/App.tsx:15, ./ui/main.tsx, path/o'brien/file.txt
 * - Single files with known extension: Cargo.toml:5, package.json
 */
export function resolveTokenAtCol(line: string, col: number): TerminalToken | null {
  if (!line || col < 0) return null;

  const targetCharIndex = gridColToCharIndex(line, col);

  // 1. Check URLs first (with balanced paren handling)
  const URL_PATTERN = /\bhttps?:\/\/[^\s<>'"]+/gi;
  let urlMatch: RegExpExecArray | null;
  while ((urlMatch = URL_PATTERN.exec(line)) !== null) {
    let url = urlMatch[0];
    while (
      /[.,;:!?]$/.test(url) ||
      (url.endsWith(")") &&
        (url.match(/\(/g) || []).length < (url.match(/\)/g) || []).length)
    ) {
      url = url.slice(0, -1);
    }
    const start = urlMatch.index;
    const end = start + url.length;
    if (targetCharIndex >= start && targetCharIndex <= end) {
      return { type: "url", target: url };
    }
  }

  // 2. Check quoted paths: '...', "...", `...` (supporting spaces and quotes inside or outside :line)
  const QUOTED_PATTERN = /['"`]([^\r\n'"`]+)['"`](?::\d+(?::\d+)?)?/g;
  let quotedMatch: RegExpExecArray | null;
  while ((quotedMatch = QUOTED_PATTERN.exec(line)) !== null) {
    const full = quotedMatch[0];
    const content = quotedMatch[1];
    const start = quotedMatch.index;
    const end = start + full.length;
    if (targetCharIndex >= start && targetCharIndex <= end) {
      const quoteChar = full[0];
      const lastQuote = full.lastIndexOf(quoteChar);
      const suffix = full.slice(lastQuote + 1);
      const candidate = parseFilePathCandidate(content + suffix);
      if (candidate) return candidate;
    }
  }

  // 3. Check parenthesized paths: (path:line:col)
  const PAREN_PATTERN = /\(([^()\r\n]+)\)/g;
  let parenMatch: RegExpExecArray | null;
  while ((parenMatch = PAREN_PATTERN.exec(line)) !== null) {
    const content = parenMatch[1].trim();
    const start = parenMatch.index + 1;
    const end = start + content.length;
    if (targetCharIndex >= start && targetCharIndex <= end) {
      const candidate = parseFilePathCandidate(content);
      if (candidate) return candidate;
    }
  }

  // 4. Check unquoted paths with slashes (supports apostrophes and escaped spaces)
  const PATH_PATTERN =
    /(?:[a-zA-Z]:[\\/]|~[\\/]|\.\.?[\\/]|\/|[a-zA-Z0-9_-]+[\\/])(?:[\w.'-]|\\ |[\\/])+[\w.-]+(?::\d+(?::\d+)?)?/g;
  let pathMatch: RegExpExecArray | null;
  while ((pathMatch = PATH_PATTERN.exec(line)) !== null) {
    const full = pathMatch[0];
    const start = pathMatch.index;
    const end = start + full.length;
    if (targetCharIndex >= start && targetCharIndex <= end) {
      const unescaped = full.replace(/\\ /g, " ");
      const candidate = parseFilePathCandidate(unescaped);
      if (candidate) return candidate;
    }
  }

  // 5. Fallback: single file names with known extensions (e.g. package.json:12)
  const SINGLE_FILE_PATTERN = /\b[\w.'-]+\.[a-zA-Z0-9_-]+(?::\d+(?::\d+)?)?\b/g;
  let fileMatch: RegExpExecArray | null;
  while ((fileMatch = SINGLE_FILE_PATTERN.exec(line)) !== null) {
    const full = fileMatch[0];
    const start = fileMatch.index;
    const end = start + full.length;
    if (targetCharIndex >= start && targetCharIndex <= end) {
      const candidate = parseFilePathCandidate(full);
      if (candidate) return candidate;
    }
  }

  return null;
}

export type OpenTerminalTokenOptions = {
  shiftKey?: boolean;
  cwd?: string;
};

/**
 * Opens a terminal token:
 * - URLs route through requestTerminalLinkOpen (or external on Shift)
 * - File paths open via Tauri cmd_open_file_path with line/column
 */
export async function openTerminalToken(
  token: TerminalToken,
  options: OpenTerminalTokenOptions = {},
): Promise<boolean> {
  if (token.type === "url") {
    await requestTerminalLinkOpen(token.target, options.shiftKey);
    return true;
  }

  if (token.type === "file") {
    if (!isTauri()) {
      return false;
    }
    try {
      return await invoke<boolean>("cmd_open_file_path", {
        path: token.path,
        cwd: options.cwd,
        line: token.line,
        col: token.col,
      });
    } catch (error) {
      console.error("Failed to open file path from terminal:", error);
      return false;
    }
  }

  return false;
}
