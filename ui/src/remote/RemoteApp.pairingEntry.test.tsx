import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";
import { isDefaultRelayOrigin, RELAY_PAIRING_INCOMPLETE, servesAccountApi } from "./PairingPage";
import { DEFAULT_RELAY_ORIGIN } from "../lib/pairedHostInventory";
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

  it("a relay exchange that answers without a machine id reports an error instead of storing an unusable host", async () => {
    // Given a relay pairing page whose exchange succeeds without the host identity
    window.history.replaceState(null, "", "/pair?relay=1");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async (input) => {
      if (String(input).includes("/api/v1/pair/exchange")) return Response.json({ token: "orphan-token" });
      return new Response("{}", { status: 404 });
    }));
    await act(async () => { render(<RemoteApp />); });

    // When the person submits a valid-looking code
    await submitCode("482913");

    // Then the answer is reported, and no credential lands under a slot no host can use
    expect(await screen.findByText(RELAY_PAIRING_INCOMPLETE)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(RELAY_PAIRING_INCOMPLETE);
    expect(Object.keys(remoteHostStore.getState().hosts)).toEqual([]);
    expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBeNull();
    expect(window.location.pathname).toBe("/pair");
  });

  it("a relay exchange that omits the cosmetic display name still pairs under the machine id", async () => {
    // Given a relay exchange answering with a machine id but no display name
    window.history.replaceState(null, "", "/pair?relay=1");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async (input) => {
      if (String(input).includes("/api/v1/pair/exchange")) return Response.json({ token: "tok", machineId: "m-1" });
      return new Response("{}", { status: 404 });
    }));
    await act(async () => { render(<RemoteApp />); });

    // When the person submits a valid-looking code
    await submitCode("482913");

    // Then the host is stored with the machine id as its label
    await waitFor(() => expect(Object.values(remoteHostStore.getState().hosts)).toHaveLength(1));
    expect(Object.values(remoteHostStore.getState().hosts)[0]).toMatchObject({ machineId: "m-1", displayName: "m-1" });
    expect(screen.queryByText(RELAY_PAIRING_INCOMPLETE)).toBeNull();
  });

  it("going back from the pairing entry returns to sign-in", async () => {
    // Given a phone that opened the pairing entry
    window.history.replaceState(null, "", "/pair");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });
    expect(screen.getByRole("heading", { name: "Pair this phone" })).toBeInTheDocument();

    // When the browser goes back to the root
    await act(async () => {
      window.history.pushState(null, "", "/");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });

    // Then the entry is left, not still shown over the root the browser moved to
    expect(await screen.findByRole("heading", { name: /Sign in to Ferryx/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Pair this phone" })).toBeNull();
  });

  it("a relay page's pairing entry keeps the relay marker so a reload stays on the relay", async () => {
    // Given a relay page (the desktop's link marker is present) showing sign-in
    window.history.replaceState(null, "", "/?relay=1");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });

    // When pairing with a code is chosen
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pair with a code instead" })); });

    // Then the history entry keeps the marker that classifies the page
    expect(window.location.pathname).toBe("/pair");
    expect(window.location.search).toBe("?relay=1");
    expect(screen.getByRole("heading", { name: "Pair this phone" })).toBeInTheDocument();
  });

  it("a direct gateway's pairing entry adds no relay marker", async () => {
    // Given a direct gateway page showing sign-in
    window.history.replaceState(null, "", "/");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    await act(async () => { render(<RemoteApp />); });

    // When pairing with a code is chosen
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Pair with a code instead" })); });

    // Then the entry is the plain LAN route the gateway serves
    expect(window.location.pathname).toBe("/pair");
    expect(window.location.search).toBe("");
  });
});

describe("relay origin classification", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    window.history.replaceState(null, "", "/");
  });

  it("treats an origin whose account health route answers JSON as the relay", async () => {
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(Response.json({ status: "ok" })));

    await expect(servesAccountApi("https://relay.example.com")).resolves.toBe(true);
  });

  it.each([
    { name: "the desktop gateway's SPA fallback 404", response: () => new Response("<!doctype html>", { status: 404 }) },
    { name: "an HTML answer at 200", response: () => new Response("<html></html>", { status: 200, headers: { "Content-Type": "text/html" } }) },
  ])("treats $name as a direct gateway, not a relay", async ({ response }) => {
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async () => response()));

    await expect(servesAccountApi("http://127.0.0.1:43821")).resolves.toBe(false);
  });

  it("treats a failed health probe as a direct gateway so a blip cannot reclassify the page", async () => {
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async () => { throw new TypeError("Failed to fetch"); }));

    await expect(servesAccountApi("http://127.0.0.1:43821")).resolves.toBe(false);
  });

  it("classifies only the product relay origin as the relay by origin", () => {
    expect(isDefaultRelayOrigin(DEFAULT_RELAY_ORIGIN)).toBe(true);
    expect(isDefaultRelayOrigin(window.location.origin)).toBe(false);
  });
});
