import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BrowserSignInHandoffBar } from "./BrowserSignInHandoffBar";
import * as browserTauri from "../lib/browserTauri";
import type { BrowserTab } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

vi.mock("../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/browserTauri")>()),
  detectSignInBlock: vi.fn(),
  getBrowserEngineStatus: vi.fn(),
  openExternalUrl: vi.fn(async () => undefined),
}));

const detect = vi.mocked(browserTauri.detectSignInBlock);
const engineStatus = vi.mocked(browserTauri.getBrowserEngineStatus);

const BLOCKED_URL = "https://accounts.google.com/v3/signin/rejected?continue=https%3A%2F%2Fmail.google.com%2F";

function tab(overrides: Partial<BrowserTab> = {}): BrowserTab {
  return {
    kind: "browser",
    id: "tab-1",
    label: "Sign in",
    browserId: "browser-1",
    url: BLOCKED_URL,
    engine: "system",
    profileId: "work",
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("BrowserSignInHandoffBar", () => {
  it("hands a blocked Google sign-in to a Chromium tab of the same profile", async () => {
    detect.mockResolvedValue({ kind: "googleBlocked", retryUrl: "https://mail.google.com/" });
    engineStatus.mockResolvedValue({ state: "installed", version: "1.0" });
    const received = new Promise<CustomEvent<browserTauri.SignInHandoffDetail>>((resolve) => {
      window.addEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, (event) => resolve(event as CustomEvent<browserTauri.SignInHandoffDetail>), { once: true });
    });
    render(<BrowserSignInHandoffBar tab={tab()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sign in with Chromium engine" }));

    const event = await received;
    expect(event.detail).toEqual({ url: "https://mail.google.com/", profileId: "work" });
    expect(screen.queryByTestId("browser-sign-in-handoff")).toBeNull();
  });

  it("dismiss leaves the tab unchanged and requests no handoff", async () => {
    detect.mockResolvedValue({ kind: "googleBlocked", retryUrl: "https://mail.google.com/" });
    engineStatus.mockResolvedValue({ state: "installed", version: "1.0" });
    const handoff = vi.fn();
    window.addEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
    render(<BrowserSignInHandoffBar tab={tab()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Dismiss" }));

    expect(screen.queryByTestId("browser-sign-in-handoff")).toBeNull();
    expect(handoff).not.toHaveBeenCalled();
    window.removeEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
  });

  it("offers the default browser when the Chromium engine is not installed", async () => {
    detect.mockResolvedValue({ kind: "googleBlocked", retryUrl: "https://mail.google.com/" });
    engineStatus.mockResolvedValue({ state: "notInstalled" });
    render(<BrowserSignInHandoffBar tab={tab()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Open in default browser" }));

    await waitFor(() => expect(browserTauri.openExternalUrl).toHaveBeenCalledWith("https://mail.google.com/"));
  });

  it("stays hidden for pages Google did not block, other hosts and Chromium tabs", async () => {
    detect.mockResolvedValue({ kind: "notBlocked" });
    engineStatus.mockResolvedValue({ state: "installed", version: "1.0" });
    const { rerender } = render(<BrowserSignInHandoffBar tab={tab({ url: "https://accounts.google.com/v3/signin/identifier" })} />);
    await waitFor(() => expect(detect).toHaveBeenCalledOnce());
    expect(screen.queryByTestId("browser-sign-in-handoff")).toBeNull();

    rerender(<BrowserSignInHandoffBar tab={tab({ url: "https://example.com/" })} />);
    rerender(<BrowserSignInHandoffBar tab={tab({ engine: "chromium" })} />);
    expect(detect).toHaveBeenCalledOnce();
    expect(screen.queryByTestId("browser-sign-in-handoff")).toBeNull();
  });
});
