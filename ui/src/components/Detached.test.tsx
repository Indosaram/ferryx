import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { Detached } from "./Detached";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.listen.mockReset().mockResolvedValue(() => {});
});

it("lists detached backend IDs and attaches without closing a session", async () => {
  mocks.invoke.mockImplementation(async (command, args) => command === "cmd_layout_get" ? { type: "unsupported" }
    : args.request.action === "list" ? ["cat"] : []);
  await act(async () => { render(<Detached />); });
  expect(screen.getByText("cat")).toBeTruthy();
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Attach cat" })); });
  expect(mocks.invoke.mock.calls.filter(([command]) => command === "cmd_panes")).toEqual([
    ["cmd_panes", { request: { action: "list" } }],
    ["cmd_panes", { request: { action: "attach", session: "cat", window: null, split: null } }],
  ]);
  expect(screen.queryByText("cat")).toBeNull();
});

it("attaches into the desktop's active workspace scope", async () => {
  mocks.invoke.mockImplementation(async (command, args) => command === "cmd_layout_get" ? { type: "unsupported" }
    : args.request.action === "list" ? ["cat"] : []);
  const scope = JSON.stringify(["workspace", "/repo"]);
  await act(async () => { render(<Detached scope={scope} />); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Attach cat" })); });
  expect(mocks.invoke).toHaveBeenLastCalledWith("cmd_panes", {
    request: { action: "attach", session: "cat", window: null, split: null }, scope,
  });
});

it("refreshes on the exact revision event and shows attachment failures", async () => {
  let changed: (() => void) | undefined;
  mocks.listen.mockImplementation(async (_event, callback) => { changed = callback; return () => {}; });
  let lists = 0;
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "cmd_layout_get") return { type: "unsupported" };
    if (args.request.action === "list") return lists++ === 0 ? [] : ["cat"];
    throw new Error("SESSION_NOT_FOUND");
  });
  await act(async () => { render(<Detached />); });
  await act(async () => { changed!(); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Attach cat" })); });
  expect(screen.getByRole("alert").textContent).toContain("SESSION_NOT_FOUND");
  expect(screen.getByText("cat")).toBeTruthy();
});

it("does not import an offer until explicitly opened and passes its host revision", async () => {
  mocks.invoke.mockImplementation(async (command) => command === "cmd_layout_get"
    ? { type: "ready", view: { layout: { foreignOffers: { "host-b": { revision: 900 } } } } }
    : []);
  await act(async () => { render(<Detached />); });
  expect(mocks.invoke.mock.calls.some(([command]) => command === "cmd_layout_import")).toBe(false);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: /host-b/ })); });
  expect(mocks.invoke).toHaveBeenLastCalledWith("cmd_layout_import", { machineId: "host-b", revision: 900 });
});
