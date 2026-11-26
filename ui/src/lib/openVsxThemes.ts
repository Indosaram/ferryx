import type { ThemeDocumentV1 } from "./themeDocument";
import { parseJsonc, parseVsCodeThemeFile } from "./vscodeThemeImport";

const OPEN_VSX_SEARCH_URL = "https://open-vsx.org/api/-/search";
const MAX_VSIX_BYTES = 20 * 1024 * 1024;
const MAX_THEME_BYTES = 256 * 1024;
const MAX_THEMES_PER_EXTENSION = 24;
const SEARCH_TIMEOUT_MS = 10_000;

export type OpenVsxThemeSort = "downloadCount" | "rating" | "timestamp" | "relevance";

export type OpenVsxThemeExtension = {
  id: string;
  name: string;
  publisher: string;
  description: string;
  downloadCount: number;
  iconUrl: string | null;
  vsixUrl: string;
  version: string;
};

type ZipEntry = {
  name: string;
  compressionMethod: number;
  compressedSize: number;
  uncompressedSize: number;
  localHeaderOffset: number;
};

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export async function searchOpenVsxThemes(
  query: string,
  options?: { signal?: AbortSignal; sortBy?: OpenVsxThemeSort; size?: number },
): Promise<OpenVsxThemeExtension[]> {
  const params = new URLSearchParams({
    category: "Themes",
    size: String(options?.size ?? 12),
    sortBy: options?.sortBy ?? "downloadCount",
    sortOrder: "desc",
  });
  if (query.trim()) params.set("query", query.trim());

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), SEARCH_TIMEOUT_MS);
  const onAbort = () => controller.abort();
  options?.signal?.addEventListener("abort", onAbort, { once: true });

  try {
    const response = await fetch(`${OPEN_VSX_SEARCH_URL}?${params.toString()}`, {
      signal: controller.signal,
      headers: { Accept: "application/json" },
    });
    if (!response.ok) throw new Error(`OpenVSX search failed (${response.status}).`);
    const payload: unknown = await response.json();
    if (!isRecord(payload) || !Array.isArray(payload.extensions)) return [];
    const results: OpenVsxThemeExtension[] = [];
    for (const item of payload.extensions) {
      if (!isRecord(item)) continue;
      const namespace = typeof item.namespace === "string" ? item.namespace : "";
      const extName = typeof item.name === "string" ? item.name : "";
      const files = isRecord(item.files) ? item.files : {};
      const vsixUrl = typeof files.download === "string" ? files.download : "";
      if (!namespace || !extName || !vsixUrl.startsWith("https://")) continue;
      results.push({
        id: `${namespace}.${extName}`,
        name: typeof item.displayName === "string" && item.displayName.trim() ? item.displayName.trim() : extName,
        publisher: namespace,
        description: typeof item.description === "string" ? item.description : "",
        downloadCount: typeof item.downloadCount === "number" ? item.downloadCount : 0,
        iconUrl: typeof files.icon === "string" && files.icon.startsWith("https://") ? files.icon : null,
        vsixUrl,
        version: typeof item.version === "string" ? item.version : "",
      });
    }
    return results;
  } finally {
    clearTimeout(timer);
    options?.signal?.removeEventListener("abort", onAbort);
  }
}

