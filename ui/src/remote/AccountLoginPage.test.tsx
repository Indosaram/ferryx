import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { AccountLoginPage } from "./AccountLoginPage";
import * as accountSessionModule from "./accountSession";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (err: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("AccountLoginPage", () => {
  const relayUrl = "https://relay.example.com";
  let originalLocation: Location;

  beforeEach(() => {
    originalLocation = window.location;
    vi.restoreAllMocks();
  });

  afterEach(() => {
    // React Testing Library auto-cleanup is not enabled in this suite, so every
    // test must unmount explicitly; otherwise renders leak into the next test
    // and duplicate data-testid lookups.
    cleanup();
    vi.useRealTimers();
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

    const resolveSpy = vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-123",
      accountId: "acc-1",
      email: "alice@example.com",
    });

    const onLoginSuccess = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
      />
    );

    await waitFor(() => {
      expect(resolveSpy).toHaveBeenCalledWith(relayUrl);
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

    const resolveSpy = vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-456",
      accountId: "acc-2",
      email: "bob@example.com",
    });

    const onLoginSuccess = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
      />
    );

    await waitFor(() => {
      expect(resolveSpy).toHaveBeenCalledWith(relayUrl);
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

    const resolveSpy = vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-789",
      accountId: "acc-3",
      email: "charlie@example.com",
    });

    const onLoginSuccess = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
      />
    );

    await waitFor(() => {
      expect(resolveSpy).toHaveBeenCalledWith(relayUrl);
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

    const resolveSpy = vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockResolvedValue({
      token: "jwt-session-token-000",
      accountId: "acc-4",
      email: "dave@example.com",
    });

    const onLoginSuccess = vi.fn();

    render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
      />
    );

    await waitFor(() => {
      expect(resolveSpy).toHaveBeenCalledWith(relayUrl);
      expect(consumeSpy).toHaveBeenCalledWith(relayUrl, legacyToken);
      expect(onLoginSuccess).toHaveBeenCalledWith("jwt-session-token-000", "dave@example.com");
    });
  });

  it("consumes magic link code exactly once under StrictMode and invokes latest onLoginSuccess across rerenders while consume promise is pending", async () => {
    const hexCode = "7c8d9e0f1a2b3c4d5e6f708192a3b4c5";
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      search: `?code=${hexCode}`,
      hash: "",
      pathname: "/login",
    } as any;

    let resolveConsume!: (value: { token: string; accountId: string; email: string }) => void;
    const consumePromise = new Promise<{ token: string; accountId: string; email: string }>((res) => {
      resolveConsume = res;
    });

    const resolveDeferred = deferred<string>();
    vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockImplementation(() => resolveDeferred.promise);
    const consumeSpy = vi.spyOn(accountSessionModule, "consumeLogin").mockImplementation(async () => consumePromise);

    const onLoginSuccessInitial = vi.fn();
    const onLoginSuccessUpdated = vi.fn();

    const { rerender } = render(
      <StrictMode>
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccessInitial}
        />
      </StrictMode>
    );

    // StrictMode runs effect twice synchronously or within microtask.
    // Resolve origin so consumeLogin can be called.
    await act(async () => {
      resolveDeferred.resolve(relayUrl);
    });

    // Consume should only be initiated once despite StrictMode effect replay
    expect(consumeSpy).toHaveBeenCalledTimes(1);
    expect(consumeSpy).toHaveBeenCalledWith(relayUrl, hexCode);

    // Rerender with new callback while consumeLogin promise is still pending
    rerender(
      <StrictMode>
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccessUpdated}
        />
      </StrictMode>
    );

    // Now resolve the pending consume promise
    await act(async () => {
      resolveConsume({
        token: "jwt-session-token-strict",
        accountId: "acc-strict",
        email: "strict@example.com",
      });
    });

    await waitFor(() => {
      expect(consumeSpy).toHaveBeenCalledTimes(1);
      expect(onLoginSuccessInitial).not.toHaveBeenCalled();
      expect(onLoginSuccessUpdated).toHaveBeenCalledTimes(1);
      expect(onLoginSuccessUpdated).toHaveBeenCalledWith("jwt-session-token-strict", "strict@example.com");
    });
  });

  it("does not render the legacy device PIN pairing button", () => {
    const onLoginSuccess = vi.fn();
    const { queryByTestId } = render(
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={onLoginSuccess}
      />
    );
    expect(queryByTestId("use-legacy-pin-btn")).toBeNull();
  });

  describe("magic link auto sign-in polling", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      cleanup();
      vi.useRealTimers();
    });

    it("CASE C: auto signs in when magic link is approved and stops polling", async () => {
      const loginHandle = "c".repeat(64);
      const resolveSpy = vi
        .spyOn(accountSessionModule, "resolveAccountOrigin")
        .mockResolvedValue(relayUrl);
      const requestSpy = vi
        .spyOn(accountSessionModule, "requestLogin")
        .mockResolvedValue({ loginHandle });
      const pollSpy = vi
        .spyOn(accountSessionModule, "pollLogin")
        .mockResolvedValueOnce({ status: "pending" })
        .mockResolvedValueOnce({
          status: "approved",
          token: "tok-remote-1",
          email: "user@example.com",
        });
      const storeSpy = vi
        .spyOn(accountSessionModule, "storeAccountSessionToken")
        .mockImplementation(() => {});
      const onLoginSuccess = vi.fn();

      render(
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccess}
        />
      );

      fireEvent.change(screen.getByTestId("account-email-input"), {
        target: { value: "user@example.com" },
      });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });

      expect(resolveSpy).toHaveBeenCalledWith(relayUrl);
      expect(requestSpy).toHaveBeenCalledWith(relayUrl, "user@example.com");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(1);
      expect(pollSpy).toHaveBeenCalledWith(relayUrl, loginHandle);
      expect(storeSpy).not.toHaveBeenCalled();
      expect(onLoginSuccess).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(2);
      expect(storeSpy).toHaveBeenCalledWith("tok-remote-1", relayUrl);
      expect(onLoginSuccess).toHaveBeenCalledWith("tok-remote-1", "user@example.com");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(2);
    });

    it("CASE C: stops polling and displays expiry error when TTL expires", async () => {
      const loginHandle = "e".repeat(64);
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle });
      const pollSpy = vi
        .spyOn(accountSessionModule, "pollLogin")
        .mockResolvedValue({ status: "pending" });
      const storeSpy = vi
        .spyOn(accountSessionModule, "storeAccountSessionToken")
        .mockImplementation(() => {});
      const onLoginSuccess = vi.fn();

      render(
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccess}
        />
      );

      fireEvent.change(screen.getByTestId("account-email-input"), {
        target: { value: "user-expire@example.com" },
      });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
      });

      const errorBanner = screen.getByTestId("account-login-error");
      expect(errorBanner).not.toBeNull();
      expect(errorBanner.textContent).toContain("This sign-in link expired. Send a new one.");

      const pollCallsBefore = pollSpy.mock.calls.length;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(pollCallsBefore);
      expect(storeSpy).not.toHaveBeenCalled();
      expect(onLoginSuccess).not.toHaveBeenCalled();
    });

    it("CASE D: cleans up polling interval on unmount without leaking timer", async () => {
      const loginHandle = "f".repeat(64);
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle });
      const pollSpy = vi
        .spyOn(accountSessionModule, "pollLogin")
        .mockResolvedValue({ status: "pending" });
      const onLoginSuccess = vi.fn();

      const { unmount } = render(
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccess}
        />
      );

      fireEvent.change(screen.getByTestId("account-email-input"), {
        target: { value: "user-unmount@example.com" },
      });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(1);

      act(() => {
        unmount();
      });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(1);
    });

    it("keeps account-code-input absent by default after requestLogin resolves pending and polling still logs in", async () => {
      const loginHandle = "a".repeat(64);
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle });
      const pollSpy = vi
        .spyOn(accountSessionModule, "pollLogin")
        .mockResolvedValueOnce({ status: "pending" })
        .mockResolvedValueOnce({
          status: "approved",
          token: "tok-magic-auto",
          email: "magic-user@example.com",
        });
      const storeSpy = vi
        .spyOn(accountSessionModule, "storeAccountSessionToken")
        .mockImplementation(() => {});
      const onLoginSuccess = vi.fn();

      render(
        <AccountLoginPage
          relayUrl={relayUrl}
          onLoginSuccess={onLoginSuccess}
        />
      );

      fireEvent.change(screen.getByTestId("account-email-input"), {
        target: { value: "magic-user@example.com" },
      });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });

      // Default state: account-code-input must be absent (not misleadingly required by default)
      expect(screen.queryByTestId("account-code-input")).toBeNull();

      // Polling continues in background and automatically signs in
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(1);
      expect(onLoginSuccess).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(pollSpy).toHaveBeenCalledTimes(2);
      expect(storeSpy).toHaveBeenCalledWith("tok-magic-auto", relayUrl);
      expect(onLoginSuccess).toHaveBeenCalledWith("tok-magic-auto", "magic-user@example.com");
    });
  });
  describe("user-facing copy", () => {
    it("never shows a raw network message when the link request fails", async () => {
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      vi.spyOn(accountSessionModule, "requestLogin").mockRejectedValue(new TypeError("Failed to fetch"));
      render(<AccountLoginPage relayUrl={relayUrl} onLoginSuccess={vi.fn()} />);

      fireEvent.change(screen.getByTestId("account-email-input"), { target: { value: "a@b.c" } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });

      const banner = await screen.findByTestId("account-login-error");
      expect(banner.textContent).toContain("Couldn't reach Ferryx");
      expect(banner.textContent).not.toContain("Failed to fetch");
    });

    it("maps a used sign-in link by its code instead of showing the code", async () => {
      delete (window as any).location;
      window.location = { ...originalLocation, search: "?code=deadbeef", hash: "", pathname: "/" } as any;
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      vi.spyOn(accountSessionModule, "consumeLogin").mockRejectedValue(
        new accountSessionModule.AccountSessionError("LOGIN_CODE_USED", "LOGIN_CODE_USED", 400),
      );
      render(<AccountLoginPage relayUrl={relayUrl} onLoginSuccess={vi.fn()} />);

      const banner = await screen.findByTestId("account-login-error");
      expect(banner.textContent).toContain("already used");
      expect(banner.textContent).not.toContain("LOGIN_CODE_USED");
    });

    it("enables resending the link after 30 seconds of waiting", async () => {
      vi.useFakeTimers();
      vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
      const requestSpy = vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle: "h".repeat(64) });
      vi.spyOn(accountSessionModule, "pollLogin").mockResolvedValue({ status: "pending" });
      render(<AccountLoginPage relayUrl={relayUrl} onLoginSuccess={vi.fn()} />);

      fireEvent.change(screen.getByTestId("account-email-input"), { target: { value: "a@b.c" } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("request-magic-link-btn"));
      });
      const resend = screen.getByTestId("resend-magic-link-btn");
      expect(resend).toBeDisabled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(30_000);
      });
      expect(resend).not.toBeDisabled();
      await act(async () => {
        fireEvent.click(resend);
      });
      expect(requestSpy).toHaveBeenCalledTimes(2);
    });
  });
});
