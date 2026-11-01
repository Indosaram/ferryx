import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BrowserSection } from "./BrowserSection";

vi.mock("./SiteSettingsGroup", () => ({ SiteSettingsGroup: () => null }));
// The agent access group has its own tests and talks to the backend on mount.
vi.mock("./BrowserAgentAccessGroup", () => ({ BrowserAgentAccessGroup: () => null }));

vi.mock("../../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserTauri")>()),
  listBrowsers: vi.fn(async () => [
    { browserId: "b1", title: "Docs", url: "https://example.test", profileId: "default" },
  ]),
  focusBrowser: vi.fn(async () => {
    throw new Error("BROWSER_NOT_FOUND");
  }),
  setBrowserZoom: vi.fn(async () => {
    throw new Error("zoom refused");
  }),
  getBrowserEngineStatus: vi.fn(async () => ({ state: "unsupported" as const })),
}));

vi.mock("../../lib/browserExtensions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserExtensions")>()),
  listBrowserExtensions: vi.fn(async () => ({ profile: "default", engineInstalled: false, supported: false, extensions: [] })),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../../lib/browserVault", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserVault")>()),
  getVaultStatus: vi.fn(async () => ({ available: true, backend: "keychain" as const })),
  listVaultItems: vi.fn(async () => []),
}));

describe("BrowserSection failure handling", () => {
  afterEach(cleanup);

  it("shows an error alert when focusing an active browser tab fails", async () => {
    render(<BrowserSection />);

    expect(await screen.findByText("Docs")).toBeInTheDocument();

    const focusBtn = screen.getByRole("button", { name: "Focus browser tab Docs" });
    fireEvent.click(focusBtn);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Focus failed: BROWSER_NOT_FOUND");
  });

  it("shows an error alert when resetting settings fails to apply zoom to open tabs", async () => {
    render(<BrowserSection />);

    expect(await screen.findByText("Docs")).toBeInTheDocument();

    const resetBtn = screen.getByRole("button", { name: "Reset to defaults" });
    fireEvent.click(resetBtn);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("could not be applied to every open tab: zoom refused");
  });
});
