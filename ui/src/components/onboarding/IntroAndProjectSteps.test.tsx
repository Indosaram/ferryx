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

  it("renders both project choices as buttons", () => {
    render(
      <FirstProjectStep onAddProject={vi.fn()} onConnectMachine={vi.fn()} />
    );

    expect(screen.getByTestId("onboarding-add-project")).toBeDefined();
    expect(screen.getByTestId("onboarding-connect-machine")).toBeDefined();
    expect(screen.getByRole("button", { name: /Add a local folder/ })).toBeDefined();
    expect(screen.getByRole("button", { name: /Connect a machine/ })).toBeDefined();
  });

  it("calls onAddProject when Add a local folder is clicked", () => {
    const onAddProject = vi.fn();
    const onConnectMachine = vi.fn();
    render(
      <FirstProjectStep onAddProject={onAddProject} onConnectMachine={onConnectMachine} />
    );

    fireEvent.click(screen.getByTestId("onboarding-add-project"));

    expect(onAddProject).toHaveBeenCalledTimes(1);
    expect(onConnectMachine).not.toHaveBeenCalled();
  });

  it("calls onConnectMachine when Connect a machine is clicked", () => {
    const onAddProject = vi.fn();
    const onConnectMachine = vi.fn();
    render(
      <FirstProjectStep onAddProject={onAddProject} onConnectMachine={onConnectMachine} />
    );

    fireEvent.click(screen.getByTestId("onboarding-connect-machine"));

    expect(onConnectMachine).toHaveBeenCalledTimes(1);
    expect(onAddProject).not.toHaveBeenCalled();
  });
});
