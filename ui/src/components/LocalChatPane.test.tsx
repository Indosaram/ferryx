import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { LocalChatPane, STRUCTURED_CHAT_PROVIDERS } from "./LocalChatPane";

const api = vi.hoisted(() => ({
  identity: vi.fn(),
  pane: vi.fn(),
  stagingTransport: vi.fn(),
  structuredTransport: vi.fn(),
  structuredPane: vi.fn(),
}));
vi.mock("../lib/desktopChatIpc", () => ({
  desktopChatIdentity: api.identity,
  desktopChatIpcTransport: vi.fn(),
  desktopChatFileStagingTransport: api.stagingTransport,
}));
vi.mock("../lib/structuredChatIpc", () => ({
  desktopStructuredChat: api.structuredTransport,
}));
vi.mock("../remote/chat/structured/StructuredChatPane", () => ({
  StructuredChatPane: (props: { sessionId: string; headerTitle?: string }) => {
    api.structuredPane(props);
    return <div data-testid="mock-structured-chat-pane">{props.sessionId}</div>;
  },
}));
vi.mock("./DesktopChatPane", () => ({ DesktopChatPane: (props: { session: { agentType?: string } }) => {
  api.pane(props);
  return <div>{props.session.agentType}</div>;
} }));

beforeEach(() => {
  api.pane.mockClear();
  api.structuredPane.mockClear();
  api.structuredTransport.mockReset();
  api.stagingTransport.mockClear().mockReturnValue({ stage: vi.fn(), cancel: vi.fn(), remove: vi.fn() });
  api.identity.mockResolvedValue({ hostId: "host", ownerId: "owner", epoch: "2" });
});

const paneSession = { id: "session", cwd: "/repo", workspaceId: "local", worktree: null, lifecycle: "working" as const,
  backendSessionId: "backend", daemonEpoch: "1", ownerId: "original-owner" };

it("uses the activity registry before asynchronous provider discovery fills the session", async () => {
  render(<LocalChatPane tabId="tab" leafId="leaf" active visible agentType="claude" onShowTerminal={vi.fn()}
    session={{ id: "session", cwd: "/repo", workspaceId: "local", worktree: null, lifecycle: "working",
      backendSessionId: "backend", daemonEpoch: "1", ownerId: "original-owner" }} />);
  expect(await screen.findByText("claude")).toBeInTheDocument();
  expect(api.pane).toHaveBeenLastCalledWith(expect.objectContaining({ session: expect.objectContaining({
    backendSessionId: "backend", ownerId: "original-owner", daemonEpoch: "1", agentType: "claude",
  }) }));
});

it("wires the desktop lane's file staging from the paired-paste capability", async () => {
  render(<LocalChatPane tabId="tab" leafId="leaf" active visible agentType="claude" onShowTerminal={vi.fn()}
    session={paneSession} />);
  await screen.findByText("claude");
  expect(api.pane).toHaveBeenLastCalledWith(expect.objectContaining({
    fileStaging: expect.objectContaining({ capabilities: ["pairedPasteUploadV1"] }),
  }));
});

it("stages files for the pane's backend session, not its provider name", async () => {
  render(<LocalChatPane tabId="tab" leafId="leaf" active visible agentType="codex" onShowTerminal={vi.fn()}
    session={{ ...paneSession, agentType: undefined, backendSessionId: "backend-2" }} />);
  await screen.findByText("codex");
  expect(api.stagingTransport).toHaveBeenCalledWith("backend-2");
});

it("offers no attach control for a pane with no live backend session", async () => {
  render(<LocalChatPane tabId="tab" leafId="leaf" active visible agentType="claude" onShowTerminal={vi.fn()}
    session={{ ...paneSession, backendSessionId: null }} />);
  await screen.findByText("claude");
  expect(api.stagingTransport).not.toHaveBeenCalled();
  const props = api.pane.mock.lastCall?.[0] as { fileStaging?: unknown };
  expect(props.fileStaging).toBeUndefined();
});

it("opens StructuredChatPane for supported providers and falls back cleanly on error", async () => {
  expect(STRUCTURED_CHAT_PROVIDERS.has("maho")).toBe(true);
  expect(STRUCTURED_CHAT_PROVIDERS.has("antigravity")).toBe(true);
  expect(STRUCTURED_CHAT_PROVIDERS.has("opencode")).toBe(true);
  expect(STRUCTURED_CHAT_PROVIDERS.has("cline")).toBe(true);

  api.structuredTransport
    .mockResolvedValueOnce({ ok: true, data: [] })
    .mockResolvedValueOnce({ ok: true, data: { sessionId: "sc-maho-1" } });

  render(
    <LocalChatPane
      tabId="tab"
      leafId="leaf"
      active
      visible
      agentType="maho"
      onShowTerminal={vi.fn()}
      session={paneSession}
    />,
  );
  await screen.findByText("maho");
  fireEvent.click(screen.getByTestId("local-chat-structured-toggle"));
  expect(await screen.findByTestId("mock-structured-chat-pane")).toHaveTextContent("sc-maho-1");

  fireEvent.click(screen.getByTestId("local-chat-structured-toggle"));
  expect(await screen.findByText("maho")).toBeInTheDocument();
});
