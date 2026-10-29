import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { remoteHostStore } from "../state/remoteHostStore";
import { clearStoredAccountSessionToken } from "./accountSession";

/** The gateway's structured pair-exchange refusal (src-tauri/src/remote/server.rs pairing_failure). */
function refusal(status: number, code: string, remainingAttempts: number): Response {
  return new Response(JSON.stringify({ code, message: "ignored by the client", details: { remainingAttempts } }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

async function submitCode(code: string): Promise<void> {
  fireEvent.change(screen.getByPlaceholderText("6-digit PIN"), { target: { value: code } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  });
}

describe("RemoteApp pairing entry", () => {
  beforeEach(() => {
    remoteHostStore.reset();
    localStorage.clear();
    clearStoredAccountSessionToken();
  });

  afterEach(() => {
    cleanup();
    remoteHostStore.reset();
    localStorage.clear();
    window.history.replaceState(null, "", "/");
    vi.unstubAllGlobals();
  });

  it("a signed-out phone opening /pair reaches the pairing page, not account sign-in", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>());
    await act(async () => { render(<RemoteApp />); });

    expect(screen.getByRole("heading", { name: "Pair Device" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Sign In to Ferryx/i })).toBeNull();
  });

  it("a wrong code shows the attempts left from the gateway's structured refusal", async () => {
    window.history.replaceState(null, "", "/pair");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(refusal(400, "INVALID_PIN", 3));
    vi.stubGlobal("fetch", fetchMock);
    await act(async () => { render(<RemoteApp />); });

    await submitCode("000000");

    expect(fetchMock).toHaveBeenCalledWith("/api/v1/pair/exchange", expect.objectContaining({ method: "POST" }));
    expect(await screen.findByText("Wrong code. 3 attempts left.")).toBeInTheDocument();
  });

  it("an expired code shows the expired text from the PIN_EXPIRED code", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(refusal(401, "PIN_EXPIRED", 4)));
    await act(async () => { render(<RemoteApp />); });

    await submitCode("123456");

    expect(await screen.findByText("Code expired. Ask for a new code in Ferryx Desktop settings.")).toBeInTheDocument();
    expect(screen.queryByText(/attempts? left/)).toBeNull();
  });

  it("a successful exchange stores the device token and leaves the pairing route", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async (input) => {
      if (String(input).includes("/api/v1/pair/exchange")) {
        return Response.json({ token: "paired-token", machineId: "m-1", displayName: "Desk" });
      }
      return new Response("{}", { status: 404 });
    }));
    await act(async () => { render(<RemoteApp />); });

    await submitCode("482913");

    expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBe("paired-token");
    expect(window.location.pathname).toBe("/");
    expect(screen.queryByRole("heading", { name: "Pair Device" })).toBeNull();
  });

  it("without a pairing entry a signed-out phone still sees account sign-in", async () => {
    window.history.replaceState(null, "", "/");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });

    expect(screen.getByRole("heading", { name: /Sign In to Ferryx/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Pair Device" })).toBeNull();
  });
});
