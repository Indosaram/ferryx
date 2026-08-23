import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsDialog } from "./SettingsDialog";

vi.mock("../lib/terminalSettings", () => ({
  useTerminalSettings: () => ({
    settings: {
      fontFamily: "monospace",
      fontSize: 13,
      scrollback: 10_000,
      macosOptionAsAlt: false,
      fontFamilySource: "fallback",
      macosOptionAsAltSource: "fallback",
    },
    nativePreferences: {
      source: "fallback",
      status: "fallback",
      sourcePath: null,
    },
    updateSettings: vi.fn(),
    refreshNativePreferences: vi.fn(),
  }),
}));

afterEach(cleanup);

describe("SettingsDialog workspace section", () => {
  it("renders live project/worktree state and wires workspace actions", () => {
    const onClose = vi.fn();
    const onSelectProject = vi.fn();
    const onAddProject = vi.fn();
    const onAddWorktree = vi.fn();
    const projects = [
      { workspaceId: "default", repoRoot: "/repo/default" },
      { workspaceId: "frontend", repoRoot: "/repo/frontend" },
    ];
    const activeWorktree = {
      path: "/repo/default",
      head: "abc123",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    render(
      <SettingsDialog
        open
        onClose={onClose}
        projects={projects}
        activeProjectId="default"
        activeWorktree={activeWorktree}
        onSelectProject={onSelectProject}
        onAddProject={onAddProject}
        onAddWorktree={onAddWorktree}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));

    expect(screen.getAllByText("/repo/default")).toHaveLength(2);
    expect(screen.getByText("/repo/frontend")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Active" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));
    fireEvent.click(screen.getByRole("button", { name: "Add Worktree" }));
    fireEvent.click(screen.getByRole("button", { name: "Select" }));

    expect(onAddProject).toHaveBeenCalledTimes(1);
    expect(onAddWorktree).toHaveBeenCalledTimes(1);
    expect(onSelectProject).toHaveBeenCalledWith(projects[1]);
  });

  it("does not render workspace section or dialogue content when open=false", () => {
    const { container } = render(
      <SettingsDialog
        open={false}
        onClose={vi.fn()}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
