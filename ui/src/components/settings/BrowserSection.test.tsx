import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserSection } from "./BrowserSection";
import * as browserTauri from "../../lib/browserTauri";
import * as platform from "../../lib/platform";
import { BROWSER_SETTINGS_STORAGE_KEY } from "../../lib/storageKeys";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("./SiteSettingsGroup", () => ({ SiteSettingsGroup: () => null }));
// The agent access group has its own tests and talks to the backend on mount.
vi.mock("./BrowserAgentAccessGroup", () => ({ BrowserAgentAccessGroup: () => null }));
// The default-browser group has its own tests and queries the OS on mount.
vi.mock("./DefaultBrowserGroup", () => ({ DefaultBrowserGroup: () => null }));
vi.mock("../../lib/browserVault", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserVault")>()),
  getVaultStatus: vi.fn(async () => ({ available: true, backend: "keychain" as const })),
  listVaultItems: vi.fn(async () => []),
}));

vi.mock("../../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/browserTauri")>();
  return {
    ...actual,
    focusBrowser: vi.fn(async () => undefined),
    importBrowserCookies: vi.fn(async () => 10),
    importInstalledBrowserCookies: vi.fn(async () => ({ importedCount: 15, skippedCount: 0 })),
    listBrowsers: vi.fn(async () => []),
    setBrowserZoom: vi.fn(async () => 1.0),
  };
});

