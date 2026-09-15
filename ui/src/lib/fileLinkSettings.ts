import { getMigratedItem } from "./storageKeys";

export type FileLinkEditor = "system" | "vscode" | "cursor" | "zed";
const KEY = "ferryx.terminal.fileLinkEditor";

export function parseFileLinkEditor(value: string | null): FileLinkEditor {
  switch (value) {
    case "vscode": case "cursor": case "zed": return value;
    default: return "system";
  }
}

export function loadFileLinkEditor(): FileLinkEditor {
  return parseFileLinkEditor(getMigratedItem(KEY));
}

export function saveFileLinkEditor(value: FileLinkEditor): void {
  window.localStorage.setItem(KEY, value);
}
