import { parseT3ThemeFile, type ThemeColorRole } from "./themePalettes";
import type { ThemeDocumentV1 } from "./themeDocument";

/**
 * Client for the t3code community theme store (t3themes.com, sourced from
 * SunkenInTime/t3-themes). Themes are plain t3code v1 JSON, so the only work here is
 * discovery and download: `parseT3ThemeFile` already owns format compatibility.
 */
const STORE_DATES_URL = "https://t3themes.com/theme-dates.json";
const STORE_RAW_BASE = "https://raw.githubusercontent.com/SunkenInTime/t3-themes/main/themes";
const STORE_TREE_URL = "https://api.github.com/repos/SunkenInTime/t3-themes/git/trees/main?recursive=1";
const MAX_PARALLEL = 8;
const FETCH_TIMEOUT_MS = 10_000;

/** Roles that make a recognisable preview swatch, most distinctive first. */
const SWATCH_ROLES: ReadonlyArray<ThemeColorRole> = [
  "canvas",
  "surface",
  "accent",
  "messageAction",
  "text",
];

export type T3StoreErrorCode = "network" | "http" | "empty";

export type T3StoreError = {
  code: T3StoreErrorCode;
  message: string;
};

export type T3StoreTheme = {
  id: string;
  name: string;
  author: string | null;
  description: string | null;
  base: "light" | "dark";
  addedAt: number;
  swatches: string[];
  doc: ThemeDocumentV1;
};

export type T3StoreCatalogResult =
  | { ok: true; themes: T3StoreTheme[]; skipped: number }
  | { ok: false; error: T3StoreError };

type FetchResponse = {
  ok: boolean;
  status: number;
  json: () => Promise<unknown>;
};

export type T3StoreFetch = (url: string, init?: { signal?: AbortSignal }) => Promise<FetchResponse>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function httpError(status: number): T3StoreError {
  return { code: "http", message: `Theme store responded with HTTP ${status}.` };
}

function networkError(cause: unknown): T3StoreError {
  const detail = cause instanceof Error ? cause.message : String(cause);
  return { code: "network", message: `Could not reach the t3code theme store: ${detail}` };
}

async function getJson(fetchImpl: T3StoreFetch, url: string): Promise<unknown> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetchImpl(url, { signal: controller.signal });
    if (!response.ok) throw httpError(response.status);
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

/** `"<id>.json"` keys with numeric timestamps, in whatever order the index lists them. */
function readDates(raw: unknown): Array<{ id: string; addedAt: number }> | null {
  if (!isRecord(raw)) return null;
  const rows: Array<{ id: string; addedAt: number }> = [];
  for (const [key, value] of Object.entries(raw)) {
    if (!key.endsWith(".json") || typeof value !== "number" || !Number.isFinite(value)) continue;
    rows.push({ id: key.slice(0, -".json".length), addedAt: value });
  }
  return rows;
}

function readTree(raw: unknown): Array<{ id: string; addedAt: number }> | null {
  if (!isRecord(raw) || !Array.isArray(raw.tree)) return null;
  const rows: Array<{ id: string; addedAt: number }> = [];
  for (const entry of raw.tree) {
    if (!isRecord(entry) || typeof entry.path !== "string") continue;
    const match = /^themes\/(.+)\.json$/.exec(entry.path);
    if (match) rows.push({ id: match[1], addedAt: 0 });
  }
  return rows;
}

function swatchesFor(raw: unknown): string[] {
  const colors = isRecord(raw) && isRecord(raw.colors) ? raw.colors : {};
  const swatches: string[] = [];
  for (const role of SWATCH_ROLES) {
    const value = colors[role];
    if (typeof value === "string" && value.trim()) swatches.push(value.trim());
  }
  return swatches;
}

function textField(raw: unknown, key: string): string | null {
  if (!isRecord(raw)) return null;
  const value = raw[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** Maps `tasks` with at most `limit` in flight, preserving input order. */
async function mapLimit<T, R>(items: T[], limit: number, run: (item: T) => Promise<R>): Promise<R[]> {
  const results = new Array<R>(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.min(limit, items.length) }, async () => {
    for (;;) {
      const index = next++;
      if (index >= items.length) return;
      results[index] = await run(items[index]);
    }
  });
  await Promise.all(workers);
  return results;
}

async function loadOne(fetchImpl: T3StoreFetch, entry: { id: string; addedAt: number }): Promise<T3StoreTheme | null> {
  let raw: unknown;
  try {
    raw = await getJson(fetchImpl, `${STORE_RAW_BASE}/${entry.id}.json`);
  } catch {
    return null;
  }
  const parsed = parseT3ThemeFile(raw);
  if (!parsed.ok) return null;
  return {
    id: entry.id,
    name: textField(raw, "name") ?? parsed.doc.name,
    author: textField(raw, "author"),
    description: textField(raw, "description"),
    base: parsed.doc.base,
    addedAt: entry.addedAt,
    swatches: swatchesFor(raw),
    doc: parsed.doc,
  };
}

let catalogCache: { themes: T3StoreTheme[]; skipped: number } | null = null;
let catalogInflight: Promise<T3StoreCatalogResult> | null = null;

/** Test hook: the catalog is cached for the process lifetime. */
export function resetT3StoreCache() {
  catalogCache = null;
  catalogInflight = null;
}

export async function loadT3StoreCatalog(options?: { fetchImpl?: T3StoreFetch }): Promise<T3StoreCatalogResult> {
  if (catalogCache) return { ok: true, ...catalogCache };
  if (catalogInflight) return catalogInflight;
  catalogInflight = (async (): Promise<T3StoreCatalogResult> => {
    const fetchImpl = options?.fetchImpl ?? (fetch as unknown as T3StoreFetch);
    let entries: Array<{ id: string; addedAt: number }> | null = null;
    let datesError: T3StoreError | null = null;
    try {
      entries = readDates(await getJson(fetchImpl, STORE_DATES_URL));
    } catch (error) {
      datesError = error && typeof error === "object" && "code" in error ? (error as T3StoreError) : networkError(error);
    }
    if (!entries || entries.length === 0) {
      // The dated index is a site convenience; the repository tree is the durable source.
      try {
        entries = readTree(await getJson(fetchImpl, STORE_TREE_URL));
      } catch {
        entries = null;
      }
    }
    if (!entries || entries.length === 0) {
      return { ok: false, error: datesError ?? { code: "empty", message: "The t3code theme store listed no themes." } };
    }

    const seen = new Set<string>();
    const unique = entries.filter((entry) => {
      if (seen.has(entry.id)) return false;
      seen.add(entry.id);
      return true;
    });
    const loaded = await mapLimit(unique, MAX_PARALLEL, (entry) => loadOne(fetchImpl, entry));
    const themes = loaded.filter((theme): theme is T3StoreTheme => theme !== null);
    const skipped = unique.length - themes.length;
    if (themes.length === 0) {
      return { ok: false, error: { code: "empty", message: "No t3code store theme could be imported." } };
    }
    themes.sort((a, b) => b.addedAt - a.addedAt || a.name.localeCompare(b.name));
    catalogCache = { themes, skipped };
    return { ok: true, themes, skipped };
  })();
  try {
    return await catalogInflight;
  } finally {
    catalogInflight = null;
  }
}

export function filterT3StoreThemes(themes: ReadonlyArray<T3StoreTheme>, query: string): T3StoreTheme[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...themes];
  return themes.filter((theme) =>
    [theme.name, theme.author ?? "", theme.description ?? "", theme.id]
      .some((field) => field.toLowerCase().includes(needle)),
  );
}
