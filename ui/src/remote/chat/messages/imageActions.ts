/**
 * Save and copy an image the transcript draws (t3code activity-log.md "Open an image").
 *
 * The lightbox asks the owner for these two actions and falls back to a download anchor when no
 * owner supplies them. Both surfaces need the same behaviour, so it lives here once: a save is a
 * download of exactly the bytes the reader is looking at, and a copy puts those bytes on the
 * clipboard as a PNG, which is what a chat client can paste back.
 *
 * Nothing here throws: a refused save or an unavailable clipboard is reported as `false` so the
 * caller can keep quiet instead of breaking the lightbox.
 */

/** A filename an image can be saved under: the caller's name, else the last path segment. */
export function imageFileName(src: string, name?: string): string {
  const trimmed = name?.trim();
  if (trimmed !== undefined && trimmed.length > 0) {
    return /\.[A-Za-z0-9]{1,8}$/.test(trimmed) ? trimmed : `${trimmed}.png`;
  }
  // A `blob:` or `data:` source names no file: its last segment is an opaque handle (or the whole
  // payload), which is not a name to save under. Only an `http(s):` path ends in one.
  if (/^(blob|data):/i.test(src)) return "image.png";
  const tail = src.split(/[?#]/, 1)[0]?.split("/").pop() ?? "";
  return tail.length > 0 ? tail : "image.png";
}

/**
 * Save the image bytes: an anchor download of `src`, named as the lightbox asked. A `blob:` or
 * `data:` source downloads the bytes the renderer already holds; an `http(s):` source downloads
 * what the browser fetches, which is the same URL the `<img>` drew.
 */
export function saveImage(src: string, name?: string): boolean {
  if (typeof document === "undefined") return false;
  try {
    const anchor = document.createElement("a");
    anchor.href = src;
    anchor.download = imageFileName(src, name);
    anchor.rel = "noopener";
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    return true;
  } catch {
    return false;
  }
}

/**
 * Copy the image to the clipboard as a PNG.
 *
 * The clipboard takes a `ClipboardItem` of encoded bytes, never a URL, so the bytes are fetched
 * first. A source that will not fetch (an expired host URL, a cross-origin refusal) is `false`,
 * which is the same answer as a browser without the async clipboard API.
 */
export async function copyImage(src: string): Promise<boolean> {
  const clipboard = typeof navigator === "undefined" ? undefined : navigator.clipboard;
  if (clipboard === undefined || typeof clipboard.write !== "function") return false;
  try {
    const response = await fetch(src);
    if (!response.ok) return false;
    const blob = await response.blob();
    // A browser only accepts image/png on the clipboard; re-encode anything else first.
    const png = blob.type === "image/png" ? blob : await toPngBlob(blob);
    if (png === null) return false;
    await clipboard.write([new ClipboardItem({ "image/png": png })]);
    return true;
  } catch {
    return false;
  }
}

/** Re-encode an image blob as PNG through a canvas; `null` when the browser cannot decode it. */
async function toPngBlob(blob: Blob): Promise<Blob | null> {
  if (typeof createImageBitmap !== "function" || typeof document === "undefined") return null;
  try {
    const bitmap = await createImageBitmap(blob);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const context = canvas.getContext("2d");
    if (context === null) return null;
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    return await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((encoded) => resolve(encoded), "image/png");
    });
  } catch {
    return null;
  }
}

/**
 * The pair a chat owner hands the workspace: `onSaveImage`/`onCopyImage` in the lightbox's own
 * signatures, so a pane wires them with one spread and no adapter.
 */
export interface ImageActionHandlers {
  readonly onSaveImage: (src: string, name?: string) => void;
  readonly onCopyImage: (src: string) => void;
}

export function imageActionHandlers(): ImageActionHandlers {
  return {
    onSaveImage: (src, name) => void saveImage(src, name),
    onCopyImage: (src) => void copyImage(src),
  };
}
