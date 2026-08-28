import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadAgentSettings } from "../../lib/agentsSettings";
import { AgentsSection } from "./AgentsSection";

const native = vi.hoisted(() => ({
  detectAgents: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  detectAgents: native.detectAgents,
}));

describe("AgentsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    native.detectAgents.mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
  });

  it("surfaces agent detection failure with an alert message", async () => {
    native.detectAgents.mockRejectedValue(new Error("detect boom"));

    render(<AgentsSection />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("detect boom");
  });

  it("probes every built-in candidate command on mount", async () => {
    render(<AgentsSection />);

    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());
    expect(native.detectAgents.mock.calls[0][0]).toContain("claude");
    expect(native.detectAgents.mock.calls[0][0]).toContain("codex");
  });

  it("marks a detected agent and counts it", async () => {
    native.detectAgents.mockResolvedValue([{ name: "claude", available: true }]);

    render(<AgentsSection />);

    expect(await screen.findByText("1 detected")).toBeInTheDocument();
    expect(screen.getAllByText("Detected")).toHaveLength(1);
  });

  it("registers a custom agent, persists it, and re-probes its command", async () => {
    render(<AgentsSection />);
    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Add custom agent" }));
    fireEvent.change(screen.getByLabelText("Custom agent name"), {
      target: { value: "My Agent" },
    });
    fireEvent.change(screen.getByLabelText("Custom agent command"), {
      target: { value: "my-agent-cli" },
    });
    fireEvent.change(screen.getByLabelText("Custom agent arguments"), {
      target: { value: "--resume" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add Agent" }));

    await waitFor(() =>
      expect(loadAgentSettings().custom).toEqual([
        { name: "my-agent", command: "my-agent-cli", args: "--resume" },
      ]),
    );

    expect(screen.getByText("1 registered")).toBeInTheDocument();
    expect(screen.getByText("my-agent-cli --resume")).toBeInTheDocument();

    await waitFor(() => {
      const probed = native.detectAgents.mock.calls.at(-1)?.[0] as string[];
      expect(probed).toContain("my-agent-cli");
    });
  });

  it("blocks a custom agent that collides with a built-in name", async () => {
    render(<AgentsSection />);
    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Add custom agent" }));
    fireEvent.change(screen.getByLabelText("Custom agent name"), {
      target: { value: "Claude" },
    });
    fireEvent.change(screen.getByLabelText("Custom agent command"), {
      target: { value: "claude" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add Agent" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That name is reserved for a built-in agent.",
    );
    expect(loadAgentSettings().custom).toEqual([]);
  });

  it("removes a registered custom agent", async () => {
    render(<AgentsSection />);
    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Add custom agent" }));
    fireEvent.change(screen.getByLabelText("Custom agent name"), {
      target: { value: "my-agent" },
    });
    fireEvent.change(screen.getByLabelText("Custom agent command"), {
      target: { value: "my-agent-cli" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add Agent" }));

    await waitFor(() => expect(loadAgentSettings().custom).toHaveLength(1));

    fireEvent.click(screen.getByRole("button", { name: "Remove My-agent" }));

    await waitFor(() => expect(loadAgentSettings().custom).toEqual([]));
    expect(screen.getByText("No custom agents registered yet.")).toBeInTheDocument();
  });
});
