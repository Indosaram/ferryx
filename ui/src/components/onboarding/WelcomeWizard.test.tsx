import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ResolvedAgent } from "../../lib/agentsSettings";
import { ONBOARDING_STEP_ORDER, type OnboardingStepId } from "../../lib/onboarding";
import { WelcomeWizard, type WelcomeWizardProps } from "./WelcomeWizard";

vi.mock("./FeaturesStep", () => ({
  FeaturesStep: () => <div data-testid="mock-features-step" />,
}));

vi.mock("./AgentsCliStep", () => ({
  AgentsCliStep: () => <div data-testid="mock-agents-step" />,
}));

vi.mock("./CliStep", () => ({
  CliStep: () => <div data-testid="mock-cli-step" />,
}));

vi.mock("./AccountStep", () => ({
  AccountStep: ({ onLinked }: { onLinked?: () => void }) => (
    <button type="button" data-testid="mock-account-link" onClick={() => onLinked?.()} />
  ),
}));

const remoteCalls = vi.hoisted(() => ({
  getRemoteStatus: vi.fn(),
  listRemoteDevices: vi.fn(),
  createPairingCode: vi.fn(),
  enableRemoteGateway: vi.fn(),
}));

vi.mock("../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/tauri")>()),
  ...remoteCalls,
}));

const gatewayOff = {
  enabled: false,
  mode: "off" as const,
  port: 43821,
  boundAddress: null,
  localIp: null,
  relayUrl: null,
};
const gatewayOn = { ...gatewayOff, enabled: true, mode: "localNetwork" as const, localIp: "192.168.0.5" };
const pairedPhone = {
  id: "phone-1",
  name: "Phone",
  permission: "view" as const,
  createdAt: 1,
  lastSeenAt: 1,
  revoked: false,
};

const mockAgents: readonly ResolvedAgent[] = [
  {
    name: "claude",
    available: true,
    enabled: true,
    command: "claude",
    args: "",
    custom: false,
  },
];

