import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNativeTerminalVisibilityState } from "../lib/nativeTerminalVisibility";
import { resetSessionLifecycleForTests } from "../lib/sessionLifecycle";
import type { TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

const state = vi.hoisted(() => ({ mac: false, mounted: 0, visible: false, visibility: [] as boolean[], invoke: vi.fn() }));
vi.mock("../lib/shortcuts", async (original) => ({
  ...await original<typeof import("../lib/shortcuts")>(),
  isMacShortcutPlatform: () => state.mac,
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: (...args: unknown[]) => state.invoke(...args) }));
vi.mock("../lib/tauri", async (original) => ({
  ...await original<typeof import("../lib/tauri")>(),
  scrollNativeTerminal: vi.fn(async () => undefined),
  setNativeTerminalDomOverlayInput: vi.fn(async () => undefined),
}));
vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: ({ session }: { session: TerminalSession }) => {
    const visibility = useNativeTerminalVisibilityState();
    const identity = useRef<string | null>(null);
    if (identity.current === null) identity.current = `mount-${++state.mounted}`;
    useEffect(() => {
      state.visible = visibility.visible;
      state.visibility.push(visibility.visible);
    }, [visibility.visible]);
    return <div data-testid="native-probe" data-identity={identity.current}
      data-backend={session.backendSessionId} data-visible={String(visibility.visible)}
      data-interactive={String(visibility.interactive)} />;
  },
}));

const session: TerminalSession = {
  id: "frontend", backendSessionId: "backend", cwd: "/repo", workspaceId: "workspace",
  worktree: { wsId: "workspace", slug: "main" }, lifecycle: "working",
};
beforeEach(() => {
  resetSessionLifecycleForTests();
  state.mac = false; state.mounted = 0; state.visible = false; state.visibility = [];
  state.invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_native_terminal_search") {
      if (!state.visible) throw new Error("Native surface is not attached");
      return { matches: [{ row: 0, startCol: 0, endCol: 5 }], totalMatches: 1 };
    }
    return undefined;
  });
});
afterEach(() => { cleanup(); resetSessionLifecycleForTests(); });

describe("terminal search and native visibility integration", () => {
  it("keeps the same non-Mac surface attached through open, query, cancel and reopen", async () => {
    const { rerender } = render(<TerminalPane session={session} active searchOpen={false} />);
    const native = screen.getByTestId("native-probe");
    const identity = native.dataset.identity;
    await act(async () => rerender(<TerminalPane session={session} active searchOpen />));
    const search = screen.getByRole("search");
    expect(search).toHaveAttribute("data-native-terminal-keep-visible", "true");
    expect(native).toHaveAttribute("data-interactive", "false");
    expect(search).toHaveAttribute("data-native-terminal-search-layout", "docked");
    expect(search).toHaveStyle({ height: "48px" });
    expect(screen.getByTestId("terminal-native-viewport")).toHaveStyle({ top: "48px" });
    expect(native).toHaveAttribute("data-visible", "true");
    expect(screen.getByTestId("terminal-search-input")).toHaveFocus();
    fireEvent.change(screen.getByTestId("terminal-search-input"), { target: { value: "marker" } });
    await waitFor(() => expect(screen.getByText("1/1")).toBeInTheDocument());
    expect(state.invoke).toHaveBeenCalledWith("cmd_native_terminal_search", {
      sessionId: "backend", query: "marker", caseSensitive: false,
    });
    await act(async () => rerender(<TerminalPane session={session} active searchOpen={false} />));
    expect(screen.getByTestId("terminal-native-viewport")).toHaveStyle({ top: "0px" });
    await act(async () => rerender(<TerminalPane session={session} active searchOpen />));
    expect(screen.getByTestId("native-probe")).toBe(native);
    expect(native.dataset.identity).toBe(identity);
    expect(native).toHaveAttribute("data-backend", "backend");
    expect(state.mounted).toBe(1);
    expect(state.visibility).not.toContain(false);
  });

  it("still yields to a separate modal while the docked search is open", async () => {
    render(<TerminalPane session={session} active searchOpen />);
    await waitFor(() => expect(screen.getByTestId("native-probe")).toHaveAttribute("data-visible", "true"));
    const dialog = document.createElement("div"); dialog.setAttribute("role", "dialog");
    try {
      await act(async () => { document.body.append(dialog); });
      expect(screen.getByTestId("native-probe")).toHaveAttribute("data-visible", "false");
      expect(screen.getByTestId("native-probe")).toHaveAttribute("data-interactive", "false");
    } finally { await act(async () => { dialog.remove(); }); }
    expect(screen.getByTestId("native-probe")).toHaveAttribute("data-visible", "true");
  });

  it("preserves macOS floating search and input occlusion without hiding the surface", async () => {
    state.mac = true;
    render(<TerminalPane session={session} active searchOpen />);
    expect(screen.getByRole("search")).not.toHaveAttribute("data-native-terminal-yield", "off");
    expect(screen.getByRole("search")).toHaveAttribute("data-native-terminal-search-layout", "floating");
    expect(screen.getByTestId("terminal-native-viewport")).toHaveStyle({ top: "0px" });
    await waitFor(() => expect(screen.getByTestId("native-probe")).toHaveAttribute("data-interactive", "false"));
    expect(screen.getByTestId("native-probe")).toHaveAttribute("data-visible", "true");
  });
});
