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
  mocks.invoke.mockResolvedValueOnce(["cat"]).mockResolvedValueOnce([]);
  await act(async () => { render(<Detached />); });
  expect(screen.getByText("cat")).toBeTruthy();
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Attach cat" })); });
  expect(mocks.invoke.mock.calls).toEqual([
    ["cmd_panes", { request: { action: "list" } }],
    ["cmd_panes", { request: { action: "attach", session: "cat", window: null, split: null } }],
  ]);
  expect(screen.queryByText("cat")).toBeNull();
});

it("refreshes on the exact revision event and shows attachment failures", async () => {
  let changed: (() => void) | undefined;
  mocks.listen.mockImplementation(async (_event, callback) => { changed = callback; return () => {}; });
  mocks.invoke.mockResolvedValueOnce([]).mockResolvedValueOnce(["cat"]).mockRejectedValueOnce(new Error("SESSION_NOT_FOUND"));
  await act(async () => { render(<Detached />); });
  await act(async () => { changed!(); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Attach cat" })); });
  expect(screen.getByRole("alert").textContent).toContain("SESSION_NOT_FOUND");
  expect(screen.getByText("cat")).toBeTruthy();
});