vi.mock("../../lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/platform")>()),
  getHostPlatform: vi.fn(() => "macos"),
  isInstalledBrowserCookieImportSupported: vi.fn(() => true),
  isMacHost: vi.fn(() => true),
  isWindowsHost: vi.fn(() => false),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("BrowserSection installed cookie import", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(platform.isInstalledBrowserCookieImportSupported).mockReturnValue(true);
    vi.mocked(platform.isMacHost).mockReturnValue(true);
  });

  it("clicking Import from Chrome invokes importInstalledBrowserCookies with profileId and chrome source", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockResolvedValueOnce({
      importedCount: 25,
      skippedCount: 0,
    });

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    expect(chromeButtons.length).toBeGreaterThan(0);

    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(browserTauri.importInstalledBrowserCookies).toHaveBeenCalledTimes(1);
      expect(browserTauri.importInstalledBrowserCookies).toHaveBeenCalledWith("default", "chrome");
      expect(screen.getByText("Imported 25 cookies")).toBeInTheDocument();
    });
  });

  it("clicking Import from Edge invokes importInstalledBrowserCookies with profileId and edge source", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockResolvedValueOnce({
      importedCount: 12,
      skippedCount: 0,
    });

    render(<BrowserSection />);

    const edgeButtons = screen.getAllByRole("button", { name: "Import from Edge" });
    expect(edgeButtons.length).toBeGreaterThan(0);

    fireEvent.click(edgeButtons[0]);

    await waitFor(() => {
      expect(browserTauri.importInstalledBrowserCookies).toHaveBeenCalledTimes(1);
      expect(browserTauri.importInstalledBrowserCookies).toHaveBeenCalledWith("default", "edge");
      expect(screen.getByText("Imported 12 cookies")).toBeInTheDocument();
    });
  });

  it("surfaces structured reason and actionable message when importInstalledBrowserCookies rejects with keychain-denied", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockRejectedValueOnce({
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "keychain-denied: Keychain access for Chrome Safe Storage failed",
      details: { reason: "keychain-denied" },
    });

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(
        screen.getByText(
          "Cookie import failed: keychain-denied: Allow Keychain access in the macOS prompt to import cookies.",
        ),
      ).toBeInTheDocument();
    });
  });

  it("surfaces structured reason and actionable message when importInstalledBrowserCookies rejects with no-tab", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockRejectedValueOnce({
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "open a browser tab using the default profile before importing cookies",
      details: { reason: "no-tab" },
    });

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(
        screen.getByText(
          "Cookie import failed: no-tab: Open a browser tab using this profile before importing cookies.",
        ),
      ).toBeInTheDocument();
    });
  });

  it("preserves error code semantics when error payload lacks structured reason", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockRejectedValueOnce({
      code: "BROWSER_COOKIE_IMPORT_FAILED",
      message: "Cookie import failed",
    });

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(screen.getByText("Cookie import failed: BROWSER_COOKIE_IMPORT_FAILED")).toBeInTheDocument();
    });
  });

  it("falls back to UNKNOWN_ERROR when error payload is empty or null", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockRejectedValueOnce(null);

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(screen.getByText("Cookie import failed: UNKNOWN_ERROR")).toBeInTheDocument();
    });
  });

  it("renders skipped count when skippedCount > 0 and omits skipped text when skippedCount is 0", async () => {
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockResolvedValueOnce({
      importedCount: 30,
      skippedCount: 4,
    });

    render(<BrowserSection />);

    const chromeButtons = screen.getAllByRole("button", { name: "Import from Chrome" });
    fireEvent.click(chromeButtons[0]);

    await waitFor(() => {
      expect(screen.getByText("Imported 30 cookies (4 skipped)")).toBeInTheDocument();
    });
  });

  it("renders no import buttons on Windows platform and shows muted unsupported note", () => {
    vi.mocked(platform.isInstalledBrowserCookieImportSupported).mockReturnValue(false);
    vi.mocked(platform.isMacHost).mockReturnValue(false);

    render(<BrowserSection />);

    expect(screen.queryByRole("button", { name: "Import from Chrome" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Import from Edge" })).toBeNull();
    expect(
      screen.getAllByText(
        "Installed browser cookie import is unavailable on Windows (Chromium app-bound encryption)",
      ).length,
    ).toBeGreaterThan(0);
  });

  it("renders macOS Keychain access hint on macOS platform", () => {
    vi.mocked(platform.isInstalledBrowserCookieImportSupported).mockReturnValue(true);
    vi.mocked(platform.isMacHost).mockReturnValue(true);

    render(<BrowserSection />);

    expect(
      screen.getAllByText("macOS will ask for Keychain access").length,
    ).toBeGreaterThan(0);
  });
});

describe("BrowserSection review fixes", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(platform.isInstalledBrowserCookieImportSupported).mockReturnValue(true);
    vi.mocked(platform.isMacHost).mockReturnValue(false);
  });

  it("disables a profile's import buttons while its import is running", async () => {
    let finish!: (value: { importedCount: number; skippedCount: number }) => void;
    vi.mocked(browserTauri.importInstalledBrowserCookies).mockReturnValueOnce(
      new Promise((resolve) => { finish = resolve; }),
    );
    render(<BrowserSection />);
    const chrome = screen.getAllByRole("button", { name: "Import from Chrome" })[0];
    const edge = screen.getAllByRole("button", { name: "Import from Edge" })[0];
    fireEvent.click(chrome);
    await waitFor(() => expect(edge).toBeDisabled());
    expect(chrome).toBeDisabled();
    finish({ importedCount: 1, skippedCount: 0 });
    await waitFor(() => expect(edge).toBeEnabled());
  });

  it("exposes an invalid home page as an alert linked to the input", () => {
    render(<BrowserSection />);
    const input = screen.getByLabelText("Default Home Page");
    fireEvent.change(input, { target: { value: "ftp://example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(input).toHaveAttribute("aria-invalid", "true");
    const alert = screen.getByRole("alert");
    expect(input.getAttribute("aria-describedby")).toBe(alert.id);
  });

  it("requires confirmation before deleting a profile", () => {
    localStorage.setItem(BROWSER_SETTINGS_STORAGE_KEY, JSON.stringify({ profiles: [{ id: "work", name: "Work" }] }));
    render(<BrowserSection />);
    fireEvent.click(screen.getByRole("button", { name: "Delete profile Work" }));
    expect(screen.getByLabelText("Profile name work")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm delete profile Work" }));
    expect(screen.queryByLabelText("Profile name work")).toBeNull();
  });

  it("restores the visible name when a rename is blanked", () => {
    localStorage.setItem(BROWSER_SETTINGS_STORAGE_KEY, JSON.stringify({ profiles: [{ id: "work", name: "Work" }] }));
    render(<BrowserSection />);
    const input = screen.getByLabelText("Profile name work") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);
    expect(input.value).toBe("Work");
  });
});
