import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { APPROVE_PENDING_CODE_KEY, ApprovePage, isApproveRoute } from "./ApprovePage";
import { RemoteApp } from "./RemoteApp";
import * as accountSessionModule from "./accountSession";
import { AccountSessionError, type MachineRequestView } from "./accountSession";

const relayUrl = "https://relay.example.com";
const nowSecs = Math.floor(Date.now() / 1000);
const request: MachineRequestView = {
  requestId: "req-1",
  machineId: "mach-new",
  displayName: "noye",
  platform: "linux",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: nowSecs,
  expiresAt: nowSecs + 900,
};

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("ApprovePage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    sessionStorage.clear();
    window.history.replaceState(null, "", "/approve?code=abcd-efgh");
    vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
  });

  afterEach(() => {
    cleanup();
    window.history.replaceState(null, "", "/");
  });

  it("isApproveRoute matches /approve with or without a trailing slash only", () => {
    expect(isApproveRoute("/approve")).toBe(true);
    expect(isApproveRoute("/approve/")).toBe(true);
    expect(isApproveRoute("/")).toBe(false);
    expect(isApproveRoute("/approve/extra")).toBe(false);
  });

  it("signed out: shows the account login page and keeps the code out of the URL", async () => {
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue(null);
    const consume = vi.spyOn(accountSessionModule, "consumeLogin");
    const lookup = vi.spyOn(accountSessionModule, "lookupMachineRequest");
    render(<ApprovePage relayUrl={relayUrl} />);
    await flush();

    expect(screen.getByPlaceholderText("name@example.com")).toBeInTheDocument();
    // The approval code is not a magic-link login code.
    expect(window.location.search).toBe("");
    expect(consume).not.toHaveBeenCalled();
    expect(lookup).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(APPROVE_PENDING_CODE_KEY)).toBe("abcd-efgh");
  });

  it("signed in: looks up the ?code= request and approves it", async () => {
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue("tok");
    const lookup = vi.spyOn(accountSessionModule, "lookupMachineRequest").mockResolvedValue(request);
    const approve = vi.spyOn(accountSessionModule, "approveMachineRequest").mockResolvedValue({ machineId: "mach-new" });
    const deny = vi.spyOn(accountSessionModule, "denyMachineRequest").mockResolvedValue(undefined);
    render(<ApprovePage relayUrl={relayUrl} />);
    await flush();

    expect(lookup).toHaveBeenCalledWith(relayUrl, "tok", "ABCD-EFGH");
    expect(screen.getByTestId("machine-approval-card")).toHaveTextContent("noye");
    expect(screen.getByRole("button", { name: "Deny" })).toHaveFocus();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    expect(approve).toHaveBeenCalledWith(relayUrl, "tok", "req-1", "ABCD-EFGH");
    expect(deny).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(APPROVE_PENDING_CODE_KEY)).toBeNull();
  });

  it("signs in, then continues to the lookup with the remembered code", async () => {
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue(null);
    const lookup = vi.spyOn(accountSessionModule, "lookupMachineRequest").mockResolvedValue(request);
    vi.spyOn(accountSessionModule, "requestLogin").mockResolvedValue({ loginHandle: "h" });
    vi.spyOn(accountSessionModule, "pollLogin").mockResolvedValue({ status: "approved", token: "new-tok", email: "a@b.c" });
    vi.spyOn(accountSessionModule, "storeAccountSessionToken").mockImplementation(() => {});
    vi.useFakeTimers();
    try {
      render(<ApprovePage relayUrl={relayUrl} />);
      await flush();
      fireEvent.change(screen.getByPlaceholderText("name@example.com"), { target: { value: "a@b.c" } });
      await act(async () => { fireEvent.submit(screen.getByPlaceholderText("name@example.com").closest("form")!); });
      await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
      await flush();
    } finally {
      vi.useRealTimers();
    }
    expect(lookup).toHaveBeenCalledWith(relayUrl, "new-tok", "ABCD-EFGH");
    expect(screen.getByTestId("machine-approval-card")).toBeInTheDocument();
  });

  it("maps a foreign-account lookup refusal by code", async () => {
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue("tok");
    vi.spyOn(accountSessionModule, "lookupMachineRequest").mockRejectedValue(
      new AccountSessionError("MACHINE_REQUEST_FOREIGN_ACCOUNT", "raw", 403),
    );
    render(<ApprovePage relayUrl={relayUrl} />);
    await flush();
    expect(screen.getByRole("alert")).toHaveTextContent(/different Ferryx account/);
    expect(screen.queryByTestId("machine-approval-card")).toBeNull();
  });

  it("drops an expired session back to sign-in on UNAUTHORIZED", async () => {
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue("old");
    const clear = vi.spyOn(accountSessionModule, "clearStoredAccountSessionToken").mockImplementation(() => {});
    vi.spyOn(accountSessionModule, "lookupMachineRequest").mockRejectedValue(
      new AccountSessionError("UNAUTHORIZED", "expired", 401),
    );
    render(<ApprovePage relayUrl={relayUrl} />);
    await flush();
    expect(clear).toHaveBeenCalled();
    expect(screen.getByPlaceholderText("name@example.com")).toBeInTheDocument();
  });

  it("without a code asks for one", async () => {
    window.history.replaceState(null, "", "/approve");
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue("tok");
    const lookup = vi.spyOn(accountSessionModule, "lookupMachineRequest").mockResolvedValue(request);
    render(<ApprovePage relayUrl={relayUrl} />);
    await flush();
    expect(lookup).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Approval code"), { target: { value: "wxyz2345" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Find machine" })); });
    await flush();
    expect(lookup).toHaveBeenCalledWith(relayUrl, "tok", "WXYZ-2345");
  });
});

describe("RemoteApp /approve route", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    sessionStorage.clear();
    localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    window.history.replaceState(null, "", "/");
  });

  it("renders the approval flow instead of the remote workspace", async () => {
    window.history.replaceState(null, "", "/approve?code=ABCD-EFGH");
    vi.spyOn(accountSessionModule, "resolveAccountOrigin").mockResolvedValue(relayUrl);
    vi.spyOn(accountSessionModule, "getStoredAccountSessionToken").mockReturnValue("tok");
    const lookup = vi.spyOn(accountSessionModule, "lookupMachineRequest").mockResolvedValue(request);
    render(<RemoteApp />);
    await flush();

    expect(lookup).toHaveBeenCalledWith(relayUrl, "tok", "ABCD-EFGH");
    expect(screen.getByRole("heading", { name: "Approve a machine" })).toBeInTheDocument();
    expect(screen.getByTestId("machine-approval-card")).toBeInTheDocument();
  });
});
