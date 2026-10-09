import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * Copies text to the system clipboard.
 *
 * On desktop the WebView clipboard APIs refuse writes whenever a native terminal view holds
 * keyboard focus, so the native writer runs first; the web APIs remain the path for the remote
 * web client and the fallback when the native writer is unavailable.
 */
export async function copyTextToClipboard(text: string): Promise<boolean> {
  if (isTauri()) {
    try {
      await invoke("cmd_clipboard_write_text", { text });
      return true;
    } catch (error) {
      console.warn("[clipboard] native clipboard write failed; falling back to WebView", error);
    }
  }

  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (error) {
    console.warn("[clipboard] navigator.clipboard.writeText failed", error);
  }

  try {
    if (typeof document === "undefined" || !document.body) {
      return false;
    }
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    try {
      ta.select();
      return document.execCommand("copy");
    } finally {
      ta.remove();
    }
  } catch (error) {
    console.warn("[clipboard] execCommand copy failed", error);
    return false;
  }
}
