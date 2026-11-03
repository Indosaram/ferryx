import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AccountSessionError } from "../../remote/accountSession";
import { AccountStep, type AccountStepDeps } from "./AccountStep";

const ORIGIN = "https://relay.example.test";
const NOT_ENROLLED = { enrolled: false, accountOrigin: null, enrolledAt: null };
const ENROLLED = { enrolled: true, accountOrigin: ORIGIN, enrolledAt: 1 };

function deps(overrides: Partial<AccountStepDeps>): Partial<AccountStepDeps> {
  return {
    loadStatus: vi.fn().mockResolvedValue(NOT_ENROLLED),
    issueCode: vi.fn().mockResolvedValue({ code: "ENR-1" }),
    enroll: vi.fn().mockResolvedValue(ENROLLED),
    readToken: vi.fn().mockReturnValue(null),
    ...overrides,
  };
}

describe("AccountStep", () => {
  afterEach(() => cleanup());

  it("shows the email sign-in form when signed out and not linked", async () => {
    render(<AccountStep origin={ORIGIN} deps={deps({})} />);
    expect(await screen.findByTestId("account-sign-in")).toBeTruthy();
  });

  it("reports an already enrolled computer as linked without enrolling again", async () => {
    const d = deps({ loadStatus: vi.fn().mockResolvedValue(ENROLLED) });
    render(<AccountStep origin={ORIGIN} deps={d} />);
    await waitFor(() =>
      expect(screen.getByTestId("onboarding-account").dataset.phase).toBe("linked"),
    );
    expect(d.enroll).not.toHaveBeenCalled();
  });

  it("links this computer with a freshly issued code when a session token exists", async () => {
    const onLinked = vi.fn();
    const d = deps({ readToken: vi.fn().mockReturnValue("tok") });
    render(<AccountStep origin={ORIGIN} deps={d} onLinked={onLinked} />);
    await waitFor(() => expect(onLinked).toHaveBeenCalledTimes(1));
    expect(d.issueCode).toHaveBeenCalledWith(ORIGIN, "tok");
    expect(d.enroll).toHaveBeenCalledWith(ORIGIN, "ENR-1", "tok");
    expect(screen.getByTestId("onboarding-account").dataset.phase).toBe("linked");
  });

  it("surfaces the structured error code and retries on demand", async () => {
    const issueCode = vi
      .fn()
      .mockRejectedValueOnce(new AccountSessionError("UNAUTHORIZED", "Session expired", 401))
      .mockResolvedValueOnce({ code: "ENR-2" });
    const d = deps({ readToken: vi.fn().mockReturnValue("tok"), issueCode });
    render(<AccountStep origin={ORIGIN} deps={d} />);
    expect(await screen.findByText("UNAUTHORIZED: Session expired")).toBeTruthy();

    fireEvent.click(screen.getByTestId("onboarding-account-retry"));
    await waitFor(() =>
      expect(screen.getByTestId("onboarding-account").dataset.phase).toBe("linked"),
    );
    expect(d.enroll).toHaveBeenCalledWith(ORIGIN, "ENR-2", "tok");
  });
});
