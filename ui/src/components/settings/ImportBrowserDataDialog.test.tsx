import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ImportBrowserDataDialog } from "./ImportBrowserDataDialog";
import type { BrowserImportReport, BrowserImportSource } from "../../lib/browserImport";

const api = vi.hoisted(() => ({
  listBrowserImportSources: vi.fn(),
  runBrowserImport: vi.fn(),
  cancelBrowserImport: vi.fn(),
  importPasswordFile: vi.fn(),
}));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("../../lib/browserImport", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserImport")>()),
  ...api,
}));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

const chrome: BrowserImportSource = {
  id: "chrome:Default",
  browser: "chrome",
  name: "Chrome",
  types: ["bookmarks", "history", "cookies", "passwords"],
  requiresFullDiskAccess: false,
  passwordsNeedFile: false,
};
const safari: BrowserImportSource = {
  id: "safari:Safari",
  browser: "safari",
  name: "Safari",
  types: ["bookmarks", "passwords"],
  requiresFullDiskAccess: true,
  passwordsNeedFile: true,
};
const profiles = [
  { id: "default", name: "Default" },
  { id: "work", name: "Work" },
];

function renderDialog() {
  const onOpenChange = vi.fn();
  render(<ImportBrowserDataDialog open onOpenChange={onOpenChange} profiles={profiles} defaultProfileId="default" />);
  return onOpenChange;
}

describe("ImportBrowserDataDialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.listBrowserImportSources.mockResolvedValue([chrome, safari]);
  });
  afterEach(cleanup);

  it("imports the checked types from the chosen browser into the chosen profile", async () => {
    const report: BrowserImportReport = {
      results: [
        { type: "history", imported: 40, skipped: 0, notes: [], error: null },
        {
          type: "cookies",
          imported: 0,
          skipped: 0,
          notes: [],
          error: { code: "key-unavailable", message: "denied" },
        },
      ],
    };
    api.runBrowserImport.mockResolvedValue(report);
    renderDialog();

    expect(await screen.findByRole("option", { name: "Chrome" })).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Bookmarks" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Passwords" }));
    fireEvent.change(screen.getByLabelText("Into Ferryx profile"), { target: { value: "work" } });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    await waitFor(() => expect(api.runBrowserImport).toHaveBeenCalledTimes(1));
    expect(api.runBrowserImport).toHaveBeenCalledWith({
      sourceId: "chrome:Default",
      profileId: "work",
      types: ["history", "cookies"],
    });
    const results = await screen.findByRole("list", { name: "Import results" });
    expect(results.textContent).toContain("History: 40 imported");
    expect(results.textContent).toContain("Cookies: The browser's encryption key was not available");
  });

  it("asks for the Safari passwords CSV before importing passwords", async () => {
    api.runBrowserImport.mockResolvedValue({ results: [] });
    dialog.open.mockResolvedValue("/Users/me/Passwords.csv");
    renderDialog();

    await screen.findByRole("option", { name: "Safari" });
    fireEvent.change(screen.getByLabelText("Import from"), { target: { value: "safari:Safari" } });
    expect(await screen.findByText(/Full Disk Access/)).toBeTruthy();
    const importButton = screen.getByRole("button", { name: "Import" }) as HTMLButtonElement;
    expect(importButton.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    expect(await screen.findByText("/Users/me/Passwords.csv")).toBeTruthy();
    expect(importButton.disabled).toBe(false);
    fireEvent.click(importButton);

    await waitFor(() =>
      expect(api.runBrowserImport).toHaveBeenCalledWith({
        sourceId: "safari:Safari",
        profileId: "default",
        types: ["bookmarks", "passwords"],
        passwordCsvPath: "/Users/me/Passwords.csv",
      }),
    );
  });

  it("imports a password manager export file", async () => {
    api.importPasswordFile.mockResolvedValue({ type: "passwords", imported: 7, skipped: 1, notes: [], error: null });
    dialog.open.mockResolvedValue("/tmp/vault.json");
    renderDialog();

    await screen.findByRole("option", { name: "Chrome" });
    fireEvent.change(screen.getByLabelText("Import from"), { target: { value: "password-file" } });
    fireEvent.change(screen.getByLabelText("File format"), { target: { value: "bitwardenJson" } });
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    await screen.findByText("/tmp/vault.json");
    expect(dialog.open).toHaveBeenCalledWith({
      multiple: false,
      filters: [{ name: "Password export", extensions: ["json"] }],
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    expect(await screen.findByText("Passwords: 7 imported, 1 skipped")).toBeTruthy();
    expect(api.importPasswordFile).toHaveBeenCalledWith({
      format: "bitwardenJson",
      path: "/tmp/vault.json",
      profileId: "default",
    });
  });

  it("shows a whole-import failure with its reason", async () => {
    api.runBrowserImport.mockRejectedValue({
      code: "BROWSER_IMPORT_FAILED",
      message: "another import is running",
      details: { reason: "busy" },
    });
    renderDialog();
    await screen.findByRole("option", { name: "Chrome" });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Another import is running.");
  });

  it("says when no browsers were found and still offers export files", async () => {
    api.listBrowserImportSources.mockResolvedValue([]);
    renderDialog();
    expect(await screen.findByText("No other browsers were found on this computer.")).toBeTruthy();
    expect((screen.getByLabelText("Import from") as HTMLSelectElement).value).toBe("password-file");
  });
});
