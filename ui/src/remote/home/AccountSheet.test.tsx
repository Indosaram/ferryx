import { act, fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AccountSessionError } from "../accountSession";
import { AccountSheet } from "./AccountSheet";

function renderSheet(onDelete: (email: string) => Promise<void>) {
  const onDeleted = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <AccountSheet open onOpenChange={onOpenChange} email="me@example.com" onSignOut={vi.fn()} onDelete={onDelete} onDeleted={onDeleted} />,
  );
  fireEvent.click(screen.getByTestId("account-delete-open"));
  fireEvent.change(screen.getByTestId("account-delete-email"), { target: { value: " typo@example.com " } });
  return { onDeleted, onOpenChange };
}

afterEach(cleanup);

describe("AccountSheet delete flow", () => {
  it("shows the mismatch in text and stays signed in", async () => {
    const onDelete = vi.fn(async () => {
      throw new AccountSessionError("CONFIRMATION_MISMATCH", "mismatch", 400);
    });
    const { onDeleted } = renderSheet(onDelete);
    await act(async () => fireEvent.click(screen.getByTestId("account-delete-confirm")));
    expect(onDelete).toHaveBeenCalledWith("typo@example.com");
    expect(screen.getByTestId("account-delete-error")).toHaveAttribute("data-code", "CONFIRMATION_MISMATCH");
    expect(screen.getByTestId("account-delete-error")).toHaveTextContent("does not match");
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("names an active subscription as the reason", async () => {
    renderSheet(async () => {
      throw new AccountSessionError("SUBSCRIPTION_ACTIVE", "sub", 409);
    });
    await act(async () => fireEvent.click(screen.getByTestId("account-delete-confirm")));
    expect(screen.getByTestId("account-delete-error")).toHaveTextContent("active subscription");
  });

  it("closes and signs out after a confirmed deletion", async () => {
    const { onDeleted, onOpenChange } = renderSheet(async () => {});
    await act(async () => fireEvent.click(screen.getByTestId("account-delete-confirm")));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });

  it("keeps the email out of the field and moves focus into the confirm step and back", () => {
    render(<AccountSheet open onOpenChange={vi.fn()} email="me@example.com" onSignOut={vi.fn()} onDelete={vi.fn()} onDeleted={vi.fn()} />);
    fireEvent.click(screen.getByTestId("account-delete-open"));
    const input = screen.getByTestId("account-delete-email");
    expect(input).not.toHaveAttribute("placeholder");
    expect(document.activeElement).toBe(input);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(document.activeElement).toBe(screen.getByTestId("account-delete-open"));
  });

  it("says the email is unavailable instead of a filler phrase", () => {
    render(<AccountSheet open onOpenChange={vi.fn()} email={null} onSignOut={vi.fn()} onDelete={vi.fn()} onDeleted={vi.fn()} />);
    expect(screen.getByTestId("account-email")).toHaveTextContent("Email unavailable");
  });

  it("shows plain copy for an unknown failure instead of the raw message", async () => {
    render(<AccountSheet open onOpenChange={vi.fn()} email="me@example.com" onSignOut={vi.fn()} onDelete={async () => { throw new AccountSessionError("BOOM_42", "stack trace", 500); }} onDeleted={vi.fn()} />);
    fireEvent.click(screen.getByTestId("account-delete-open"));
    fireEvent.change(screen.getByTestId("account-delete-email"), { target: { value: "me@example.com" } });
    await act(async () => fireEvent.click(screen.getByTestId("account-delete-confirm")));
    expect(screen.getByTestId("account-delete-error")).not.toHaveTextContent("stack trace");
  });
});
