import { render, screen, fireEvent, cleanup, within } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { saveBrowserSettings } from "../lib/browserSettings";
import { NewTabPopover } from "./NewTabPopover";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("NewTabPopover", () => {
  it("offers Windows terminal kinds from the new-tab menu", () => {
    const onNewTerminal = vi.fn();

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={onNewTerminal}
        onNewBrowser={vi.fn()}
        showWindowsTerminalKinds={true}
      />
    );

    fireEvent.change(screen.getByLabelText("Terminal shell"), { target: { value: "cmd" } });
    fireEvent.click(screen.getByText("New Terminal"));
    expect(onNewTerminal).toHaveBeenCalledWith("cmd");
  });

  it("renders an action-only menu without a search input", () => {
    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
      />
    );

    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.getByText("New Terminal")).toBeInTheDocument();
    expect(screen.getByText("New Browser Tab")).toBeInTheDocument();
    expect(screen.queryByText("New Markdown")).not.toBeInTheDocument();
    expect(screen.queryByText("New Mobile Emulator")).not.toBeInTheDocument();
    expect(screen.queryByText("Agent settings…")).not.toBeInTheDocument();
  });

  it("renders optional actions when optional props are provided", () => {
    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        onNewMarkdown={vi.fn()}
        onNewMobileEmulator={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    expect(screen.getByText("New Terminal")).toBeInTheDocument();
    expect(screen.getByText("New Browser Tab")).toBeInTheDocument();
    expect(screen.getByText("New Markdown")).toBeInTheDocument();
    expect(screen.getByText("New Mobile Emulator")).toBeInTheDocument();
    expect(screen.getByText("Agent settings…")).toBeInTheDocument();
  });

  it("calls onNewTerminal when New Terminal is clicked", () => {
    const onNewTerminal = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={onNewTerminal}
        onNewBrowser={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText("New Terminal"));
    expect(onNewTerminal).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("uses the configured new-tab URL when New Browser Tab is clicked", () => {
    saveBrowserSettings({ homePage: "https://example.com/start" });
    const onNewBrowser = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={onNewBrowser}
      />
    );

    fireEvent.click(screen.getByText("New Browser Tab"));

    expect(onNewBrowser).toHaveBeenCalledWith("https://example.com/start", undefined);
    expect(onClose).toHaveBeenCalled();
  });

  it("passes about:blank when New Browser Tab is clicked with no homepage", () => {
    const onNewBrowser = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={onNewBrowser}
      />
    );

    fireEvent.click(screen.getByText("New Browser Tab"));

    expect(onNewBrowser).toHaveBeenCalledWith("about:blank", undefined);
    expect(onClose).toHaveBeenCalled();
  });

  it("calls onNewMarkdown when New Markdown is clicked", () => {
    const onNewMarkdown = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        onNewMarkdown={onNewMarkdown}
      />
    );

    fireEvent.click(screen.getByText("New Markdown"));
    expect(onNewMarkdown).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("calls onNewMobileEmulator when New Mobile Emulator is clicked", () => {
    const onNewMobileEmulator = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        onNewMobileEmulator={onNewMobileEmulator}
      />
    );

    fireEvent.click(screen.getByText("New Mobile Emulator"));
    expect(onNewMobileEmulator).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("calls onOpenSettings when Agent settings… is clicked", () => {
    const onOpenSettings = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        onOpenSettings={onOpenSettings}
      />
    );

    fireEvent.click(screen.getByText("Agent settings…"));
    expect(onOpenSettings).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("hides agents explicitly disabled or not installed from the menu", () => {
    const onLaunchAgent = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "", enabled: true, available: true },
      { name: "codex", command: "codex", args: "", enabled: false, available: true },
      { name: "antigravity", command: "agy", args: "", enabled: true, available: false },
      { name: "opencode", command: "opencode", args: "", enabled: false, available: false },
      // omitted fields = legacy callers, stays visible
      { name: "aider", command: "aider", args: "" },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
      />,
    );

    expect(screen.getByText("Claude")).toBeInTheDocument();
    expect(screen.getByText("Aider")).toBeInTheDocument();
    expect(screen.queryByText("Codex")).not.toBeInTheDocument();
    expect(screen.queryByText("Antigravity")).not.toBeInTheDocument();
    expect(screen.queryByText("Opencode")).not.toBeInTheDocument();
  });

  it("omits the entire AGENTS section when every provided agent is disabled or unavailable", () => {
    const agents = [
      { name: "codex", command: "codex", args: "", enabled: false, available: true },
      { name: "antigravity", command: "agy", args: "", enabled: true, available: false },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
      />,
    );

    expect(screen.queryByText("AGENTS")).not.toBeInTheDocument();
  });

  it("renders real brand logo icons for agents, not a generic placeholder", () => {
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "opencode", command: "opencode", args: "" },
      { name: "cursor-agent", command: "cursor-agent", args: "" },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
      />,
    );

    // Vite inlines SVGs as data URIs, so assert the resolved agent-type mapping
    // (claude direct, opencode monochrome, cursor-agent -> cursor alias)
    const agentsSection = screen.getByText("AGENTS").parentElement as HTMLElement;
    const logoImages = Array.from(
      agentsSection.querySelectorAll('[data-testid="popover-agent-icon"]'),
    );
    const types = logoImages.map((img) => img.getAttribute("data-agent-type") ?? "");
    expect(types).toEqual(["claude", "opencode", "cursor-agent"]);
    expect(logoImages.every((img) => img.getAttribute("src")?.startsWith("data:image"))).toBe(true);
    const opencodeImg = logoImages[1];
    expect(opencodeImg.className).toContain("agent-tab-logo--monochrome");
    const claudeImg = logoImages[0];
    expect(claudeImg.className).not.toContain("agent-tab-logo--monochrome");
  });

  it("falls back to the terminal icon for an unrecognized agent name", () => {
    const agents = [{ name: "mystery-cli", command: "mystery-cli", args: "" }];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
      />,
    );

    expect(screen.getByTestId("popover-agent-terminal-icon")).toBeInTheDocument();
  });

  it("closes on Escape key", () => {
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
      />
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("renders agents group when agents prop is provided and triggers onLaunchAgent on click", () => {
    const onLaunchAgent = vi.fn();
    const onClose = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "--model sonnet" },
      { name: "aider", command: "aider", args: "" },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
      />
    );

    expect(screen.getByText("AGENTS")).toBeInTheDocument();
    expect(screen.getByText("Claude")).toBeInTheDocument();
    expect(screen.getByText("Aider")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Claude"));
    expect(onLaunchAgent).toHaveBeenCalledWith(agents[0]);
    expect(onClose).toHaveBeenCalled();
  });

  it("renders the enabled available defaultAgentId first with an accessible Default label", () => {
    const agents = [
      { name: "claude", command: "claude", args: "--model sonnet", enabled: true, available: true },
      { name: "aider", command: "aider", args: "--chat", enabled: true, available: true },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        defaultAgentId="aider"
      />,
    );

    const buttons = agentLaunchButtons();
    expect(buttons).toHaveLength(2);
    expect(buttons[0]).toHaveTextContent("Aider");
    expect(within(buttons[0]).getByText("Default")).toBeVisible();
    expect(buttons[0]).toHaveAccessibleName(/Aider/i);
    expect(buttons[0]).toHaveAccessibleName(/Default/i);
    expect(buttons[1]).toHaveTextContent("Claude");
    expect(within(buttons[1]).queryByText("Default")).not.toBeInTheDocument();
  });

  it("launches the exact clicked agent object even when a default is selected", () => {
    const onLaunchAgent = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "--model sonnet" },
      { name: "aider", command: "aider", args: "--chat" },
    ];

    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
        defaultAgentId="aider"
      />,
    );

    fireEvent.click(screen.getByText("Claude"));
    expect(onLaunchAgent).toHaveBeenCalledTimes(1);
    expect(onLaunchAgent.mock.calls[0][0]).toBe(agents[0]);

    fireEvent.click(screen.getByText("Aider"));
    expect(onLaunchAgent).toHaveBeenCalledTimes(2);
    expect(onLaunchAgent.mock.calls[1][0]).toBe(agents[1]);
  });

  it("keeps natural agent order and omits the Default badge when defaultAgentId is missing", () => {
    // omitted enabled/available fields = legacy caller shape, agents stay visible
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "aider", command: "aider", args: "" },
    ];

    const { rerender } = render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        defaultAgentId="antigravity"
      />,
    );

    expectNaturalAgentOrder(["Claude", "Aider"]);

    rerender(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        defaultAgentId={null}
      />,
    );
    expectNaturalAgentOrder(["Claude", "Aider"]);

    rerender(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={agents}
        defaultAgentId="none"
      />,
    );
    expectNaturalAgentOrder(["Claude", "Aider"]);
  });

  it("hides the default agent and omits the Default badge when the default is disabled or unavailable", () => {
    const disabledDefault = [
      { name: "claude", command: "claude", args: "", enabled: true, available: true },
      { name: "aider", command: "aider", args: "", enabled: false, available: true },
    ];
    const unavailableDefault = [
      { name: "claude", command: "claude", args: "", enabled: true, available: true },
      { name: "aider", command: "aider", args: "", enabled: true, available: false },
    ];

    const { rerender } = render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={disabledDefault}
        defaultAgentId="aider"
      />,
    );
    expectNaturalAgentOrder(["Claude"]);

    rerender(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
        agents={unavailableDefault}
        defaultAgentId="aider"
      />,
    );
    expectNaturalAgentOrder(["Claude"]);
  });
});

function agentLaunchButtons() {
  return within(screen.getByText("AGENTS").parentElement as HTMLElement).getAllByRole("button");
}

function expectNaturalAgentOrder(expectedLabels: string[]) {
  const buttons = agentLaunchButtons();
  expect(buttons.map((button) => button.textContent)).toEqual(expectedLabels);
  const agentsSection = screen.getByText("AGENTS").parentElement as HTMLElement;
  expect(within(agentsSection).queryByText("Default")).not.toBeInTheDocument();
}
