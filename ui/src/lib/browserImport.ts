import { invoke } from "@tauri-apps/api/core";

/** Data a browser import can bring over. */
export type BrowserImportType = "bookmarks" | "history" | "cookies" | "passwords";

export const BROWSER_IMPORT_TYPES: readonly BrowserImportType[] = [
  "bookmarks",
  "history",
  "cookies",
  "passwords",
];

/** A browser profile found on this machine (`cmd_browser_import_sources`). */
export type BrowserImportSource = {
  id: string;
  browser: string;
  name: string;
  types: BrowserImportType[];
  requiresFullDiskAccess: boolean;
  /** Passwords come from a CSV the user exports (Safari). */
  passwordsNeedFile: boolean;
};

export type BrowserImportTypeResult = {
  type: BrowserImportType;
  imported: number;
  skipped: number;
  notes: string[];
  error: { code: string; message: string } | null;
};

export type BrowserImportReport = { results: BrowserImportTypeResult[] };

export type PasswordFileFormat =
  | "onePasswordCsv"
  | "onePasswordPux"
  | "bitwardenCsv"
  | "bitwardenOrganizationCsv"
  | "bitwardenJson"
  | "applePasswordsCsv"
  | "keepassxcCsv"
  | "keepassCsv";

export const PASSWORD_FILE_FORMATS: ReadonlyArray<{
  format: PasswordFileFormat;
  label: string;
  extensions: string[];
}> = [
  { format: "onePasswordPux", label: "1Password (.1pux)", extensions: ["1pux"] },
  { format: "onePasswordCsv", label: "1Password CSV", extensions: ["csv"] },
  { format: "bitwardenJson", label: "Bitwarden JSON", extensions: ["json"] },
  { format: "bitwardenCsv", label: "Bitwarden CSV", extensions: ["csv"] },
  { format: "bitwardenOrganizationCsv", label: "Bitwarden organization CSV", extensions: ["csv"] },
  { format: "applePasswordsCsv", label: "Apple Passwords / Safari CSV", extensions: ["csv"] },
  { format: "keepassxcCsv", label: "KeePassXC CSV", extensions: ["csv"] },
  { format: "keepassCsv", label: "KeePass CSV", extensions: ["csv"] },
];

export function listBrowserImportSources(): Promise<BrowserImportSource[]> {
  return invoke<BrowserImportSource[]>("cmd_browser_import_sources");
}

export function runBrowserImport(request: {
  sourceId: string;
  profileId: string;
  types: BrowserImportType[];
  passwordCsvPath?: string;
}): Promise<BrowserImportReport> {
  return invoke<BrowserImportReport>("cmd_browser_import_run", { request });
}

export function cancelBrowserImport(): Promise<boolean> {
  return invoke<boolean>("cmd_browser_import_cancel");
}

export function importPasswordFile(request: {
  format: PasswordFileFormat;
  path: string;
  profileId: string;
}): Promise<BrowserImportTypeResult> {
  return invoke<BrowserImportTypeResult>("cmd_browser_import_password_file", { request });
}

const TYPE_LABELS: Record<BrowserImportType, string> = {
  bookmarks: "Bookmarks",
  history: "History",
  cookies: "Cookies",
  passwords: "Passwords",
};

export function browserImportTypeLabel(type: BrowserImportType): string {
  return TYPE_LABELS[type];
}

const TYPE_ERROR_MESSAGES: Record<string, string> = {
  "not-found": "Nothing to import was found in this profile.",
  "permission-denied": "Ferryx was not allowed to read this data. On macOS, grant Full Disk Access in System Settings.",
  locked: "The browser is using this data. Quit the browser and try again.",
  "key-unavailable": "The browser's encryption key was not available (Keychain or keyring access was denied).",
  parse: "The data could not be read.",
  io: "Reading the data failed.",
  cancelled: "Cancelled.",
  "no-destination": "Open a tab in this profile, then import again.",
  unsupported: "This browser does not offer this data.",
  "no-file": "Choose the passwords file exported from the browser.",
  "sink-failed": "Ferryx could not save the imported data.",
};

/** One line per type: counts, or the reason the type failed. */
export function describeBrowserImportResult(result: BrowserImportTypeResult): string {
  const label = browserImportTypeLabel(result.type);
  if (result.error) {
    const reason = TYPE_ERROR_MESSAGES[result.error.code] ?? result.error.message;
    return `${label}: ${reason}`;
  }
  const skipped = result.skipped > 0 ? `, ${result.skipped} skipped` : "";
  return `${label}: ${result.imported} imported${skipped}`;
}

const RUN_ERROR_MESSAGES: Record<string, string> = {
  "unknown-source": "That browser is no longer available. Refresh the list and pick it again.",
  "invalid-profile": "Choose a Ferryx profile to import into.",
  "private-profile": "Private browsing keeps nothing. Choose another profile.",
  busy: "Another import is running.",
  "no-data-dir": "Ferryx could not find its data folder.",
};

/** The message for a whole-command failure (`BROWSER_IMPORT_FAILED` + `details.reason`). */
export function formatBrowserImportError(error: unknown): string {
  if (error && typeof error === "object") {
    const record = error as { code?: unknown; message?: unknown; details?: { reason?: unknown } };
    const reason = typeof record.details?.reason === "string" ? record.details.reason : null;
    if (reason && RUN_ERROR_MESSAGES[reason]) return RUN_ERROR_MESSAGES[reason];
    if (typeof record.message === "string" && record.message.trim()) return record.message.trim();
    if (typeof record.code === "string") return record.code;
  }
  if (typeof error === "string" && error.trim()) return error.trim();
  return "Import failed.";
}