function parseZipCentralDirectory(bytes: Uint8Array): Map<string, ZipEntry> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let eocdOffset = -1;
  const minOffset = Math.max(0, bytes.length - 65557);
  for (let i = bytes.length - 22; i >= minOffset; i -= 1) {
    if (view.getUint32(i, true) === 0x06054b50) {
      eocdOffset = i;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error("Invalid VSIX archive (missing EOCD).");
  const entryCount = view.getUint16(eocdOffset + 10, true);
  const cdOffset = view.getUint32(eocdOffset + 16, true);
  const decoder = new TextDecoder();
  const entries = new Map<string, ZipEntry>();
  let offset = cdOffset;
  for (let i = 0; i < entryCount && offset + 46 <= bytes.length; i += 1) {
    if (view.getUint32(offset, true) !== 0x02014b50) break;
    const compressionMethod = view.getUint16(offset + 10, true);
    const compressedSize = view.getUint32(offset + 20, true);
    const uncompressedSize = view.getUint32(offset + 24, true);
    const nameLen = view.getUint16(offset + 28, true);
    const extraLen = view.getUint16(offset + 30, true);
    const commentLen = view.getUint16(offset + 32, true);
    const localHeaderOffset = view.getUint32(offset + 42, true);
    const name = decoder.decode(bytes.subarray(offset + 46, offset + 46 + nameLen));
    entries.set(name, { name, compressionMethod, compressedSize, uncompressedSize, localHeaderOffset });
    offset += 46 + nameLen + extraLen + commentLen;
  }
  return entries;
}

async function readZipEntryText(bytes: Uint8Array, entry: ZipEntry): Promise<string> {
  if (entry.uncompressedSize > MAX_THEME_BYTES) {
    throw new Error(`Archive entry ${entry.name} exceeds size cap.`);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const off = entry.localHeaderOffset;
  if (off + 30 > bytes.length || view.getUint32(off, true) !== 0x04034b50) {
    throw new Error(`Invalid local header for ${entry.name}.`);
  }
  const nameLen = view.getUint16(off + 26, true);
  const extraLen = view.getUint16(off + 28, true);
  const dataStart = off + 30 + nameLen + extraLen;
  const raw = bytes.subarray(dataStart, dataStart + entry.compressedSize);
  if (entry.compressionMethod === 0) {
    return new TextDecoder().decode(raw);
  }
  if (entry.compressionMethod === 8 && typeof DecompressionStream !== "undefined") {
    const copy = new Uint8Array(raw.byteLength);
    copy.set(raw);
    const stream = new Blob([copy]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
    return await new Response(stream).text();
  }
  throw new Error(`Unsupported compression method ${entry.compressionMethod} in VSIX.`);
}

function normalizeZipPath(baseDir: string, relativePath: string): string {
  const joined = `${baseDir}/${relativePath.replace(/^\.\//, "")}`;
  const parts: string[] = [];
  for (const part of joined.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return parts.join("/");
}

export async function loadOpenVsxExtensionThemes(
  extension: Pick<OpenVsxThemeExtension, "id" | "name" | "vsixUrl">,
  signal?: AbortSignal,
): Promise<ThemeDocumentV1[]> {
  const response = await fetch(extension.vsixUrl, { signal });
  if (!response.ok) throw new Error(`Could not download ${extension.name} (${response.status}).`);
  const buffer = await response.arrayBuffer();
  if (buffer.byteLength > MAX_VSIX_BYTES) {
    throw new Error("Extension archive exceeds 20 MiB limit.");
  }
  const bytes = new Uint8Array(buffer);
  const entries = parseZipCentralDirectory(bytes);
  const pkgEntry = entries.get("extension/package.json");
  if (!pkgEntry) throw new Error("Extension package.json not found in VSIX.");
  const pkg: unknown = parseJsonc(await readZipEntryText(bytes, pkgEntry));
  const contributes = isRecord(pkg) && isRecord(pkg.contributes) ? pkg.contributes : null;
  const themesList = contributes && Array.isArray(contributes.themes) ? contributes.themes : [];

  const docs: ThemeDocumentV1[] = [];
  const seenIds = new Set<string>();
  for (const contrib of themesList.slice(0, MAX_THEMES_PER_EXTENSION)) {
    if (!isRecord(contrib) || typeof contrib.path !== "string") continue;
    const entryPath = normalizeZipPath("extension", contrib.path);
    const themeEntry = entries.get(entryPath);
    if (!themeEntry) continue;
    try {
      const themeRaw = parseJsonc(await readZipEntryText(bytes, themeEntry));
      if (!isRecord(themeRaw)) continue;
      let merged = themeRaw;
      if (typeof themeRaw.include === "string") {
        const dir = entryPath.slice(0, entryPath.lastIndexOf("/"));
        const includeEntry = entries.get(normalizeZipPath(dir, themeRaw.include));
        if (includeEntry) {
          const baseRaw = parseJsonc(await readZipEntryText(bytes, includeEntry));
          if (isRecord(baseRaw)) {
            merged = {
              ...baseRaw,
              ...themeRaw,
              colors: {
                ...(isRecord(baseRaw.colors) ? baseRaw.colors : {}),
                ...(isRecord(themeRaw.colors) ? themeRaw.colors : {}),
              },
            };
          }
        }
      }
      const label = typeof contrib.label === "string" && contrib.label.trim() ? contrib.label.trim() : extension.name;
      const parsed = parseVsCodeThemeFile({ ...merged, displayName: label }, label);
      if (parsed.ok) {
        let id = parsed.doc.id;
        let suffix = 2;
        while (seenIds.has(id)) {
          id = `${parsed.doc.id.slice(0, 58)}-${suffix++}`;
        }
        seenIds.add(id);
        docs.push({
          ...parsed.doc,
          id,
          collection: { id: extension.id, label: extension.name },
        });
      }
    } catch {
      continue;
    }
  }
  return docs;
}
