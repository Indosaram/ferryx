import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FirstProjectStep } from "./FirstProjectStep";
import { IntroStep } from "./IntroStep";

describe("IntroStep", () => {
  afterEach(() => {
    cleanup();
  });

  it("lists one overview row per remaining wizard step, in wizard order", () => {
    render(
      <IntroStep steps={["intro", "features", "agents", "cli", "project"]} />
    );

    const rows = screen.getAllByTestId(/^onboarding-intro-/);
    expect(rows.map((row) => row.getAttribute("data-testid"))).toEqual([
      "onboarding-intro-features",
      "onboarding-intro-agents",
      "onboarding-intro-cli",
      "onboarding-intro-project",
    ]);
  });

  it("omits the command-line row when cli is not part of the wizard", () => {
    render(<IntroStep steps={["intro", "features", "agents", "project"]} />);

    expect(screen.queryByTestId("onboarding-intro-cli")).toBeNull();
  });
});

describe("FirstProjectStep", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders the project row and no machine registration entry point", () => {
    render(<FirstProjectStep onAddProject={vi.fn()} />);

    expect(screen.getByTestId("onboarding-add-project")).toBeDefined();
    expect(screen.getByRole("button", { name: /Add a local folder/ })).toBeDefined();
    expect(screen.queryByTestId("onboarding-connect-machine")).toBeNull();
    expect(screen.queryByRole("button", { name: /Connect a machine/ })).toBeNull();
  });

  it("calls onAddProject when Add a local folder is clicked", () => {
    const onAddProject = vi.fn();
    render(<FirstProjectStep onAddProject={onAddProject} />);

    fireEvent.click(screen.getByTestId("onboarding-add-project"));

    expect(onAddProject).toHaveBeenCalledTimes(1);
  });
});
