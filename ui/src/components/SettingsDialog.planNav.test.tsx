import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_RELAY_ORIGIN } from "../lib/pairedHostInventory";
import { storeAccountSessionToken } from "../remote/accountSession";
import { SettingsDialog } from "./SettingsDialog";
import { observeDom, settle, settledTestId } from "../test/domSignals";

const native = vi.hoisted(() => ({
  getTerminalPreferences: vi.fn(),
}));

vi.mock(import("../lib/tauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getTerminalPreferences: native.getTerminalPreferences,
  };
});

vi.mock("./settings/GeneralSection", () => ({
  GeneralSection: () => <div data-testid="general-section" />,
}));

vi.mock("./settings/RemoteSection", () => ({
  RemoteSection: ({
    onAccountSessionChange,
  }: {
    onAccountSessionChange?: (token: string | null, origin: string) => void;
  }) => (
    <div data-testid="remote-section-stub">
      <button
        type="button"
        onClick={() => onAccountSessionChange?.("session-token-1", "https://origins.example.test")}
      >
        Stub sign in
      </button>
      <button
        type="button"
        onClick={() => onAccountSessionChange?.(null, "https://origins.example.test")}
      >
        Stub sign out
      </button>
    </div>
  ),
}));

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

const ENTITLEMENT = {
  plan: "free",
  status: "ok",
  machineLimit: 1,
  machinesUsed: 0,
  seats: null,
  hostPacks: 0,
  graceEndsAt: null,
  orgId: null,
  role: null,
  manageUrl: null,
};

beforeEach(() => {
  localStorage.clear();
  native.getTerminalPreferences.mockReset();
  native.getTerminalPreferences.mockResolvedValue({
    fontFamily: "Noto Sans KR",
    macosOptionAsAlt: true,
    source: "ghostty",
    status: "imported",
    sourcePath: "/Users/test/.config/ghostty/config",
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("SettingsDialog plan navigation", () => {
  it("does not offer the Plan section while signed out", () => {
    render(<SettingsDialog open onClose={vi.fn()} initialSection="shortcuts" />);

    expect(screen.queryByRole("button", { name: "Plan" })).toBeNull();
  });

  it("offers the Plan section for a stored account session and mounts it on demand", async () => {
    storeAccountSessionToken("session-token-1", DEFAULT_RELAY_ORIGIN);
    vi.stubGlobal("fetch", async () => jsonResponse(ENTITLEMENT));

    render(<SettingsDialog open onClose={vi.fn()} initialSection="plan" />);

    expect(screen.getByRole("button", { name: "Plan" })).toHaveAttribute("aria-current", "page");
    expect(await settledTestId("plan-summary")).toHaveAttribute("data-plan", "free");
  });

  it("shows the Plan section when the sign-in happens inside the same open dialog", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      const url =
        typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
      calls.push(url);
      return jsonResponse(ENTITLEMENT);
    });

    render(<SettingsDialog open onClose={vi.fn()} initialSection="remote" />);

    expect(screen.queryByRole("button", { name: "Plan" })).toBeNull();

    const planNavSignal = observeDom(() => screen.queryByRole("button", { name: "Plan" }));
    fireEvent.click(screen.getByRole("button", { name: "Stub sign in" }));

    const planNav = await settle(planNavSignal);
    fireEvent.click(planNav);

    expect(await settledTestId("plan-summary")).toHaveAttribute("data-plan", "free");
    expect(calls).toContain(
      "https://origins.example.test/api/account/v1/billing/entitlement",
    );
  });

  it("drops the Plan navigation and falls back to General when the server answers 404", async () => {
    storeAccountSessionToken("session-token-1", DEFAULT_RELAY_ORIGIN);
    vi.stubGlobal("fetch", async () => jsonResponse({ code: "NOT_FOUND", message: "not found" }, 404));

    const fallbackSignal = settledTestId("general-section");
    render(<SettingsDialog open onClose={vi.fn()} initialSection="plan" />);

    expect(await fallbackSignal).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Plan" })).toBeNull();
    expect(screen.queryByTestId("plan-summary")).toBeNull();
  });
});
