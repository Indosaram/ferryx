import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, waitFor } from "@testing-library/react";
import { AccountLoginPage } from "./AccountLoginPage";
import * as accountSessionModule from "./accountSession";

describe("AccountLoginPage", () => {
  const relayUrl = "https://relay.example.com";
  let originalLocation: Location;

  beforeEach(() => {
    originalLocation = window.location;
    vi.restoreAllMocks();
  });

  afterEach(() => {
    Object.defineProperty(window, "location", {
      configurable: true,
      value: originalLocation,
    });
  });

  it("consumes login code from URL query parameter ?code=<hex>", async () => {
    const hexCode = "4a9f3b8c2d1e0f7a5b6c3d2e1f0a9b8c";
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      search: `?code=${hexCode}`,
      hash: "",
      pathname: "/login",
    } as any;

    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-123",
      accountId: "acc-1",
      email: "alice@example.com",
    });

    const onLoginSuccess = vi.fn();
    const onUseLegacyPin = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
        onUseLegacyPin={onUseLegacyPin}
      />
    );

    await waitFor(() => {
      expect(consumeSpy).toHaveBeenCalledWith(relayUrl, hexCode);
      expect(onLoginSuccess).toHaveBeenCalledWith("jwt-session-token-123", "alice@example.com");
    });
  });

  it("consumes login code from URL hash #code=<hex>", async () => {
    const hexCode = "aabbccddeeff00112233445566778899";
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      search: "",
      hash: `#code=${hexCode}`,
      pathname: "/login",
    } as any;

    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-456",
      accountId: "acc-2",
      email: "bob@example.com",
    });

    const onLoginSuccess = vi.fn();
    const onUseLegacyPin = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
        onUseLegacyPin={onUseLegacyPin}
      />
    );

    await waitFor(() => {
      expect(consumeSpy).toHaveBeenCalledWith(relayUrl, hexCode);
      expect(onLoginSuccess).toHaveBeenCalledWith("jwt-session-token-456", "bob@example.com");
    });
  });

  it("preserves backward compatibility with legacy #login= and ?login= parameters", async () => {
    const legacyCode = "legacy-login-code-999";
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      search: `?login=${legacyCode}`,
      hash: "",
      pathname: "/login",
    } as any;

    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-789",
      accountId: "acc-3",
      email: "charlie@example.com",
    });

    const onLoginSuccess = vi.fn();
    const onUseLegacyPin = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
        onUseLegacyPin={onUseLegacyPin}
      />
    );

    await waitFor(() => {
      expect(consumeSpy).toHaveBeenCalledWith(relayUrl, legacyCode);
      expect(onLoginSuccess).toHaveBeenCalledWith("jwt-session-token-789", "charlie@example.com");
    });
  });

  it("preserves backward compatibility with legacy ?account_token= parameter", async () => {
    const legacyToken = "legacy-account-token-333";
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      search: `?account_token=${legacyToken}`,
      hash: "",
      pathname: "/login",
    } as any;

    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-000",
      accountId: "acc-4",
      email: "dave@example.com",
    });

    const onLoginSuccess = vi.fn();
    const onUseLegacyPin = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
        onUseLegacyPin={onUseLegacyPin}
      />
    );

    await waitFor(() => {
      expect(consumeSpy).toHaveBeenCalledWith(relayUrl, legacyToken);
      expect(onLoginSuccess).toHaveBeenCalledWith("jwt-session-token-000", "dave@example.com");
    });
  });
});
