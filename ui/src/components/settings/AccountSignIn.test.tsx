import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AccountSignIn } from "./AccountSignIn";
import * as accountSessionModule from "../../remote/accountSession";
import { AccountSessionError } from "../../remote/accountSession";

describe("AccountSignIn - magic link auto sign-in polling", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("CASE A: requests magic link, displays info message, polls until approved, and stores token", async () => {
    const loginHandle = "a".repeat(64);
    const requestSpy = vi
      .spyOn(accountSessionModule, "requestLogin")
      .mockResolvedValue({ loginHandle });
    const pollSpy = vi
      .spyOn(accountSessionModule, "pollLogin")
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({
        status: "approved",
        token: "tok-1",
        email: "user@example.com",
      });
    const storeSpy = vi
      .spyOn(accountSessionModule, "storeAccountSessionToken")
      .mockImplementation(() => {});
    const onSignIn = vi.fn();

    render(<AccountSignIn onSignIn={onSignIn} />);

    fireEvent.change(screen.getByLabelText("Email Address"), {
      target: { value: "user@example.com" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send Magic Link" }));
    });

    expect(requestSpy).toHaveBeenCalledWith(window.location.origin, "user@example.com");

    const infoMessage = screen.getByTestId("account-signin-info");
    expect(infoMessage).not.toBeNull();
    expect(infoMessage.textContent).toContain("will sign in automatically");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(1);
    expect(pollSpy).toHaveBeenCalledWith(window.location.origin, loginHandle);
    expect(storeSpy).not.toHaveBeenCalled();
    expect(onSignIn).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(2);
    expect(storeSpy).toHaveBeenCalledWith("tok-1", window.location.origin);
    expect(onSignIn).toHaveBeenCalledWith("tok-1", "user@example.com");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(2);
  });

  it("CASE B (transient): transient failure keeps polling and completes on subsequent approval", async () => {
    const loginHandle = "b".repeat(64);
    vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle });
    const pollSpy = vi
      .spyOn(accountSessionModule, "pollLogin")
      .mockRejectedValueOnce(new TypeError("Load failed"))
      .mockResolvedValueOnce({
        status: "approved",
        token: "tok-2",
        email: "user2@example.com",
      });
    const storeSpy = vi
      .spyOn(accountSessionModule, "storeAccountSessionToken")
      .mockImplementation(() => {});
    const onSignIn = vi.fn();

    render(<AccountSignIn onSignIn={onSignIn} />);

    fireEvent.change(screen.getByLabelText("Email Address"), {
      target: { value: "user2@example.com" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send Magic Link" }));
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("account-signin-error")).toBeNull();
    expect(screen.getByTestId("account-signin-info")).not.toBeNull();
    expect(storeSpy).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(2);
    expect(storeSpy).toHaveBeenCalledWith("tok-2", window.location.origin);
    expect(onSignIn).toHaveBeenCalledWith("tok-2", "user2@example.com");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(2);
  });

  it("CASE B (terminal): terminal failure stops polling and displays error banner", async () => {
    const loginHandle = "c".repeat(64);
    vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle });
    const terminalError = new AccountSessionError(
      "LOGIN_HANDLE_INVALID",
      "Invalid or expired login handle",
      401,
    );
    const pollSpy = vi
      .spyOn(accountSessionModule, "pollLogin")
      .mockRejectedValue(terminalError);
    const storeSpy = vi
      .spyOn(accountSessionModule, "storeAccountSessionToken")
      .mockImplementation(() => {});
    const onSignIn = vi.fn();

    render(<AccountSignIn onSignIn={onSignIn} />);

    fireEvent.change(screen.getByLabelText("Email Address"), {
      target: { value: "user3@example.com" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send Magic Link" }));
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(1);

    const errorBanner = screen.getByTestId("account-signin-error");
    expect(errorBanner).not.toBeNull();
    expect(errorBanner.textContent).toContain("Invalid or expired login handle");
    expect(errorBanner.getAttribute("data-code")).toBe("LOGIN_HANDLE_INVALID");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(pollSpy).toHaveBeenCalledTimes(1);
    expect(storeSpy).not.toHaveBeenCalled();
    expect(onSignIn).not.toHaveBeenCalled();
  });

  it("an invalid configured origin surfaces INVALID_ACCOUNT_ORIGIN instead of the hosted relay", async () => {
    window.localStorage.setItem("ferryx.account.origin", "http://evil.example");
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    try {
      render(<AccountSignIn />);
      fireEvent.change(screen.getByLabelText("Email Address"), {
        target: { value: "user4@example.com" },
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Send Magic Link" }));
      });

      const errorBanner = screen.getByTestId("account-signin-error");
      expect(errorBanner.getAttribute("data-code")).toBe("INVALID_ACCOUNT_ORIGIN");
      expect(errorBanner.textContent).toContain("http://evil.example");
      expect(fetchSpy).not.toHaveBeenCalled();
    } finally {
      window.localStorage.removeItem("ferryx.account.origin");
      vi.unstubAllGlobals();
    }
  });
});
