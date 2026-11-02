import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserSignInHandoffBar } from "./BrowserSignInHandoffBar";
import * as browserTauri from "../lib/browserTauri";
import { BROWSER_ENGINE_CHANGED_EVENT } from "../lib/browserExtensions";
import type { BrowserTab } from "../lib/types";

const events = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => events);

vi.mock("../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/browserTauri")>()),
  getSignInHandoffPlan: vi.fn(),
  installBrowserEngine: vi.fn(),
  runSignInAuthSession: vi.fn(),
  openExternalUrl: vi.fn(async () => undefined),
}));

const getPlan = vi.mocked(browserTauri.getSignInHandoffPlan);
const install = vi.mocked(browserTauri.installBrowserEngine);
const authSession = vi.mocked(browserTauri.runSignInAuthSession);
const openExternal = vi.mocked(browserTauri.openExternalUrl);

const BLOCKED_URL = "https://accounts.google.com/v3/signin/rejected?continue=https%3A%2F%2Fmail.google.com%2F";
const RETRY_URL = "https://mail.google.com/";
const AUTH_SESSION: Extract<browserTauri.HandoffAction, { kind: "systemAuthSession" }> = {
  kind: "systemAuthSession",
  authorizeUrl: "https://accounts.google.com/o/oauth2/v2/auth?client_id=c&redirect_uri=com.example%3A%2Fcb&response_type=code",
  callback: { kind: "scheme", scheme: "com.example" },
};

function tab(overrides: Partial<BrowserTab> = {}): BrowserTab {
  return {
    kind: "browser",
    id: "tab-1",
    label: "Sign in",
    browserId: "browser-1",
    url: BLOCKED_URL,
    title: "Couldn't sign you in",
    engine: "system",
    profileId: "work",
    ...overrides,
  };
}

function plan(overrides: Partial<browserTauri.HandoffPlan> = {}): browserTauri.HandoffPlan {
  return {
    provider: "google",
    engine: "ready",
    primary: { kind: "chromiumTab", url: RETRY_URL },
    alternatives: [],
    onDecline: [{ kind: "externalBrowser", url: RETRY_URL }],
    ...overrides,
  };
}

function nextHandoff(): Promise<browserTauri.SignInHandoffDetail> {
  return new Promise((resolve) => {
    window.addEventListener(
      browserTauri.SIGN_IN_HANDOFF_EVENT,
      (event) => resolve((event as CustomEvent<browserTauri.SignInHandoffDetail>).detail),
      { once: true },
    );
  });
}

function bar(): HTMLElement | null {
  return screen.queryByTestId("browser-sign-in-handoff");
}

