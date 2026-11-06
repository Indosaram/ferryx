import { render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { LocalChatPane } from "./LocalChatPane";

const api = vi.hoisted(() => ({ identity: vi.fn(), pane: vi.fn() }));
vi.mock("../lib/desktopChatIpc", () => ({ desktopChatIdentity: api.identity, desktopChatIpcTransport: vi.fn() }));
vi.mock("./DesktopChatPane", () => ({ DesktopChatPane: (props: { session: { agentType?: string } }) => {
  api.pane(props);
  return <div>{props.session.agentType}</div>;
} }));

beforeEach(() => {
  api.pane.mockClear();
  api.identity.mockResolvedValue({ hostId: "host", ownerId: "owner", epoch: "2" });
});

it("uses the activity registry before asynchronous provider discovery fills the session", async () => {
  render(<LocalChatPane tabId="tab" leafId="leaf" active visible agentType="claude" onShowTerminal={vi.fn()}
    session={{ id: "session", cwd: "/repo", workspaceId: "local", worktree: null, lifecycle: "working",
      backendSessionId: "backend", daemonEpoch: "1", ownerId: "original-owner" }} />);
  expect(await screen.findByText("claude")).toBeInTheDocument();
  expect(api.pane).toHaveBeenLastCalledWith(expect.objectContaining({ session: expect.objectContaining({
    backendSessionId: "backend", ownerId: "original-owner", daemonEpoch: "1", agentType: "claude",
  }) }));
});
