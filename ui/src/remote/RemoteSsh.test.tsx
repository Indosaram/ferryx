import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { normalizeRemoteWorkspaceState } from "./RemoteSessionList";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => <div data-testid="session">{sessionId}</div>,
}));

const inventory = {
  activeContext: { workspaceId: "local", sessionId: null, terminalTabs: [] },
  projects: [{ workspaceId: "ssh:build", worktrees: [{ worktreeLabel: "repo" }] }],
  sessions: [
    { workspaceId: "ssh:build", sessionId: "ssh-one", worktreeLabel: "repo", running: true },
    { workspaceId: "ssh:build", sessionId: "ssh-two", worktreeLabel: "repo", running: true },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  localStorage.clear();
});

it("lists each SSH session separately without treating background sessions as focused", () => {
  const model = normalizeRemoteWorkspaceState(inventory);
  expect(model.context.activeTerminal).toBeNull();
  expect(model.options.filter((option) => option.sessionId).map((option) => option.sessionId))
    .toEqual(["ssh-one", "ssh-two"]);
});

it("sends the backend session identity when selecting an SSH session", async () => {
  localStorage.setItem("ferryx_remote_token", "paired");
  vi.stubGlobal("WebSocket", class {
    close() {}
  });
  const fetcher = vi.fn(async (_url: RequestInfo | URL, _options?: RequestInit) => new Response(JSON.stringify(inventory)));
  vi.stubGlobal("fetch", fetcher);
  await act(async () => { render(<RemoteApp />); });
  fireEvent.click(screen.getByRole("button", { name: "Change workspace context" }));
  const option = screen.getByRole("button", { name: /Terminal 2/ });
  await act(async () => { fireEvent.click(option); });
  const call = fetcher.mock.calls.find((args) => args.length > 1);
  expect(JSON.parse(String(call?.[1]?.body))).toEqual({ workspaceId: "ssh:build", sessionId: "ssh-two" });
  expect(screen.getByTestId("session").textContent).toBe("ssh-two");
});
