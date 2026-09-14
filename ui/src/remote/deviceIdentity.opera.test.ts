import { createElement } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { REMOTE_INSTALLATION_ID_STORAGE_KEY } from "../lib/storageKeys";
import { PairingPage } from "./PairingPage";
import { suggestDeviceName } from "./deviceIdentity";

const chromeUa = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const operaUa = `${chromeUa} OPR/114.0.0.0`;
const browsers = [
  { browser: "Opera", ua: operaUa, expected: "Windows - Opera" },
  { browser: "Edge", ua: `${chromeUa} Edg/128.0.0.0`, expected: "Windows - Edge" },
  { browser: "Chrome", ua: chromeUa, expected: "Windows - Chrome" },
] as const;

function deferred<T>() {
  let resolve: (value: T) => void = () => { throw new Error("Signal not initialized"); };
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}

async function completion(signal: Promise<void>) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      signal,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error("Pairing signal deadline exceeded")), 1000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

describe("Windows Opera device suggestion", () => {
  it.each(browsers)("identifies $browser when its Windows UA contains compatibility tokens", ({ ua, expected }) => {
    // Given: an explicit Windows browser UA, independent of the test host.
    // When: the real parser derives a suggestion.
    const suggestion = suggestDeviceName(ua);
    // Then: the browser-specific identity wins over compatibility markers.
    expect(suggestion).toBe(expected);
  });

  it.each([
    ...browsers.map((browser) => ({ ...browser, edit: undefined, scenario: "unedited" })),
    { browser: "Opera", ua: operaUa, expected: "My Opera workstation", edit: "My Opera workstation", scenario: "edited" },
    { browser: "Opera", ua: operaUa, expected: "Windows - Opera", edit: "   ", scenario: "blank fallback" },
  ])("submits $expected when $browser pairing uses the $scenario name", async ({ browser, ua, expected, edit }) => {
    // Given: real pairing UI/storage logic, with only the network boundary controlled.
    const previousId = localStorage.getItem(REMOTE_INSTALLATION_ID_STORAGE_KEY);
    const response = deferred<Response>();
    const requested = deferred<void>();
    const paired = deferred<void>();
    const onPaired = vi.fn(() => paired.resolve());
    const fetchMock = vi.fn<typeof fetch>(async () => {
      requested.resolve();
      return response.promise;
    });
    try {
      localStorage.setItem(REMOTE_INSTALLATION_ID_STORAGE_KEY, "p28-owned-installation");
      vi.spyOn(navigator, "userAgent", "get").mockReturnValue(ua);
      vi.stubGlobal("fetch", fetchMock);
      render(createElement(PairingPage, { onPaired }));
      const nameInput = screen.getByPlaceholderText("Device name");
      // Soft assertion lets RED still reach and assert the actual submitted field.
      expect.soft(nameInput).toHaveValue(`Windows - ${browser}`);
      if (edit !== undefined) fireEvent.change(nameInput, { target: { value: edit } });
      fireEvent.change(screen.getByPlaceholderText("6-digit PIN"), { target: { value: "654321" } });

      // When: submit through the real form, then release the exact response.
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Connect" }));
        await completion(requested.promise);
      });
      expect(onPaired).not.toHaveBeenCalled();
      expect(screen.getByRole("button", { name: "Connecting..." })).toBeDisabled();
      await act(async () => {
        response.resolve(new Response(JSON.stringify({
          token: "p28-token", machineId: "p28-host", displayName: "P28 Host",
        }), { status: 200, headers: { "Content-Type": "application/json" } }));
        await completion(paired.promise);
      });

      // Then: real JSON response processing completes and the request retains identity/edit.
      expect(onPaired).toHaveBeenCalledExactlyOnceWith("p28-token", {
        machineId: "p28-host", displayName: "P28 Host",
      });
      expect(fetchMock).toHaveBeenCalledExactlyOnceWith("/api/v1/pair/exchange", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: "654321", deviceName: expected, installationId: "p28-owned-installation" }),
      });
      if (edit === undefined) expect(nameInput).toHaveValue(expected);
    } finally {
      cleanup();
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
      if (previousId === null) localStorage.removeItem(REMOTE_INSTALLATION_ID_STORAGE_KEY);
      else localStorage.setItem(REMOTE_INSTALLATION_ID_STORAGE_KEY, previousId);
    }
  }, 5000);
});
