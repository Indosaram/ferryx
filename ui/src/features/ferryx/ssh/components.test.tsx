import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { RunOn } from "./components";
afterEach(cleanup);
it("keeps removed host reference disabled rather than switching to Local", () => {
  render(<RunOn value={{ kind: "ssh", hostId: "removed" }} hosts={[]} immutable onChange={() => { throw new Error("immutable target changed"); }} />);
  expect(screen.getByTestId("run-on")).toBeDisabled();
  expect(screen.getByTestId("run-on")).toHaveAttribute("data-host-id", "removed");
});
