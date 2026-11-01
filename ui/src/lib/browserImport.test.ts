import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  cancelBrowserImport,
  describeBrowserImportResult,
  formatBrowserImportError,
  importPasswordFile,
  listBrowserImportSources,
  runBrowserImport,
} from "./browserImport";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

afterEach(() => {
  vi.clearAllMocks();
});

describe("browser import IPC", () => {
  it("sends each command with its request shape", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    await listBrowserImportSources();
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_import_sources");

    vi.mocked(invoke).mockResolvedValue({ results: [] });
    await runBrowserImport({ sourceId: "chrome:Default", profileId: "default", types: ["cookies"] });
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_import_run", {
      request: { sourceId: "chrome:Default", profileId: "default", types: ["cookies"] },
    });

    vi.mocked(invoke).mockResolvedValue(true);
    await cancelBrowserImport();
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_import_cancel");

    vi.mocked(invoke).mockResolvedValue({});
    await importPasswordFile({ format: "bitwardenJson", path: "/tmp/a.json", profileId: "work" });
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_import_password_file", {
      request: { format: "bitwardenJson", path: "/tmp/a.json", profileId: "work" },
    });
  });
});

describe("describeBrowserImportResult", () => {
  it("reports counts, and a known error code as its explanation", () => {
    expect(
      describeBrowserImportResult({ type: "history", imported: 12, skipped: 0, notes: [], error: null }),
    ).toBe("History: 12 imported");
    expect(
      describeBrowserImportResult({ type: "cookies", imported: 3, skipped: 2, notes: [], error: null }),
    ).toBe("Cookies: 3 imported, 2 skipped");
    expect(
      describeBrowserImportResult({
        type: "passwords",
        imported: 0,
        skipped: 0,
        notes: [],
        error: { code: "locked", message: "in use" },
      }),
    ).toBe("Passwords: The browser is using this data. Quit the browser and try again.");
    expect(
      describeBrowserImportResult({
        type: "bookmarks",
        imported: 0,
        skipped: 0,
        notes: [],
        error: { code: "something-new", message: "raw detail" },
      }),
    ).toBe("Bookmarks: raw detail");
  });
});

describe("formatBrowserImportError", () => {
  it("maps details.reason, then falls back to the message", () => {
    expect(
      formatBrowserImportError({ code: "BROWSER_IMPORT_FAILED", message: "x", details: { reason: "busy" } }),
    ).toBe("Another import is running.");
    expect(formatBrowserImportError({ code: "INTERNAL_ERROR", message: "boom" })).toBe("boom");
    expect(formatBrowserImportError(undefined)).toBe("Import failed.");
  });
});
