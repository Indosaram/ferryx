import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PluginPane } from "./PluginPane";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));
afterEach(cleanup);

it("isolates documents without script or origin privileges and masks without remounting", async () => {
  let subscribed!: () => void;
  const subscription = new Promise<void>((resolve) => { subscribed = resolve; });
  let delivered!: () => void;
  const delivery = new Promise<void>((resolve) => { delivered = resolve; });
  bridge.listen.mockImplementation(() => { subscribed(); return Promise.resolve(vi.fn()); });
  bridge.invoke.mockImplementation(() => { delivered(); return Promise.resolve("<time>12:00</time>"); });
  const result = render(<PluginPane plugin="clock" view="main" visible />);
  await act(async () => { await subscription; await delivery; });
  const frame = screen.getByTitle("clock: main");
  expect(frame.getAttribute("sandbox")).toBe("");
  expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
  expect(frame.getAttribute("srcdoc")).toBe("<time>12:00</time>");
  result.rerender(<PluginPane plugin="clock" view="main" visible={false} />);
  expect(screen.getByTitle("clock: main")).toBe(frame);
  expect(frame.parentElement).toHaveStyle({ visibility: "hidden" });
});
