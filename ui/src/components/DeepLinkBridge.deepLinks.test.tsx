import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { DeepLinkBridge } from "./DeepLinkBridge";

const transport = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: transport.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: transport.listen }));

it("deepLinks drains cold and warm delivery only after listening and requires an in-app click", async () => {
  let receive!: () => void;
  const unlisten = vi.fn();
  transport.listen.mockImplementation(async (_event: string, callback: () => void) => {
    receive = callback;
    return unlisten;
  });
  transport.invoke.mockResolvedValueOnce(["ferryx://settings/remote"])
    .mockResolvedValueOnce(["ferryx://onboarding/agents"]);
  const actions = {
    hasWorkspace: () => true, settings: vi.fn(), onboarding: vi.fn(),
    workspace: vi.fn(), toast: vi.fn(),
  };
  const view = render(<DeepLinkBridge actions={actions} />);
  await screen.findByRole("dialog");
  expect(transport.listen.mock.invocationCallOrder[0]).toBeLessThan(transport.invoke.mock.invocationCallOrder[0]);
  expect(actions.settings).not.toHaveBeenCalled();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open" })));
  expect(actions.settings).toHaveBeenCalledWith("remote");
  await act(async () => receive());
  await screen.findByRole("dialog");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Cancel" })));
  expect(actions.onboarding).not.toHaveBeenCalled();
  view.unmount();
  await act(async () => { await Promise.resolve(); });
  expect(unlisten).toHaveBeenCalled();
});
