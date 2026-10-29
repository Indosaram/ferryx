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
  createWorktree: vi.fn(),
}));

vi.mock("../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/tauri")>()),
  ...remoteCalls,
}));

const toastMock = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

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
      expect(screen.getByTestId("onboarding-phone-url").textContent).toBe("http://192.168.0.5:43821");
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

  describe("try step: first worktree", () => {
    const project = {
      workspaceId: "scratch",
      repoRoot: "/repos/scratch",
      gitRoot: "/repos/scratch",
    };
    const created = {
      path: "/repos/.ferryx-worktrees/scratch/try-ferryx",
      head: "abc",
      branch: "refs/heads/ferryx/try-ferryx",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const agents: ResolvedAgent[] = [
      { name: "claude", available: true, enabled: true, command: "claude", args: "", custom: false },
      { name: "codex", available: true, enabled: true, command: "codex", args: "", custom: false },
    ];

    function renderTry(overrides?: Partial<WelcomeWizardProps>) {
      const props = createDefaultProps({
        steps: ["project", "try", "phone"],
        initialStepIndex: 1,
        agents,
        tryProject: project,
        defaultAgentId: "codex",
        onTryWorktreeCreated: vi.fn(),
        ...overrides,
      });
      render(<WelcomeWizard {...props} />);
      return props;
    }

    afterEach(() => {
      remoteCalls.createWorktree.mockReset();
    });

    it("creates try-ferryx from HEAD, starts the default agent, then finishes", async () => {
      remoteCalls.createWorktree.mockResolvedValue(created);
      const props = renderTry();

      expect(remoteCalls.createWorktree).not.toHaveBeenCalled();
      const tryButton = screen.getByTestId("onboarding-try-create");
      expect(tryButton.textContent).toBe("Create a worktree and start Codex");
      await act(async () => {
        fireEvent.click(tryButton);
      });

      expect(remoteCalls.createWorktree).toHaveBeenCalledTimes(1);
      expect(remoteCalls.createWorktree).toHaveBeenCalledWith({
        workspaceId: "scratch",
        worktree: { wsId: "scratch", slug: "try-ferryx" },
        baseRef: "HEAD",
      });
      expect(props.onTryWorktreeCreated).toHaveBeenCalledWith(created, { agentId: "codex" });
      expect(props.onStepCompleted).toHaveBeenCalledWith("try");
      expect(props.onFinish).toHaveBeenCalledTimes(1);
    });

    it("Skip creates nothing, does not record the step, and moves to the next step", () => {
      const props = renderTry();

      const skip = screen.getByTestId("onboarding-next");
      expect(skip.textContent).toBe("Skip");
      fireEvent.click(skip);

      expect(props.onStepCompleted).not.toHaveBeenCalled();
      expect(remoteCalls.createWorktree).not.toHaveBeenCalled();
      expect(props.onTryWorktreeCreated).not.toHaveBeenCalled();
      expect(props.onFinish).not.toHaveBeenCalled();
      expect(screen.getByTestId("onboarding-rail-phone").getAttribute("aria-current")).toBe("step");
    });

    it("offers only Skip for a folder that is not a Git repository", () => {
      const props = renderTry({ tryProject: { ...project, gitRoot: null } });

      expect(screen.getByTestId("onboarding-try-not-git").textContent).toContain(
        "This folder is not a Git repository",
      );
      expect(screen.queryByTestId("onboarding-try-create")).toBeNull();
      fireEvent.click(screen.getByTestId("onboarding-next"));
      expect(remoteCalls.createWorktree).not.toHaveBeenCalled();
      expect(props.onFinish).not.toHaveBeenCalled();
    });

    it("switches to the not-a-Git-repository view when the backend says so", async () => {
      remoteCalls.createWorktree.mockRejectedValue({ code: "INVALID_REPO_ROOT", message: "Not a git repository: /repos/scratch" });
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("onboarding-try-not-git")).toBeDefined();
      expect(screen.queryByTestId("onboarding-try-create")).toBeNull();
      expect(props.onFinish).not.toHaveBeenCalled();
    });

    it("keeps the step open with the backend error when creation fails", async () => {
      remoteCalls.createWorktree.mockRejectedValue({ code: "GIT_ERROR", message: "Git command failed" });
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(screen.getByTestId("onboarding-try-error").textContent).toBe("Git command failed");
      expect(props.onTryWorktreeCreated).not.toHaveBeenCalled();
      expect(props.onFinish).not.toHaveBeenCalled();
    });

    it("takes the next free slug when try-ferryx already exists", async () => {
      remoteCalls.createWorktree
        .mockRejectedValueOnce({ code: "WORKTREE_ALREADY_EXISTS", message: "Worktree already exists" })
        .mockRejectedValueOnce({ code: "WORKTREE_ALREADY_EXISTS", message: "Branch already exists" })
        .mockResolvedValueOnce(created);
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree.mock.calls.map((call) => call[0].worktree.slug)).toEqual([
        "try-ferryx",
        "try-ferryx-2",
        "try-ferryx-3",
      ]);
      expect(props.onTryWorktreeCreated).toHaveBeenCalledWith(created, { agentId: "codex" });
      expect(props.onFinish).toHaveBeenCalledTimes(1);
    });

    it("retries to try-ferryx-2 when only the try-ferryx branch is left behind", async () => {
      // BranchAlreadyExists serializes as WORKTREE_ALREADY_EXISTS with the branch in details.
      remoteCalls.createWorktree
        .mockRejectedValueOnce({
          code: "WORKTREE_ALREADY_EXISTS",
          message: "Branch already exists: 'ferryx/scratch/try-ferryx'",
          details: { branch: "ferryx/scratch/try-ferryx" },
        })
        .mockResolvedValueOnce(created);
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree.mock.calls.map((call) => call[0].worktree.slug)).toEqual([
        "try-ferryx",
        "try-ferryx-2",
      ]);
      expect(props.onFinish).toHaveBeenCalledTimes(1);
    });

    it("does not retry a non-collision error", async () => {
      remoteCalls.createWorktree.mockRejectedValue({ code: "GIT_ERROR", message: "a branch named x already exists" });
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree).toHaveBeenCalledTimes(1);
      expect(props.onFinish).not.toHaveBeenCalled();
    });

    it("does not retry an error without a typed code", async () => {
      remoteCalls.createWorktree.mockRejectedValue(new Error("WORKTREE_ALREADY_EXISTS in message only"));
      renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree).toHaveBeenCalledTimes(1);
    });

    it("stops at try-ferryx-9 and reports the collision", async () => {
      remoteCalls.createWorktree.mockRejectedValue({ code: "WORKTREE_ALREADY_EXISTS", message: "exists" });
      const props = renderTry();

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree.mock.calls.map((call) => call[0].worktree.slug)).toEqual([
        "try-ferryx",
        ...[2, 3, 4, 5, 6, 7, 8, 9].map((n) => `try-ferryx-${n}`),
      ]);
      expect(screen.getByTestId("onboarding-try-error").textContent).toBe("exists");
      expect(props.onFinish).not.toHaveBeenCalled();
    });

    it("still finishes and reports the failure when the agent does not start", async () => {
      remoteCalls.createWorktree.mockResolvedValue(created);
      toastMock.error.mockReset();
      const props = renderTry({
        onTryWorktreeCreated: vi.fn().mockRejectedValue({ code: "PTY_SPAWN_FAILED", message: "spawn failed" }),
      });

      await act(async () => {
        fireEvent.click(screen.getByTestId("onboarding-try-create"));
      });

      expect(remoteCalls.createWorktree).toHaveBeenCalledTimes(1);
      expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("spawn failed"));
      expect(props.onFinish).toHaveBeenCalledTimes(1);
    });

    it("starts the first launchable agent when no default is set, like the new-tab menu", () => {
      renderTry({
        defaultAgentId: null,
        agents: [{ ...agents[0], available: false }, agents[1]],
      });

      expect(screen.getByTestId("onboarding-try-create").textContent).toBe("Create a worktree and start Codex");
    });

    it("offers no create button without a local project", () => {
      renderTry({ tryProject: null });

      expect(screen.getByTestId("onboarding-try-no-project")).toBeDefined();
      expect(screen.queryByTestId("onboarding-try-create")).toBeNull();
      expect(screen.getByTestId("onboarding-next").textContent).toBe("Skip");
    });

    it("Skip on the last step finishes without recording it", () => {
      const props = renderTry({ steps: ["project", "try"] });

      fireEvent.click(screen.getByTestId("onboarding-finish"));

      expect(props.onFinish).toHaveBeenCalledTimes(1);
      expect(props.onStepCompleted).not.toHaveBeenCalled();
      expect(remoteCalls.createWorktree).not.toHaveBeenCalled();
    });

    it("keeps the footer disabled while the worktree is being created", async () => {
      let resolve: (value: typeof created) => void = () => undefined;
      remoteCalls.createWorktree.mockReturnValue(new Promise((r) => { resolve = r; }));
      renderTry();

      fireEvent.click(screen.getByTestId("onboarding-try-create"));
      expect(screen.getByTestId("onboarding-next").hasAttribute("disabled")).toBe(true);
      expect(screen.getByTestId("onboarding-back").hasAttribute("disabled")).toBe(true);
      await act(async () => {
        resolve(created);
      });
    });
  });
});
