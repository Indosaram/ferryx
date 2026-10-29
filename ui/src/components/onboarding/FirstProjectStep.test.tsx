import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FirstProjectStep } from "./FirstProjectStep";

describe("FirstProjectStep Git notice", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows no Git notice when Git works or the folder is not a repository", () => {
    render(
      <FirstProjectStep
        onAddProject={vi.fn()}
        onConnectMachine={vi.fn()}
        gitUnavailable={{ kind: "notARepository" }}
      />
    );

    expect(screen.queryByTestId("onboarding-git-unavailable")).toBeNull();
  });

  it.each([
    [{ kind: "gitMissing" } as const, /Install Git/],
    [{ kind: "xcodeCommandLineToolsMissing" } as const, /xcode-select --install/],
    [
      { kind: "dubiousOwnership", path: "/srv/repo" } as const,
      /safe\.directory \/srv\/repo/,
    ],
    [{ kind: "gitFailed", message: "fatal: bad config" } as const, /fatal: bad config/],
    [{ kind: "nonUtf8Output" } as const, /not UTF-8/],
  ])("renders the reason Git is unavailable: %o", (reason, text) => {
    render(
      <FirstProjectStep onAddProject={vi.fn()} onConnectMachine={vi.fn()} gitUnavailable={reason} />
    );

    expect(screen.getByTestId("onboarding-git-unavailable").textContent).toMatch(text);
  });
});
