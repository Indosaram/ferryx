import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BrowserSection } from "./BrowserSection";
import * as browserImport from "../../lib/browserImport";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
// These groups have their own tests and talk to the backend on mount.
vi.mock("./SiteSettingsGroup", () => ({ SiteSettingsGroup: () => null }));
vi.mock("./BrowserAgentAccessGroup", () => ({ BrowserAgentAccessGroup: () => null }));
vi.mock("./DefaultBrowserGroup", () => ({ DefaultBrowserGroup: () => null }));
vi.mock("./BrowserPasswordsGroup", () => ({ BrowserPasswordsGroup: () => null }));
vi.mock("../../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserTauri")>()),
  listBrowsers: vi.fn(async () => []),
}));
vi.mock("../../lib/browserImport", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserImport")>()),
  listBrowserImportSources: vi.fn(async () => []),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("BrowserSection import", () => {
  it("opens the import dialog, which never offers the private profile as a target", async () => {
    localStorage.clear();
    render(<BrowserSection />);
    fireEvent.click(screen.getByRole("button", { name: "Import browser data…" }));

    expect(await screen.findByRole("dialog", { name: "Import browser data" })).toBeTruthy();
    expect(browserImport.listBrowserImportSources).toHaveBeenCalledTimes(1);
    const targets = Array.from((screen.getByLabelText("Into Ferryx profile") as HTMLSelectElement).options).map(
      (option) => option.value,
    );
    expect(targets).toContain("default");
    expect(targets).not.toContain("private");
  });
});
