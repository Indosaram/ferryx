import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { StatusDot, type StatusDotState } from "./StatusDot";

function dot(state: StatusDotState): HTMLElement {
  const { container } = render(<StatusDot state={state} />);
  const el = container.querySelector<HTMLElement>("[data-status-state]");
  if (!el) throw new Error(`no status dot for ${state}`);
  return el;
}

describe("StatusDot", () => {
  afterEach(cleanup);

  it("tells starting from waiting by shape: starting is a hollow ring, waiting is filled", () => {
    const starting = dot("starting");
    cleanup();
    const waiting = dot("waiting");

    expect(starting.className).toMatch(/\bborder-status-warning\b/);
    expect(starting.className).not.toMatch(/\bbg-status-warning\b/);
    expect(waiting.className).toMatch(/\bbg-status-warning\b/);
  });

  it.each<StatusDotState>(["working", "starting", "waiting", "unread", "done", "exited", "failed"])(
    "never draws a decorative halo ring for %s",
    (state) => {
      expect(dot(state).getAttribute("class") ?? "").not.toMatch(/\bring-\d/);
    },
  );
});
