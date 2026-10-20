import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteSection } from "./RemoteSection";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { createPairedHostInventory } from "../../lib/pairedHostInventory";
import * as accountSessionModule from "../../remote/accountSession";

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("RemoteSection Sign-out & Auth Error Handling", () => {
  const originalFetch = globalThis.fetch;
  const customOrigin = "https://relay.custom.dev";

  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    localStorage.clear();
    sessionStorage.clear();
    vi.restoreAllMocks();
  });

  it("handleSignOut calls logoutAccountSession with issuer origin and token, and clears credentials", async () => {
    const store = createRemoteHostStore();
    const inventory = createPairedHostInventory(store);
    await inventory.refresh();

    accountSessionModule.storeAccountSessionToken("test-session-tok", customOrigin);

    let logoutCalledWithOrigin = "";
    let logoutCalledWithToken = "";
    const logoutSpy = vi.spyOn(accountSessionModule, "logoutAccountSession").mockImplementation(async (origin, tok) => {
      logoutCalledWithOrigin = origin;
      logoutCalledWithToken = tok;
    });

    globalThis.fetch = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
      }
      return new Response("Not found", { status: 404 });
    });

    await act(async () => {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={customOrigin}
          accountSessionToken="test-session-tok"
        />
      );
    });

    const signOutBtn = screen.getByRole("button", { name: "Sign Out" });
    expect(signOutBtn).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(signOutBtn);
    });

    expect(logoutSpy).toHaveBeenCalledTimes(1);
    expect(logoutCalledWithOrigin).toBe(customOrigin);
    expect(logoutCalledWithToken).toBe("test-session-tok");
    expect(accountSessionModule.getStoredAccountSessionToken(customOrigin)).toBeNull();
  });

  it("keeps account token when listMachines throws untyped 401 (e.g. gateway error code LIST_MACHINES_FAILED)", async () => {
    const store = createRemoteHostStore();
    const inventory = createPairedHostInventory(store);
    await inventory.refresh();

    accountSessionModule.storeAccountSessionToken("test-session-tok", customOrigin);

    globalThis.fetch = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return new Response("Gateway 401 Proxy Error", {
          status: 401,
          headers: { "Content-Type": "text/plain" },
        });
      }
      return new Response("Not found", { status: 404 });
    });

    await act(async () => {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={customOrigin}
          accountSessionToken="test-session-tok"
        />
      );
    });

    await waitFor(() => {
      expect(screen.getByText(/Failed to list account machines/i)).toBeInTheDocument();
    });

    expect(accountSessionModule.getStoredAccountSessionToken(customOrigin)).toBe("test-session-tok");
  });

  it("clears account token when listMachines throws structured 401 with code UNAUTHORIZED", async () => {
    const store = createRemoteHostStore();
    const inventory = createPairedHostInventory(store);
    await inventory.refresh();

    accountSessionModule.storeAccountSessionToken("expired-session-tok", customOrigin);

    globalThis.fetch = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/machines")) {
        return new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "Session expired" }), {
          status: 401,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response("Not found", { status: 404 });
    });

    await act(async () => {
      render(
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={customOrigin}
          accountSessionToken="expired-session-tok"
        />
      );
    });

    await waitFor(() => {
      expect(accountSessionModule.getStoredAccountSessionToken(customOrigin)).toBeNull();
    });
  });
});
