import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AccountStep, type AccountStepDeps } from "./AccountStep";

const ORIGIN = "https://relay.example.test";
const NOT_ENROLLED = { enrolled: false, accountOrigin: null, enrolledAt: null };
const ENROLLED = { enrolled: true, accountOrigin: ORIGIN, enrolledAt: 1 };

function deps(overrides: Partial<AccountStepDeps>): Partial<AccountStepDeps> {
  return {
    loadStatus: vi.fn().mockResolvedValue(NOT_ENROLLED),
    readToken: vi.fn().mockReturnValue(null),
    adopt: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("AccountStep", () => {
  afterEach(() => cleanup());

  it("shows the email sign-in form when signed out and not linked", async () => {
    render(<AccountStep origin={ORIGIN} deps={deps({})} />);
    expect(await screen.findByTestId("account-sign-in")).toBeTruthy();
  });

  it("reports an already enrolled computer as linked", async () => {
    render(<AccountStep origin={ORIGIN} deps={deps({ loadStatus: vi.fn().mockResolvedValue(ENROLLED) })} />);
    await waitFor(() =>
      expect(screen.getByTestId("onboarding-account").dataset.phase).toBe("linked"),
    );
  });

  it("treats an existing session token as linked with no code entry", async () => {
    const onLinked = vi.fn();
    const adopt = vi.fn().mockResolvedValue(undefined);
    render(
      <AccountStep
        origin={ORIGIN}
        deps={deps({ readToken: vi.fn().mockReturnValue("tok"), adopt })}
        onLinked={onLinked}
      />,
    );
    await waitFor(() => expect(onLinked).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("onboarding-account").dataset.phase).toBe("linked");
    expect(screen.queryByText(/code/i)).toBeNull();
    // Onboarding may be the only surface the user ever sees, so the session reaches the
    // daemon here rather than waiting for Settings to mount.
    expect(adopt).toHaveBeenCalledWith(ORIGIN, "tok");
  });

  it("never offers a retry or a manual link action", async () => {
    render(<AccountStep origin={ORIGIN} deps={deps({})} />);
    await screen.findByTestId("account-sign-in");
    expect(screen.queryByTestId("onboarding-account-retry")).toBeNull();
    expect(screen.queryByText("Retry")).toBeNull();
  });
});
