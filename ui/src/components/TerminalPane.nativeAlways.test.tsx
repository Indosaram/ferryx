import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

function session(id = "session-native"): TerminalSession {
  return {
    id,
    cwd: "/repo/main",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

describe("TerminalPane always routes to the native terminal surface", () => {
  beforeEach(() => {
    tauriCoreMocks.invoke.mockReset().mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset().mockReturnValue(true);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("mounts the native pane for an active pane", () => {
    render(<TerminalPane session={session()} active={true} />);

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
  });

  it("mounts the native pane for an inactive split leaf", () => {
    render(<TerminalPane session={session()} active={false} />);

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
  });

  it("preserves the native pane across activity changes", () => {
    const view = render(<TerminalPane session={session()} active={false} />);
    view.rerender(<TerminalPane session={session()} active={true} />);
    view.rerender(<TerminalPane session={session()} active={false} />);

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
  });

  it("keeps two simultaneous split-leaf panes on the native surface at once", () => {
    render(
      <div>
        <TerminalPane session={session("leaf-a")} active={true} />
        <TerminalPane session={session("leaf-b")} active={false} />
      </div>,
    );

    expect(screen.getAllByTestId("native-terminal-pane")).toHaveLength(2);
  });
});
