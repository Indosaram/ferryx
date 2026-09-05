import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { DesignFeedback } from "./DesignFeedback";
import { DesignSession } from "./session";
it("explicit target is required; arming subscribes; cancel releases mask and restores keyboard focus", async () => {
  const masks: boolean[] = []; let cancels = 0;
  const session = new DesignSession({ subscribe: async () => () => {}, begin: async () => {}, cancel: async () => { cancels++; }, capture: async () => { throw new Error("unused"); }, validate: async () => {} }, { stage: async () => { throw new Error("unused"); } }, { validateTarget: async () => {}, deliver: async () => { throw new Error("unused"); } });
  const identity = { browserId: "b", webviewLabel: "v", generation: "1", operationId: "o", viewportRevision: 1 };
  render(<DesignFeedback session={session} identity={identity} targets={[]} maskPreview={v => masks.push(v)} />);
  await act(async () => { fireEvent.click(screen.getByTestId("design-mode-toggle")); });
  expect(screen.getByTestId("design-mode-toggle").getAttribute("aria-pressed")).toBe("true");
  await act(async () => { fireEvent.keyDown(screen.getByRole("region", { name: "Design feedback" }), { key: "Escape" }); });
  expect(cancels).toBe(1); expect(document.activeElement).toBe(screen.getByTestId("design-mode-toggle"));
  expect(masks[masks.length - 1]).toBe(false);
});
