import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

import { LocalChatPane } from "./LocalChatPane";
import { PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY, PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY } from "../lib/paneChatPreferences";

const api = vi.hoisted(() => ({ identity: vi.fn(), pane: vi.fn(), stagingTransport: vi.fn() }));
vi.mock("../lib/desktopChatIpc", () => ({
  desktopChatIdentity: api.identity,
  desktopChatIpcTransport: vi.fn(),
  desktopChatFileStagingTransport: api.stagingTransport,
}));
vi.mock("./DesktopChatPane", () => ({
  DesktopChatPane: (props: Record<string, unknown>) => {
    api.pane(props);
    return <div data-testid="desktop-chat-pane-stub" />;
  },
}));

const session = {
  id: "session",
  cwd: "/repo",
  workspaceId: "local",
  worktree: null,
  lifecycle: "working" as const,
  backendSessionId: "backend",
  daemonEpoch: "1",
  ownerId: "owner",
};

const args = { tabId: "tab", leafId: "leaf", active: true, visible: true, onShowTerminal: vi.fn(), session };

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  api.pane.mockClear();
});

describe("LocalChatPane chat preferences", () => {
  it("passes the persisted Settings -> Chat preferences to the pane it mounts", async () => {
    api.identity.mockResolvedValue({ hostId: "host", ownerId: "owner", epoch: "2" });
    api.stagingTransport.mockReturnValue({ stage: vi.fn(), cancel: vi.fn(), remove: vi.fn() });
    window.localStorage.setItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY, "zen");
    window.localStorage.setItem(PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY, "everything");

    render(<LocalChatPane {...args} />);
    await screen.findByTestId("desktop-chat-pane-stub");

    expect(api.pane).toHaveBeenLastCalledWith(
      expect.objectContaining({ toolCallVisibility: "zen", announceLevel: "everything" }),
    );
  });

  it("re-reads the preference when the Settings control changes it", async () => {
    api.identity.mockResolvedValue({ hostId: "host", ownerId: "owner", epoch: "2" });
    api.stagingTransport.mockReturnValue({ stage: vi.fn(), cancel: vi.fn(), remove: vi.fn() });
    render(<LocalChatPane {...args} />);
    await screen.findByTestId("desktop-chat-pane-stub");
    expect(api.pane).toHaveBeenLastCalledWith(
      expect.objectContaining({ toolCallVisibility: "all" }),
    );

    const { saveToolCallVisibility } = await import("../lib/paneChatPreferences");
    saveToolCallVisibility("hide-successful");

    await waitFor(() =>
      expect(api.pane).toHaveBeenLastCalledWith(
        expect.objectContaining({ toolCallVisibility: "hide-successful" }),
      ),
    );
  });

  it("tells the pane whether the user is looking at it", async () => {
    api.identity.mockResolvedValue({ hostId: "host", ownerId: "owner", epoch: "2" });
    api.stagingTransport.mockReturnValue({ stage: vi.fn(), cancel: vi.fn(), remove: vi.fn() });
    render(<LocalChatPane {...args} active={false} />);
    await screen.findByTestId("desktop-chat-pane-stub");
    expect(api.pane).toHaveBeenLastCalledWith(expect.objectContaining({ active: false }));
  });
});
