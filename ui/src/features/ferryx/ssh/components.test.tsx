import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { RunOn } from "./components";
afterEach(cleanup);
it("keeps a paired target disabled and preserves its owner without dispatch", () => {
  const onChange = vi.fn();
  render(<RunOn value={{ kind: "pairedDaemon", hostId: "relay/machine-a" }} hosts={[]} onChange={onChange} />);
  const trigger = screen.getByTestId("run-on");
  expect(trigger).toBeDisabled();
  expect(trigger).toHaveAttribute("data-host-id", "relay/machine-a");
  expect(trigger).toHaveAttribute("data-target-kind", "pairedDaemon");
  fireEvent.click(trigger);
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  expect(screen.queryByRole("option")).not.toBeInTheDocument();
  expect(onChange).not.toHaveBeenCalled();
});
it("keeps removed host reference disabled rather than switching to Local", () => {
  render(<RunOn value={{ kind: "ssh", hostId: "removed" }} hosts={[]} immutable onChange={() => { throw new Error("immutable target changed"); }} />);
  expect(screen.getByTestId("run-on")).toBeDisabled();
  expect(screen.getByTestId("run-on")).toHaveAttribute("data-host-id", "removed");
});
