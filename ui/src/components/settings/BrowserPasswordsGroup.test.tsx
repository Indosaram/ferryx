import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserPasswordsGroup } from "./BrowserPasswordsGroup";

const vault = vi.hoisted(() => ({
  getVaultStatus: vi.fn(),
  listVaultItems: vi.fn(),
  revealVaultPassword: vi.fn(),
  updateVaultItem: vi.fn(),
  deleteVaultItem: vi.fn(),
  generateVaultPassword: vi.fn(),
}));

vi.mock("../../lib/browserVault", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserVault")>()),
  ...vault,
}));

vi.mock("../../lib/clipboard", () => ({ copyTextToClipboard: vi.fn().mockResolvedValue(true) }));

const item = { id: "v1", origin: "https://example.com", username: "ada", createdAt: 1, updatedAt: 2 };

describe("BrowserPasswordsGroup", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vault.getVaultStatus.mockResolvedValue({ available: true, backend: "keychain" });
    vault.listVaultItems.mockResolvedValue([item]);
  });
  afterEach(cleanup);

  it("explains a missing system keyring in plain words instead of an error code", async () => {
    vault.getVaultStatus.mockResolvedValue({
      available: false,
      backend: "secretService",
      error: "the platform key store is unavailable: org.freedesktop.secrets was not provided",
      errorCode: "BROWSER_VAULT_UNAVAILABLE",
    });
    render(<BrowserPasswordsGroup />);
    const notice = await screen.findByTestId("vault-unavailable");
    expect(notice).toHaveAttribute("role", "status");
    expect(notice).toHaveTextContent(/system keyring/);
    expect(notice).not.toHaveTextContent("BROWSER_VAULT_UNAVAILABLE");
    expect(screen.queryByTestId("vault-search")).toBeNull();
    expect(vault.listVaultItems).not.toHaveBeenCalled();
  });

  it("keeps other vault failures as an alert", async () => {
    vault.getVaultStatus.mockResolvedValue({ available: false, backend: "keychain", error: "vault file is damaged", errorCode: "BROWSER_VAULT_CORRUPTED" });
    render(<BrowserPasswordsGroup />);
    expect(await screen.findByTestId("vault-unavailable")).toHaveAttribute("role", "alert");
  });

  it("renders rows and deletes one", async () => {
    vault.deleteVaultItem.mockResolvedValue(undefined);
    render(<BrowserPasswordsGroup />);
    expect(await screen.findAllByTestId("vault-row")).toHaveLength(1);
    fireEvent.click(screen.getByTestId("vault-delete"));
    await vi.waitFor(() => expect(screen.queryByTestId("vault-row")).toBeNull());
    expect(vault.deleteVaultItem).toHaveBeenCalledWith("v1");
    expect(screen.getByTestId("vault-empty")).toBeInTheDocument();
  });

  it("reveals the password inline", async () => {
    vault.revealVaultPassword.mockResolvedValue("hunter2");
    render(<BrowserPasswordsGroup />);
    fireEvent.click(await screen.findByTestId("vault-reveal"));
    expect(await screen.findByTestId("vault-password")).toHaveTextContent("hunter2");
    expect(vault.revealVaultPassword).toHaveBeenCalledWith("v1");
    fireEvent.click(screen.getByTestId("vault-reveal"));
    await vi.waitFor(() => expect(screen.queryByTestId("vault-password")).toBeNull());
  });

  it("keeps the stored password when the edit password is blank", async () => {
    vault.updateVaultItem.mockResolvedValue({ ...item, username: "grace" });
    render(<BrowserPasswordsGroup />);
    fireEvent.click(await screen.findByTestId("vault-edit"));
    fireEvent.change(screen.getByTestId("vault-edit-username"), { target: { value: "grace" } });
    fireEvent.click(screen.getByTestId("vault-edit-save"));
    await vi.waitFor(() => expect(vault.updateVaultItem).toHaveBeenCalledWith("v1", "grace", null));
    await vi.waitFor(() => expect(screen.queryByTestId("vault-edit-save")).toBeNull());
  });

  it("shows a generated password for the requested length", async () => {
    vault.generateVaultPassword.mockResolvedValue({ password: "s3cr3t-generated", score: 4, entropyBits: 120 });
    render(<BrowserPasswordsGroup />);
    fireEvent.change(await screen.findByTestId("vault-generate-length"), { target: { value: "32" } });
    fireEvent.click(screen.getByTestId("vault-generate"));
    expect(await screen.findByTestId("vault-generated")).toHaveTextContent("s3cr3t-generated");
    expect(vault.generateVaultPassword).toHaveBeenCalledWith({ length: 32 });
  });

  it("surfaces a readable message when an action fails", async () => {
    vault.revealVaultPassword.mockRejectedValue({ code: "BROWSER_VAULT_NOT_FOUND", message: "gone" });
    render(<BrowserPasswordsGroup />);
    fireEvent.click(await screen.findByTestId("vault-reveal"));
    expect(await screen.findByRole("alert")).toHaveTextContent("That saved login no longer exists.");
  });
});