function createDefaultProps(overrides?: Partial<WelcomeWizardProps>): WelcomeWizardProps {
  return {
    steps: ["intro", "features", "project"] as readonly OnboardingStepId[],
    agents: mockAgents,
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

describe("WelcomeWizard", () => {
  afterEach(() => {
    cleanup();
  });

  it('indicator shows "Step 1 of 3" for steps ["intro","features","project"]', () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "project"],
    });
    render(<WelcomeWizard {...props} />);

    const indicator = screen.getByTestId("onboarding-step-indicator");
    expect(indicator.textContent).toBe("Step 1 of 3");
  });

  it('Continue calls onStepCompleted("intro") and shows the features step', () => {
    const onStepCompleted = vi.fn();
    const props = createDefaultProps({
      steps: ["intro", "features", "project"],
      onStepCompleted,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByText("Set up Ferryx")).toBeDefined();
    fireEvent.click(screen.getByTestId("onboarding-next"));

    expect(onStepCompleted).toHaveBeenCalledTimes(1);
    expect(onStepCompleted).toHaveBeenCalledWith("intro");
    expect(screen.getByTestId("mock-features-step")).toBeDefined();
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 2 of 3");
  });

  it("Back hidden on first step and returns to previous", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "project"],
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.queryByTestId("onboarding-back")).toBeNull();

    fireEvent.click(screen.getByTestId("onboarding-next"));
    expect(screen.getByTestId("onboarding-back")).toBeDefined();

    fireEvent.click(screen.getByTestId("onboarding-back"));
    expect(screen.queryByTestId("onboarding-back")).toBeNull();
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 1 of 3");
    expect(screen.getByText("Set up Ferryx")).toBeDefined();
  });

  it("last step shows Done (not Continue) and Done calls onStepCompleted then onFinish", () => {
    const onStepCompleted = vi.fn();
    const onFinish = vi.fn();
    const props = createDefaultProps({
      steps: ["intro", "features", "project"],
      onStepCompleted,
      onFinish,
      initialStepIndex: 2,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.queryByTestId("onboarding-next")).toBeNull();
    const finishButton = screen.getByTestId("onboarding-finish");
    expect(finishButton).toBeDefined();
    expect(finishButton.textContent).toBe("Done");

    const callOrder: string[] = [];
    onStepCompleted.mockImplementation(() => callOrder.push("stepCompleted"));
    onFinish.mockImplementation(() => callOrder.push("finish"));

    fireEvent.click(finishButton);

    expect(onStepCompleted).toHaveBeenCalledWith("project");
    expect(onFinish).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(["stepCompleted", "finish"]);
  });

  it("Skip calls onSkip", () => {
    const onSkip = vi.fn();
    const props = createDefaultProps({ onSkip });
    render(<WelcomeWizard {...props} />);

    fireEvent.click(screen.getByTestId("onboarding-skip"));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it("Remind me later and Escape call onRemindLater", () => {
    const onRemindLater = vi.fn();
    const props = createDefaultProps({ onRemindLater });
    render(<WelcomeWizard {...props} />);

    fireEvent.click(screen.getByTestId("onboarding-remind-later"));
    expect(onRemindLater).toHaveBeenCalledTimes(1);

    const dialog = screen.getByRole("dialog", { name: "Welcome to Ferryx" });
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onRemindLater).toHaveBeenCalledTimes(2);
  });

  it('project step Add Project button calls onStepCompleted("project") then onAddProject', () => {
    const onStepCompleted = vi.fn();
    const onAddProject = vi.fn();
    const props = createDefaultProps({
      steps: ["intro", "project"],
      initialStepIndex: 1,
      onStepCompleted,
      onAddProject,
    });
    render(<WelcomeWizard {...props} />);

    const callOrder: string[] = [];
    onStepCompleted.mockImplementation(() => callOrder.push("stepCompleted"));
    onAddProject.mockImplementation(() => callOrder.push("addProject"));

    const addProjectBtn = screen.getByTestId("onboarding-add-project");
    fireEvent.click(addProjectBtn);

    expect(onStepCompleted).toHaveBeenCalledWith("project");
    expect(onAddProject).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(["stepCompleted", "addProject"]);
  });

  it('project step Connect a machine button calls onStepCompleted("project") then onConnectMachine', () => {
    const onStepCompleted = vi.fn();
    const onConnectMachine = vi.fn();
    const props = createDefaultProps({
      steps: ["intro", "project"],
      initialStepIndex: 1,
      onStepCompleted,
      onConnectMachine,
    });
    render(<WelcomeWizard {...props} />);

    const callOrder: string[] = [];
    onStepCompleted.mockImplementation(() => callOrder.push("stepCompleted"));
    onConnectMachine.mockImplementation(() => callOrder.push("connectMachine"));

    const connectMachineBtn = screen.getByTestId("onboarding-connect-machine");
    fireEvent.click(connectMachineBtn);

    expect(onStepCompleted).toHaveBeenCalledWith("project");
    expect(onConnectMachine).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(["stepCompleted", "connectMachine"]);
  });

  it("initialStepIndex=1 starts on the second step", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "project"],
      initialStepIndex: 1,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 2 of 3");
    expect(screen.getByTestId("mock-features-step")).toBeDefined();
    expect(screen.getByTestId("onboarding-back")).toBeDefined();
  });

  it('root has role="dialog" with aria-label "Welcome to Ferryx"', () => {
    const props = createDefaultProps();
    render(<WelcomeWizard {...props} />);

    const dialog = screen.getByRole("dialog", { name: "Welcome to Ferryx" });
    expect(dialog).toBeDefined();
    expect(dialog.getAttribute("role")).toBe("dialog");
    expect(dialog.getAttribute("aria-label")).toBe("Welcome to Ferryx");
  });

  it("clamps initialStepIndex within step range", () => {
    const propsNegative = createDefaultProps({
      steps: ["intro", "features", "project"],
      initialStepIndex: -5,
    });
    const { unmount } = render(<WelcomeWizard {...propsNegative} />);
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 1 of 3");
    unmount();

    const propsOverflow = createDefaultProps({
      steps: ["intro", "features", "project"],
      initialStepIndex: 99,
    });
    render(<WelcomeWizard {...propsOverflow} />);
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 3 of 3");
    expect(screen.getByTestId("onboarding-finish")).toBeDefined();
  });

  it("renders agents step mock when step is agents", () => {
    const props = createDefaultProps({
      steps: ["intro", "agents", "project"],
      initialStepIndex: 1,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("mock-agents-step")).toBeDefined();
  });

  it("walks the five-step order with Continue and returns with Back", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "agents", "cli", "project"],
    });
    render(<WelcomeWizard {...props} />);

    const heading = () => screen.getByRole("heading", { level: 2 }).textContent;
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 1 of 5");
    expect(heading()).toBe("Set up Ferryx");

    const expected = [
      "What Ferryx does",
      "Choose your default agent",
      "Install the command-line tool",
      "Open your first project",
    ];
    expected.forEach((title, index) => {
      fireEvent.click(screen.getByTestId("onboarding-next"));
      expect(heading()).toBe(title);
      expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe(
        `Step ${index + 2} of 5`,
      );
    });
    expect(screen.getByTestId("onboarding-finish")).toBeDefined();

    fireEvent.click(screen.getByTestId("onboarding-back"));
    expect(heading()).toBe("Install the command-line tool");
    expect(screen.getByTestId("mock-cli-step")).toBeDefined();
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 4 of 5");
  });

  it("labels the agents and command-line steps separately in the rail", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "agents", "cli", "project"],
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("onboarding-rail-agents").textContent).toContain("Agents");
    expect(screen.getByTestId("onboarding-rail-agents").textContent).not.toContain("CLI");
    expect(screen.getByTestId("onboarding-rail-cli").textContent).toContain("Command line");
  });

  it("lets Continue leave the optional cli step", () => {
    const onStepCompleted = vi.fn();
    const props = createDefaultProps({
      steps: ["intro", "agents", "cli", "project"],
      initialStepIndex: 2,
      onStepCompleted,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("mock-cli-step")).toBeDefined();
    fireEvent.click(screen.getByTestId("onboarding-next"));

    expect(onStepCompleted).toHaveBeenCalledWith("cli");
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Open your first project");
  });

  it("marks already satisfied steps done in the rail but never the current step", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "agents", "project"],
      doneSteps: ["agents"],
      initialStepIndex: 0,
    });
    render(<WelcomeWizard {...props} />);

    const agentsRow = screen.getByTestId("onboarding-rail-agents");
    const introRow = screen.getByTestId("onboarding-rail-intro");
    expect(agentsRow.querySelector("svg")).not.toBeNull();
    expect(introRow.querySelector("svg")).toBeNull();
    expect(introRow.getAttribute("aria-current")).toBe("step");
  });

  it("initialStepIndex=2 opens on the agents step of the four-step order", () => {
    const props = createDefaultProps({
      steps: ["intro", "features", "agents", "project"],
      initialStepIndex: 2,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Choose your default agent");
    expect(screen.getByTestId("mock-agents-step")).toBeDefined();
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe("Step 3 of 4");
  });

  it("opens on the intro step and ends on the account step", () => {
    const props = createDefaultProps({ steps: ONBOARDING_STEP_ORDER });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("onboarding-rail-intro").getAttribute("aria-current")).toBe("step");
    expect(screen.getByTestId("onboarding-step-indicator").textContent).toBe(
      `Step 1 of ${ONBOARDING_STEP_ORDER.length}`,
    );
    const railIds = screen
      .getAllByTestId(/^onboarding-rail-/)
      .map((row) => row.getAttribute("data-testid"));
    expect(railIds[0]).toBe("onboarding-rail-intro");
    expect(railIds[railIds.length - 1]).toBe("onboarding-rail-account");
  });

  it("finishes from an unlinked account step without completing the account step", () => {
    const onStepCompleted = vi.fn();
    const onFinish = vi.fn();
    const steps = ONBOARDING_STEP_ORDER;
    const props = createDefaultProps({
      steps,
      initialStepIndex: steps.length - 1,
      onStepCompleted,
      onFinish,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("onboarding-rail-account").getAttribute("aria-current")).toBe("step");
    const finishButton = screen.getByTestId("onboarding-finish");
    expect(finishButton.textContent).toBe("Finish without signing in");

    fireEvent.click(finishButton);

    expect(onFinish).toHaveBeenCalledTimes(1);
    expect(onStepCompleted).not.toHaveBeenCalledWith("account");
  });

  it("shows Done on the account step once this computer is linked", () => {
    const onStepCompleted = vi.fn();
    const onFinish = vi.fn();
    const steps = ONBOARDING_STEP_ORDER;
    const props = createDefaultProps({
      steps,
      initialStepIndex: steps.length - 1,
      onStepCompleted,
      onFinish,
    });
    render(<WelcomeWizard {...props} />);

    fireEvent.click(screen.getByTestId("mock-account-link"));
    expect(onStepCompleted).toHaveBeenCalledWith("account");
    const finishButton = screen.getByTestId("onboarding-finish");
    expect(finishButton.textContent).toBe("Done");

    fireEvent.click(finishButton);
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it("shows Done on the account step when it is already linked", () => {
    const steps = ONBOARDING_STEP_ORDER;
    const props = createDefaultProps({
      steps,
      doneSteps: ["account"],
      initialStepIndex: steps.length - 1,
    });
    render(<WelcomeWizard {...props} />);

    expect(screen.getByTestId("onboarding-finish").textContent).toBe("Done");
  });

  it("returns null when steps is empty", () => {
    const props = createDefaultProps({
      steps: [],
    });
    const { container } = render(<WelcomeWizard {...props} />);
    expect(container.firstChild).toBeNull();
  });

  describe("phone step", () => {
    const phoneSteps = ["project", "phone", "account"] as readonly OnboardingStepId[];

    afterEach(() => {
      vi.useRealTimers();
    });

    it("offers to turn on remote access instead of enabling it silently", async () => {
      remoteCalls.getRemoteStatus.mockResolvedValue(gatewayOff);
      remoteCalls.listRemoteDevices.mockResolvedValue([]);
      remoteCalls.enableRemoteGateway.mockResolvedValue(gatewayOn);
      await act(async () => {
        render(<WelcomeWizard {...createDefaultProps({ steps: phoneSteps, initialStepIndex: 1 })} />);
      });

      expect(remoteCalls.enableRemoteGateway).not.toHaveBeenCalled();
      expect(screen.queryByRole("button", { name: "Pair a device" })).toBeNull();
      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-phone-enable"));
      });
      expect(remoteCalls.enableRemoteGateway).toHaveBeenCalledTimes(1);
      expect(screen.queryByTestId("onboarding-phone-enable")).toBeNull();
      expect(screen.getByRole("button", { name: "Pair a device" })).toBeDefined();
      expect(screen.getByTestId("onboarding-phone-url").textContent).toBe("http://192.168.0.5:43821/pair");
    });

    it("shows the issued code with a countdown", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: false });
      remoteCalls.getRemoteStatus.mockResolvedValue(gatewayOn);
      remoteCalls.listRemoteDevices.mockResolvedValue([]);
      remoteCalls.createPairingCode.mockResolvedValue({ code: "482913", expiresInSeconds: 600 });
      await act(async () => {
        render(<WelcomeWizard {...createDefaultProps({ steps: phoneSteps, initialStepIndex: 1 })} />);
      });

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Pair a device" }));
      });
      expect(remoteCalls.createPairingCode).toHaveBeenCalledWith("view");
      expect(screen.getByTestId("pair-device-code").textContent).toBe("482913");
      expect(screen.getByTestId("pair-device-countdown").textContent).toContain("10:00");
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(screen.getByTestId("pair-device-countdown").textContent).toContain("9:59");
    });

    it("completes once a new device appears in the polled device list", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: false });
      remoteCalls.getRemoteStatus.mockResolvedValue(gatewayOn);
      remoteCalls.listRemoteDevices.mockResolvedValue([]);
      const onStepCompleted = vi.fn();
      await act(async () => {
        render(
          <WelcomeWizard
            {...createDefaultProps({ steps: phoneSteps, initialStepIndex: 1, onStepCompleted })}
          />,
        );
      });

      expect(screen.getByTestId("onboarding-next").textContent).toBe("Skip");
      await act(async () => {
        vi.advanceTimersByTime(2000);
      });
      expect(screen.queryByTestId("onboarding-phone-connected")).toBeNull();
      expect(onStepCompleted).not.toHaveBeenCalled();

      remoteCalls.listRemoteDevices.mockResolvedValue([pairedPhone]);
      await act(async () => {
        vi.advanceTimersByTime(2000);
      });
      expect(screen.getByTestId("onboarding-phone-connected")).toBeDefined();
      expect(onStepCompleted).toHaveBeenCalledWith("phone");
      expect(screen.getByTestId("onboarding-next").textContent).toBe("Continue");
    });

    it("Skip moves on without recording the phone step", async () => {
      remoteCalls.getRemoteStatus.mockResolvedValue(gatewayOff);
      remoteCalls.listRemoteDevices.mockResolvedValue([]);
      const onStepCompleted = vi.fn();
      await act(async () => {
        render(
          <WelcomeWizard
            {...createDefaultProps({ steps: phoneSteps, initialStepIndex: 1, onStepCompleted })}
          />,
        );
      });

      fireEvent.click(screen.getByTestId("onboarding-next"));
      expect(onStepCompleted).not.toHaveBeenCalled();
      expect(screen.getByTestId("onboarding-rail-account").getAttribute("aria-current")).toBe("step");
    });
  });
});