beforeEach(() => {
  events.listen.mockResolvedValue(() => undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("BrowserSignInHandoffBar", () => {
  it("hands a blocked sign-in to a Chromium tab of the same profile", async () => {
    getPlan.mockResolvedValue(plan());
    const received = nextHandoff();
    render(<BrowserSignInHandoffBar tab={tab()} />);

    expect(await screen.findByText("Google does not allow sign-in in this browser engine.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Chromium engine" }));

    const detail = await received;
    expect(detail).toEqual({
      url: RETRY_URL,
      profileId: "work",
      browserId: expect.stringMatching(/^signin:/),
      idpHost: "accounts.google.com",
    });
    expect(getPlan).toHaveBeenCalledWith("browser-1", BLOCKED_URL, "Couldn't sign you in");
    expect(bar()).toBeNull();
  });

  it("installs the Chromium engine with progress, then hands off", async () => {
    getPlan.mockResolvedValue(plan({ engine: "notInstalled", primary: { kind: "installEngine", url: RETRY_URL } }));
    let finish: (status: browserTauri.BrowserEngineStatus) => void = () => undefined;
    install.mockReturnValue(new Promise<browserTauri.BrowserEngineStatus>((resolve) => { finish = resolve; }));
    let emit: (payload: { receivedBytes: number; totalBytes: number | null }) => void = () => undefined;
    events.listen.mockImplementation(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      emit = (payload) => handler({ payload });
      return () => undefined;
    });
    const engineChanged = vi.fn();
    window.addEventListener(BROWSER_ENGINE_CHANGED_EVENT, engineChanged);
    const received = nextHandoff();
    render(<BrowserSignInHandoffBar tab={tab()} />);

    expect(screen.queryByText(/Settings > Browser/)).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "Install Chromium engine and sign in" }));

    expect(await screen.findByText("Installing Chromium engine...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install Chromium engine and sign in" })).toBeDisabled();
    await waitFor(() => expect(events.listen).toHaveBeenCalledWith("browser_engine_install_progress", expect.any(Function)));
    act(() => emit({ receivedBytes: 50, totalBytes: 200 }));
    expect(screen.getByText("Installing Chromium engine... 25%")).toBeInTheDocument();

    await act(async () => finish({ state: "installed", version: "1.0" }));
    const detail = await received;
    expect(detail.url).toBe(RETRY_URL);
    expect(detail.profileId).toBe("work");
    expect(detail.browserId).toMatch(/^signin:/);
    expect(engineChanged).toHaveBeenCalledOnce();
    window.removeEventListener(BROWSER_ENGINE_CHANGED_EVENT, engineChanged);
  });

  it("promotes the macOS sign-in when the engine install fails", async () => {
    getPlan.mockResolvedValue(
      plan({ engine: "notInstalled", primary: { kind: "installEngine", url: RETRY_URL }, alternatives: [AUTH_SESSION] }),
    );
    install.mockRejectedValue({ code: "BROWSER_ENGINE_INSTALL_FAILED", message: "disk full", details: { reason: "diskFull" } });
    const handoff = vi.fn();
    window.addEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
    render(<BrowserSignInHandoffBar tab={tab()} />);

    expect(await screen.findByRole("button", { name: "Sign in with macOS" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install Chromium engine and sign in" }));

    expect(await screen.findByText("disk full")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install Chromium engine and sign in" })).toBeNull();
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with macOS" })).toBeEnabled());
    expect(handoff).not.toHaveBeenCalled();
    window.removeEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
  });

  it("hides the bar when the macOS sign-in completes", async () => {
    getPlan.mockResolvedValue(plan({ engine: "unsupported", primary: AUTH_SESSION }));
    authSession.mockResolvedValue({ outcome: "completed", callbackUrl: "https://mail.google.com/cb?code=1" });
    render(<BrowserSignInHandoffBar tab={tab()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sign in with macOS" }));

    await waitFor(() => expect(bar()).toBeNull());
    expect(authSession).toHaveBeenCalledWith("browser-1", AUTH_SESSION.authorizeUrl, AUTH_SESSION.callback);
  });

  it("keeps the bar when the macOS sign-in is cancelled", async () => {
    getPlan.mockResolvedValue(plan({ engine: "unsupported", primary: AUTH_SESSION }));
    let finish: (result: browserTauri.AuthSessionResult) => void = () => undefined;
    authSession.mockReturnValue(new Promise<browserTauri.AuthSessionResult>((resolve) => { finish = resolve; }));
    render(<BrowserSignInHandoffBar tab={tab()} />);

    const button = await screen.findByRole("button", { name: "Sign in with macOS" });
    fireEvent.click(button);
    expect(button).toBeDisabled();

    await act(async () => finish({ outcome: "cancelled" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with macOS" })).toBeEnabled());
    expect(authSession).toHaveBeenCalledOnce();
    expect(bar()).not.toBeNull();
  });

  it("offers the default browser only after Not now", async () => {
    getPlan.mockResolvedValue(plan());
    render(<BrowserSignInHandoffBar tab={tab()} />);

    await screen.findByRole("button", { name: "Sign in with Chromium engine" });
    expect(screen.queryByRole("button", { name: "Open in default browser" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Open in default browser" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(screen.getByText("Sign-in stays blocked in this tab.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in default browser" }));

    await waitFor(() => expect(openExternal).toHaveBeenCalledWith(RETRY_URL));
    await waitFor(() => expect(bar()).toBeNull());
  });

  it("dismiss leaves the tab unchanged and requests no handoff", async () => {
    getPlan.mockResolvedValue(plan());
    const handoff = vi.fn();
    window.addEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
    render(<BrowserSignInHandoffBar tab={tab()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Not now" }));
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Dismiss" }));

    expect(bar()).toBeNull();
    expect(handoff).not.toHaveBeenCalled();
    expect(openExternal).not.toHaveBeenCalled();
    expect(authSession).not.toHaveBeenCalled();
    window.removeEventListener(browserTauri.SIGN_IN_HANDOFF_EVENT, handoff);
  });

  it("shows the default browser directly when no in-app path exists", async () => {
    getPlan.mockResolvedValue(
      plan({ provider: "okta", engine: "unusable", primary: { kind: "externalBrowser", url: BLOCKED_URL }, onDecline: [] }),
    );
    render(<BrowserSignInHandoffBar tab={tab({ url: "https://acme.okta.com/login", title: "Unsupported browser" })} />);

    expect(await screen.findByText("Okta does not allow sign-in in this browser engine.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in default browser" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Not now" }));

    expect(bar()).toBeNull();
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("stays hidden for pages that are not blocked, other hosts and Chromium tabs", async () => {
    getPlan.mockResolvedValue(null);
    const { rerender } = render(<BrowserSignInHandoffBar tab={tab({ url: "https://accounts.google.com/v3/signin/identifier" })} />);
    await waitFor(() => expect(getPlan).toHaveBeenCalledOnce());
    expect(bar()).toBeNull();

    getPlan.mockResolvedValue(plan());
    rerender(<BrowserSignInHandoffBar tab={tab({ url: "https://example.com/" })} />);
    rerender(<BrowserSignInHandoffBar tab={tab({ url: "https://appleid.apple.com.evil.test/" })} />);
    rerender(<BrowserSignInHandoffBar tab={tab({ engine: "chromium" })} />);
    expect(getPlan).toHaveBeenCalledOnce();
    expect(bar()).toBeNull();
  });
});
