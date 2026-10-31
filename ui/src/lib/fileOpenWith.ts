import { invoke } from "@tauri-apps/api/core";

export type OpenWithApp = {
  id: string;
  name: string;
};

export type OpenWithApps = {
  apps: OpenWithApp[];
  supportsChooser: boolean;
};

export async function listOpenWithApps(path: string): Promise<OpenWithApps> {
  return invoke<OpenWithApps>("cmd_file_open_with_apps", { path });
}

export async function openWithApp(path: string, appId: string): Promise<void> {
  return invoke<void>("cmd_file_open_with", { path, appId });
}

export async function printCurrentWebview(root: HTMLElement): Promise<void> {
  const document = root.ownerDocument;
  const view = document.defaultView;
  if (!view || document.querySelector("[data-file-preview-print-document]")) return;
  view.dispatchEvent(new Event("beforeprint"));
  const printable = root.cloneNode(true) as HTMLElement;
  printable.setAttribute("data-file-preview-print-document", "");
  printable.setAttribute("aria-hidden", "true");
  document.body.append(printable);
  const media = view.matchMedia?.("print");
  let enteredMedia = media?.matches ?? false;
  let finished = false;
  const cleanup = () => {
    finished = true;
    printable.remove();
    view.removeEventListener("afterprint", cleanup);
    media?.removeEventListener("change", mediaChanged);
  };
  const finish = () => {
    if (finished) return;
    cleanup();
    view.dispatchEvent(new Event("afterprint"));
  };
  const mediaChanged = (event: MediaQueryListEvent) => {
    if (event.matches) enteredMedia = true;
    else if (enteredMedia) finish();
  };
  media?.addEventListener("change", mediaChanged);
  view.addEventListener("afterprint", cleanup, { once: true });
  try {
    const completed = await invoke<boolean>("cmd_webview_print");
    if (completed) finish();
  } catch (error) {
    finish();
    throw error;
  }
}
