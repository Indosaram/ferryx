import { afterEach, expect, it, vi } from "vitest";
import { unregisterProject } from "./tauri";
import { pasteClipboardImageToRemote } from "./remoteProject";
import { reconnectAgentSession } from "./agentReconnect";
import { replaceExitedShellSession } from "./shellReplacement";
import type { TerminalSession } from "./types";

if (typeof document === "undefined") {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", { url: "http://localhost" });
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    navigator: dom.window.navigator,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
  });
  (dom.window as any).__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    unregisterCallback: () => {},
  };
}

const { cleanup, render, screen } = await import("@testing-library/react");
const { TerminalPane } = await import("../components/TerminalPane");

const native = {
  invoke: vi.fn(async (cmd?: string, _args?: unknown) => {
    if (cmd === "cmd_daemon_paste_clipboard_image") {
      return null;
    }
    return null;
  }),
};
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: (cmd: string, args?: unknown) => native.invoke(cmd, args),
}));
vi.mock("../components/NativeTerminalPane", () => ({ NativeTerminalPane: () => <div data-testid="native" /> }));
vi.mock("../components/dag/DagPaneBadge", () => ({ DagPaneBadge: () => <div data-testid="dag" /> }));
const session: TerminalSession = { id: "pane", workspaceId: `daemon:${"a".repeat(64)}`, cwd: "/srv/repo", worktree: null, backendSessionId: null, lifecycle: "exited" };
afterEach(() => { cleanup(); vi.clearAllMocks(); });
it("desktop removal does not unregister a paired project on the remote daemon", async () => {
  await unregisterProject({ workspaceId: session.workspaceId });
  expect(native.invoke).not.toHaveBeenCalled();
});
it("paired clipboard images never enter SSH upload", async () => {
  await pasteClipboardImageToRemote(session.workspaceId);
  expect(native.invoke).not.toHaveBeenCalledWith("cmd_ssh_paste_clipboard_image", expect.anything());
});
it("paired clipboard images route to daemon paste command", async () => {
  await pasteClipboardImageToRemote(session.workspaceId);
  expect(native.invoke).toHaveBeenCalledWith("cmd_daemon_paste_clipboard_image", {
    workspaceId: session.workspaceId,
  });
});
it("paired shell recovery cannot spawn a local replacement", async () => {
  const spawn = vi.fn();
  await expect(replaceExitedShellSession(session.id, { getSessions: () => ({ pane: session }), dispatch: vi.fn(), spawn })).rejects.toBeTruthy();
  expect(spawn).not.toHaveBeenCalled();
});
it("paired agent recovery cannot validate or spawn through local provider APIs", async () => {
  const spawn = vi.fn();
  const agent = { ...session, agentType: "claude", providerSession: { key: "session_id" as const, id: "remote-provider" } };
  await expect(reconnectAgentSession(agent.id, { getSessions: () => ({ pane: agent }), dispatch: vi.fn(), spawn, attach: vi.fn() })).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
  expect(spawn).not.toHaveBeenCalled();
});
it("paired recovery never mounts local terminal or DAG tooling while proxy support is absent", () => {
  render(<TerminalPane session={session} active onReconnect={vi.fn()} onOpenNewShell={vi.fn()} />);
  expect(screen.queryByTestId("native")).toBeNull();
  expect(screen.queryByTestId("dag")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByTestId("paired-terminal-unavailable")).toBeTruthy();
});
