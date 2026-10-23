import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MachineApprovalCard, machineApprovalErrorMessage } from "./MachineApprovalCard";
import { AccountSessionError, type MachineRequestView } from "../../../remote/accountSession";

const NOW_MS = 1_700_000_000_000;
const request: MachineRequestView = {
  requestId: "req-1",
  machineId: "mach-1",
  displayName: "noye",
  platform: "linux",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: NOW_MS / 1000 - 30,
  expiresAt: NOW_MS / 1000 + 600,
};

afterEach(() => cleanup());

function renderCard(overrides: Partial<Parameters<typeof MachineApprovalCard>[0]> = {}) {
  const props = {
    request,
    onApprove: vi.fn().mockResolvedValue({ machineId: "mach-1" }),
    onDeny: vi.fn().mockResolvedValue(undefined),
    onDecided: vi.fn(),
    now: () => NOW_MS,
    ...overrides,
  };
  render(<MachineApprovalCard {...props} />);
  return props;
}

describe("MachineApprovalCard", () => {
  it("shows name, OS, IP, app version and expiry", () => {
    renderCard();
    expect(screen.getByRole("region", { name: "Approve machine noye" })).toBeInTheDocument();
    expect(screen.getByText("noye")).toBeInTheDocument();
    expect(screen.getByTestId("approval-platform")).toHaveTextContent("linux");
    expect(screen.getByTestId("approval-ip")).toHaveTextContent("203.0.113.42");
    expect(screen.getByTestId("approval-version")).toHaveTextContent("2026.1023.1");
    expect(screen.getByTestId("approval-expiry")).toHaveTextContent(new Date(request.expiresAt * 1000).toLocaleString());
  });

  it("focuses Deny by default", () => {
    renderCard();
    expect(screen.getByRole("button", { name: "Deny" })).toHaveFocus();
  });

  it("does not steal focus when autoFocus is false", () => {
    renderCard({ autoFocus: false });
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveFocus();
  });

  it("approves, reports the outcome and locks both buttons", async () => {
    const props = renderCard();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    expect(props.onApprove).toHaveBeenCalledTimes(1);
    expect(props.onDeny).not.toHaveBeenCalled();
    expect(props.onDecided).toHaveBeenCalledWith("approved");
    expect(screen.getByRole("status")).toHaveTextContent(/Approved/);
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
  });

  it("denies and reports the outcome", async () => {
    const props = renderCard();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Deny" })); });
    expect(props.onDeny).toHaveBeenCalledTimes(1);
    expect(props.onDecided).toHaveBeenCalledWith("denied");
    expect(screen.getByRole("status")).toHaveTextContent(/Denied/);
  });

  it.each([
    ["MACHINE_REQUEST_FOREIGN_ACCOUNT", /different Ferryx account/],
    ["MACHINE_REQUEST_DECIDED", /already approved or denied/],
    ["MACHINE_REQUEST_EXPIRED", /request expired/],
    ["ACCOUNT_MACHINE_CLAIMED", /belongs to another Ferryx account/],
    ["PLAN_LIMIT_REACHED", /machine limit is reached/],
  ])("maps %s to its message by code", async (code, expected) => {
    const props = renderCard({
      onApprove: vi.fn().mockRejectedValue(new AccountSessionError(code, "server text that must not be shown", 409)),
    });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveAttribute("data-code", code);
    expect(alert).toHaveTextContent(expected);
    expect(alert).not.toHaveTextContent("server text that must not be shown");
    expect(props.onDecided).not.toHaveBeenCalled();
  });

  it("locks both decisions after a final error but allows a retry after a plan limit", async () => {
    renderCard({ onApprove: vi.fn().mockRejectedValue(new AccountSessionError("MACHINE_REQUEST_DECIDED", "x", 409)) });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    cleanup();

    renderCard({ onApprove: vi.fn().mockRejectedValue(new AccountSessionError("PLAN_LIMIT_REACHED", "x", 402)) });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });

  it("disables Approve once the request expired but still allows Deny", () => {
    renderCard({ now: () => (request.expiresAt + 1) * 1000 });
    expect(screen.getByTestId("approval-expiry")).toHaveTextContent("Expired");
    expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });

  it("machineApprovalErrorMessage falls back to the structured message for unknown codes", () => {
    expect(machineApprovalErrorMessage({ code: "SOMETHING_NEW", message: "Readable" })).toBe("Readable");
    expect(machineApprovalErrorMessage({ code: "SOMETHING_NEW" })).toBe("SOMETHING_NEW");
    expect(machineApprovalErrorMessage(new Error("boom"))).toBe("boom");
  });
});
