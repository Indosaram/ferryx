import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { remoteHostKey, remoteHostStore } from "../state/remoteHostStore";
import { clearStoredAccountSessionToken } from "./accountSession";

/** The gateway's structured pair-exchange refusal (src-tauri/src/remote/server.rs pairing_failure). */
function refusal(status: number, code: string, remainingAttempts: number): Response {
  return new Response(JSON.stringify({ code, message: "ignored by the client", details: { remainingAttempts } }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

async function submitCode(code: string): Promise<void> {
  fireEvent.change(screen.getByLabelText("Pairing code"), { target: { value: code } });
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

    expect(screen.getByRole("heading", { name: "Pair this phone" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Sign in to Ferryx/i })).toBeNull();
  });

  it("a wrong code shows the attempts left from the gateway's structured refusal", async () => {
    window.history.replaceState(null, "", "/pair");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(refusal(400, "INVALID_PIN", 3));
    vi.stubGlobal("fetch", fetchMock);
    await act(async () => { render(<RemoteApp />); });

    await submitCode("000000");

    expect(fetchMock).toHaveBeenCalledWith("/api/v1/pair/exchange", expect.objectContaining({ method: "POST" }));
    expect(await screen.findByText("Wrong code. 3 attempts left.")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Wrong code. 3 attempts left.");
  });

  it("an expired code shows the expired text from the PIN_EXPIRED code", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(refusal(401, "PIN_EXPIRED", 4)));
    await act(async () => { render(<RemoteApp />); });

    await submitCode("123456");

    expect(await screen.findByText("This code expired. Get a new code from Ferryx on your computer.")).toBeInTheDocument();
    expect(screen.queryByText(/attempts? left/)).toBeNull();
  });

  it("INVALID_PIN on a non-400 status still shows attempts left: the code decides, not the status", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(refusal(401, "INVALID_PIN", 2)));
    await act(async () => { render(<RemoteApp />); });

    await submitCode("000000");

    expect(await screen.findByText("Wrong code. 2 attempts left.")).toBeInTheDocument();
  });

  it("a bare 401 without PIN_EXPIRED shows the plain generic failure without the status code", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("", { status: 401 })));
    await act(async () => { render(<RemoteApp />); });

    await submitCode("000000");

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("That code didn't work.");
    expect(alert.textContent).not.toMatch(/401/);
    expect(screen.queryByText(/code expired/)).toBeNull();
  });

  it("relay=1 stores the token on the relay host keyed by machine id, not the page-origin slot", async () => {
    window.history.replaceState(null, "", "/pair?relay=1");
    const fetchMock = vi.fn<typeof fetch>(async (input) => {
      if (String(input).includes("/api/v1/pair/exchange")) {
        return Response.json({ token: "relay-token", machineId: "m-9", displayName: "Desk" });
      }
      return new Response("{}", { status: 404 });
    });
    vi.stubGlobal("fetch", fetchMock);
    await act(async () => { render(<RemoteApp />); });

    await submitCode("482913");

    const origin = window.location.origin;
    const key = remoteHostKey(origin, "m-9");
    const state = remoteHostStore.getState();
    expect(fetchMock).toHaveBeenCalledWith("/api/v1/pair/exchange", expect.objectContaining({ method: "POST" }));
    expect(state.activeHostId).toBe(key);
    expect(state.hosts[key]?.deviceToken).toBe("relay-token");
    expect(localStorage.getItem(`ferryx_remote_token_local:${origin}`)).toBeNull();
    expect(window.location.pathname).toBe("/");
    expect(window.location.search).toBe("");
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
    expect(screen.queryByRole("heading", { name: "Pair this phone" })).toBeNull();
  });

  it("the pairing code field is a labelled numeric one-time-code input at 16px", async () => {
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>());
    await act(async () => { render(<RemoteApp />); });

    const input = screen.getByLabelText("Pairing code");
    expect(input).toHaveAttribute("inputmode", "numeric");
    expect(input).toHaveAttribute("autocomplete", "one-time-code");
    expect(input).toHaveClass("text-base");
  });

  it("sign-in offers pairing with a code and the pairing page offers sign-in back", async () => {
    window.history.replaceState(null, "", "/");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pair with a code instead" })); });
    expect(screen.getByRole("heading", { name: "Pair this phone" })).toBeInTheDocument();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Sign in with email instead" })); });
    expect(screen.getByRole("heading", { name: /Sign in to Ferryx/i })).toBeInTheDocument();
  });

  it("without a pairing entry a signed-out phone still sees account sign-in", async () => {
    window.history.replaceState(null, "", "/");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });

    expect(screen.getByRole("heading", { name: /Sign in to Ferryx/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Pair this phone" })).toBeNull();
  });
});
