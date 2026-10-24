import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resolveAgentLogoByCommandName } from "../../lib/agentIcon";
import {
  AGENTS_SETTINGS_CHANGED_EVENT,
  loadAgentSettings,
  type ResolvedAgent,
} from "../../lib/agentsSettings";
import { AgentsCliStep, type AgentsCliStepProps } from "./AgentsCliStep";
import { WelcomeWizard } from "./WelcomeWizard";

const EMPTY_TITLE = "No coding agents found on your PATH.";
const EMPTY_DESCRIPTION =
  "Install one (for example Claude Code or Codex), or add a custom command below.";

function agent(name: string, available: boolean): ResolvedAgent {
  return { name, available, enabled: true, command: name, args: "", custom: false };
}

function renderStep(overrides: Partial<AgentsCliStepProps> = {}) {
  return render(<AgentsCliStep agents={[agent("claude", true)]} {...overrides} />);
}

describe("AgentsCliStep", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("renders a default-agent radiogroup with Auto plus only the installed agents", () => {
    renderStep({
      agents: [agent("claude", true), agent("codex", false)],
    });

    const group = screen.getByRole("radiogroup", { name: "Default agent" });
    const options = within(group).getAllByRole("radio");

    expect(options).toHaveLength(2);
    expect(screen.getByText("Auto")).toBeDefined();
    expect(screen.getByText("Use the first available agent.")).toBeDefined();
    expect(options[0]).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("onboarding-agent-claude")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getByTestId("onboarding-agent-claude")).toHaveTextContent("claude");
    expect(screen.queryByTestId("onboarding-agent-codex")).toBeNull();
  });

  it("renders the brand icon for a known agent and no image for an unknown one", () => {
    renderStep({
      agents: [agent("claude", true), agent("mystery", true)],
    });

    const claudeOption = screen.getByTestId("onboarding-agent-claude");
    const claudeImg = claudeOption.querySelector("img");
    expect(claudeImg).not.toBeNull();
    expect(claudeImg?.getAttribute("src")).toBe(resolveAgentLogoByCommandName("claude"));

    const unknownOption = screen.getByTestId("onboarding-agent-mystery");
    expect(unknownOption.querySelector("img")).toBeNull();
    expect(unknownOption.querySelector("svg")).not.toBeNull();
  });

  it("persists the chosen default agent immediately", () => {
    renderStep({
      agents: [agent("claude", true)],
    });

    expect(loadAgentSettings().defaultAgentId).toBeNull();

    fireEvent.click(screen.getByTestId("onboarding-agent-claude"));
    expect(loadAgentSettings().defaultAgentId).toBe("claude");
    expect(screen.getByTestId("onboarding-agent-claude")).toHaveAttribute(
      "aria-checked",
      "true",
    );

    fireEvent.click(screen.getAllByRole("radio")[0]);
    expect(loadAgentSettings().defaultAgentId).toBeNull();
    expect(screen.getAllByRole("radio")[0]).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText(/^Not installed:/)).toBeNull();
  });

  it("moves the selection with arrow keys and keeps the roving tabindex", () => {
    renderStep({
      agents: [agent("claude", true)],
    });

    const autoOption = screen.getAllByRole("radio")[0];
    expect(autoOption).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(autoOption, { key: "ArrowDown" });

    const claudeOption = screen.getByTestId("onboarding-agent-claude");
    expect(claudeOption).toHaveAttribute("aria-checked", "true");
    expect(claudeOption).toHaveAttribute("tabindex", "0");
    expect(autoOption).toHaveAttribute("tabindex", "-1");
    expect(loadAgentSettings().defaultAgentId).toBe("claude");
  });

  it("lists unavailable agents in one quiet line", () => {
    renderStep({
      agents: [agent("claude", true), agent("gjc", false), agent("aider", false)],
    });

    expect(screen.getByText("Not installed: gjc, aider")).toBeDefined();
  });

  it("shows the empty message when nothing is installed", () => {
    renderStep({ agents: [agent("codex", false)] });

    expect(screen.queryByRole("radiogroup")).toBeNull();
    expect(screen.getByText(EMPTY_TITLE)).toBeDefined();
    expect(screen.getByText(EMPTY_DESCRIPTION)).toBeDefined();
  });

  it("has no control that leaves the wizard", async () => {
    const onSkip = vi.fn();
    const onRemindLater = vi.fn();
    const onFinish = vi.fn();
    render(
      <WelcomeWizard
        steps={["agents"]}
        agents={[agent("claude", true), agent("codex", false)]}
        isMac
        onAddProject={vi.fn()}
        onConnectMachine={vi.fn()}
        onStepCompleted={vi.fn()}
        onFinish={onFinish}
        onSkip={onSkip}
        onRemindLater={onRemindLater}
      />
    );

    const wizardChrome = new Set([
      "onboarding-skip",
      "onboarding-remind-later",
      "onboarding-finish",
      "onboarding-next",
      "onboarding-back",
    ]);
    const stepButtons = screen
      .getAllByRole("button")
      .concat(screen.getAllByRole("radio"))
      .filter((button) => !wizardChrome.has(button.getAttribute("data-testid") ?? ""));
    expect(stepButtons.length).toBeGreaterThan(0);

    await act(async () => {
      for (const button of stepButtons) {
        if (button.isConnected) fireEvent.click(button);
      }
    });

    expect(screen.getByRole("dialog")).toBeDefined();
    expect(onSkip).not.toHaveBeenCalled();
    expect(onRemindLater).not.toHaveBeenCalled();
    expect(onFinish).not.toHaveBeenCalled();
  });

  it("focuses the name field and adds a custom agent as the default", () => {
    renderStep();

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));

    const nameInput = screen.getByTestId("onboarding-custom-agent-name");
    expect(document.activeElement).toBe(nameInput);

    fireEvent.change(nameInput, { target: { value: "My Agent" } });
    fireEvent.change(screen.getByTestId("onboarding-custom-agent-command"), {
      target: { value: "echo" },
    });
    fireEvent.click(screen.getByTestId("onboarding-custom-agent-add"));

    const settings = loadAgentSettings();
    expect(settings.custom.map((entry) => entry.name)).toContain("my-agent");
    expect(settings.defaultAgentId).toBe("my-agent");
    expect(screen.queryByTestId("onboarding-custom-agent-name")).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByTestId("onboarding-add-custom-agent"),
    );
  });

  it("persists arguments separately from the command in a single settings write", () => {
    renderStep();
    const listener = vi.fn();
    window.addEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);

    try {
      fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
      fireEvent.change(screen.getByTestId("onboarding-custom-agent-name"), {
        target: { value: "runner" },
      });
      fireEvent.change(screen.getByTestId("onboarding-custom-agent-command"), {
        target: { value: "my-agent" },
      });
      fireEvent.change(screen.getByTestId("onboarding-custom-agent-args"), {
        target: { value: " --continue " },
      });
      fireEvent.click(screen.getByTestId("onboarding-custom-agent-add"));

      expect(listener).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);
    }

    const settings = loadAgentSettings();
    expect(settings.custom).toEqual([
      { name: "runner", command: "my-agent", args: "--continue" },
    ]);
    expect(settings.defaultAgentId).toBe("runner");
  });

  it("submits the custom agent form with Enter", () => {
    renderStep();

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
    fireEvent.change(screen.getByTestId("onboarding-custom-agent-name"), {
      target: { value: "runner" },
    });
    const commandInput = screen.getByTestId("onboarding-custom-agent-command");
    fireEvent.change(commandInput, { target: { value: "runner --go" } });
    fireEvent.submit(commandInput.closest("form") as HTMLFormElement);

    expect(loadAgentSettings().defaultAgentId).toBe("runner");
    expect(screen.queryByTestId("onboarding-custom-agent-name")).toBeNull();
  });

  it("rejects an empty custom agent name without persisting", () => {
    renderStep();

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
    fireEvent.change(screen.getByTestId("onboarding-custom-agent-command"), {
      target: { value: "echo" },
    });
    fireEvent.click(screen.getByTestId("onboarding-custom-agent-add"));

    const alert = screen.getByRole("alert");
    expect(alert.id).not.toBe("");
    expect(screen.getByTestId("onboarding-custom-agent-name")).toHaveAttribute(
      "aria-describedby",
      alert.id,
    );
    expect(loadAgentSettings().custom).toHaveLength(0);
    expect(loadAgentSettings().defaultAgentId).toBeNull();
  });

  it("rejects a reserved built-in agent name", () => {
    renderStep();

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
    fireEvent.change(screen.getByTestId("onboarding-custom-agent-name"), {
      target: { value: "claude" },
    });
    fireEvent.change(screen.getByTestId("onboarding-custom-agent-command"), {
      target: { value: "echo" },
    });
    fireEvent.click(screen.getByTestId("onboarding-custom-agent-add"));

    expect(screen.getByRole("alert")).toBeDefined();
    expect(loadAgentSettings().custom).toHaveLength(0);
    expect(loadAgentSettings().defaultAgentId).toBeNull();
  });

  it("dispatches the agents-settings changed event on Rescan", () => {
    renderStep();
    const listener = vi.fn();
    window.addEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);

    try {
      fireEvent.click(screen.getByTestId("onboarding-rescan-agents"));
      expect(listener).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);
    }
  });

  it("closes the custom agent form on Escape without propagating the keydown", () => {
    const parentKeyDown = vi.fn();
    render(
      <div onKeyDown={parentKeyDown}>
        <AgentsCliStep
          agents={[agent("claude", true)]}
        />
      </div>
    );

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
    fireEvent.keyDown(screen.getByTestId("onboarding-custom-agent-name"), {
      key: "Escape",
    });

    expect(screen.queryByTestId("onboarding-custom-agent-name")).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByTestId("onboarding-add-custom-agent"),
    );
    expect(parentKeyDown).not.toHaveBeenCalled();
  });

  it("returns focus to the add button when the form is cancelled", () => {
    renderStep();

    fireEvent.click(screen.getByTestId("onboarding-add-custom-agent"));
    fireEvent.click(screen.getByTestId("onboarding-custom-agent-cancel"));

    expect(screen.queryByTestId("onboarding-custom-agent-name")).toBeNull();
    expect(document.activeElement).toBe(
      screen.getByTestId("onboarding-add-custom-agent"),
    );
  });
});
