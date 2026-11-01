import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserVaultBar } from "./BrowserVaultBar";
import type { VaultSavePrompt } from "../lib/browserVault";

const vault = vi.hoisted(() => ({
  onVaultSavePrompt: vi.fn(),
  saveVaultPrompt: vi.fn(),
  dismissVaultPrompt: vi.fn(),
  vaultMatchesForBrowser: vi.fn(),
  fillVaultItem: vi.fn(),
}));

vi.mock("../lib/browserVault", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/browserVault")>()),
  ...vault,
}));

const match = { id: "v1", origin: "https://example.com", username: "ada", createdAt: 1, updatedAt: 2 };
let emit: (prompt: VaultSavePrompt) => void;

beforeEach(() => {
  vi.resetAllMocks();
  vault.onVaultSavePrompt.mockImplementation(async (handler: (prompt: VaultSavePrompt) => void) => {
    emit = handler;
    return () => undefined;
  });
  vault.vaultMatchesForBrowser.mockResolvedValue([]);
});
afterEach(cleanup);

const prompt = (overrides: Partial<VaultSavePrompt> = {}): VaultSavePrompt => ({
  browserId: "b1",
  promptId: "p1",
  origin: "https://example.com",
  username: "ada",
  update: false,
  ...overrides,
});

describe("BrowserVaultBar", () => {
  it("ignores save prompts for other browsers", async () => {
    render(<BrowserVaultBar browserId="b1" url="https://example.com/login" />);
    await vi.waitFor(() => expect(vault.onVaultSavePrompt).toHaveBeenCalled());
    act(() => emit(prompt({ browserId: "other" })));
    expect(screen.queryByTestId("vault-save-prompt")).toBeNull();
  });

  it("saves a prompt by id and hides the bar", async () => {
    vault.saveVaultPrompt.mockResolvedValue(match);
    render(<BrowserVaultBar browserId="b1" url="https://example.com/login" />);
    await vi.waitFor(() => expect(vault.onVaultSavePrompt).toHaveBeenCalled());
    act(() => emit(prompt()));
    fireEvent.click(await screen.findByTestId("vault-save-accept"));
    await vi.waitFor(() => expect(screen.queryByTestId("vault-save-prompt")).toBeNull());
    expect(vault.saveVaultPrompt).toHaveBeenCalledWith("p1");
  });

  it("dismisses a prompt by id", async () => {
    vault.dismissVaultPrompt.mockResolvedValue(undefined);
    render(<BrowserVaultBar browserId="b1" url="https://example.com/login" />);
    await vi.waitFor(() => expect(vault.onVaultSavePrompt).toHaveBeenCalled());
    act(() => emit(prompt({ update: true })));
    fireEvent.click(await screen.findByTestId("vault-save-dismiss"));
    await vi.waitFor(() => expect(screen.queryByTestId("vault-save-prompt")).toBeNull());
    expect(vault.dismissVaultPrompt).toHaveBeenCalledWith("p1");
  });

  it("shows a fill button per match and fills on click", async () => {
    vault.vaultMatchesForBrowser.mockResolvedValue([match]);
    vault.fillVaultItem.mockResolvedValue(undefined);
    render(<BrowserVaultBar browserId="b1" url="https://example.com/login" />);
    fireEvent.click(await screen.findByTestId("vault-fill"));
    await vi.waitFor(() => expect(vault.fillVaultItem).toHaveBeenCalledWith("b1", "v1"));
  });

  it("hides the offer until the url changes", async () => {
    vault.vaultMatchesForBrowser.mockResolvedValue([match]);
    const { rerender } = render(<BrowserVaultBar browserId="b1" url="https://example.com/a" />);
    fireEvent.click(await screen.findByTestId("vault-fill-dismiss"));
    expect(screen.queryByTestId("vault-fill-offer")).toBeNull();
    rerender(<BrowserVaultBar browserId="b1" url="https://example.com/b" />);
    expect(await screen.findByTestId("vault-fill-offer")).toBeInTheDocument();
  });

  it("re-queries matches when the url changes and skips non-web urls", async () => {
    const { rerender } = render(<BrowserVaultBar browserId="b1" url="https://example.com/a" />);
    await vi.waitFor(() => expect(vault.vaultMatchesForBrowser).toHaveBeenCalledTimes(1));
    rerender(<BrowserVaultBar browserId="b1" url="https://example.com/b" />);
    await vi.waitFor(() => expect(vault.vaultMatchesForBrowser).toHaveBeenCalledTimes(2));
    rerender(<BrowserVaultBar browserId="b1" url="about:blank" />);
    expect(vault.vaultMatchesForBrowser).toHaveBeenCalledTimes(2);
  });

  it("shows the error code when filling fails", async () => {
    vault.vaultMatchesForBrowser.mockResolvedValue([match]);
    vault.fillVaultItem.mockRejectedValue({ code: "UNSUPPORTED_CAPABILITY", message: "no isolated world" });
    render(<BrowserVaultBar browserId="b1" url="https://example.com/login" />);
    fireEvent.click(await screen.findByTestId("vault-fill"));
    expect(await screen.findByRole("alert")).toHaveTextContent("UNSUPPORTED_CAPABILITY");
  });
});
