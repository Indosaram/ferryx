import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { seriousAxeViolations } from "../../test/axe";
import type { OnboardingStepId } from "../../lib/onboarding";
import { WelcomeWizard, type WelcomeWizardProps } from "./WelcomeWizard";

vi.mock("./FeaturesStep", () => ({ FeaturesStep: () => <div data-testid="mock-features-step" /> }));
vi.mock("./AgentsCliStep", () => ({ AgentsCliStep: () => <div data-testid="mock-agents-step" /> }));
vi.mock("./CliStep", () => ({ CliStep: () => <div data-testid="mock-cli-step" /> }));

function props(overrides?: Partial<WelcomeWizardProps>): WelcomeWizardProps {
  return {
    steps: ["intro", "features", "project"] as readonly OnboardingStepId[],
    agents: [],
    isMac: true,
    onAddProject: vi.fn(),
    onConnectMachine: vi.fn(),
    onStepCompleted: vi.fn(),
    onFinish: vi.fn(),
    onSkip: vi.fn(),
    onRemindLater: vi.fn(),
    ...overrides,
  };
}

describe("WelcomeWizard accessibility", () => {
  afterEach(() => cleanup());

  it("has no serious or critical axe violations on the first and last step", async () => {
    const { container } = render(<WelcomeWizard {...props()} />);
    expect(await seriousAxeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByTestId("onboarding-next"));
    fireEvent.click(screen.getByTestId("onboarding-next"));
    expect(screen.getByTestId("onboarding-finish")).toBeInTheDocument();
    expect(await seriousAxeViolations(container)).toEqual([]);
  });

  it("names the dialog and the step rail from the i18n resources", () => {
    render(<WelcomeWizard {...props()} />);
    expect(screen.getByRole("dialog", { name: "Welcome to Ferryx" })).toBeInTheDocument();
    const rail = screen.getByRole("list", { name: "Setup steps" });
    expect(rail.querySelector('[aria-current="step"]')).toHaveTextContent("Welcome");
    expect(screen.getByTestId("onboarding-step-indicator")).toHaveTextContent("Step 1 of 3");
    expect(screen.getByRole("heading", { name: "Set up Ferryx" })).toHaveFocus();
  });
});
