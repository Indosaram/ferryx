import { afterEach, expect, it } from "vitest";
import { installDesignOverlay } from "./overlay";
import { normalizeRect, type GuestEvent } from "./model";

const identity = { browserId: "b", webviewLabel: "instance", generation: "1", operationId: "op", viewportRevision: 1 };
let dispose = () => {};
afterEach(() => { dispose(); document.body.replaceChildren(); });
it("normalizes reverse drag and clips the viewport", () => {
  expect(normalizeRect(130, 80, -10, 20, 100, 60)).toEqual({ x: 0, y: 20, width: 100, height: 40 });
  expect(() => normalizeRect(1, 1, 1, 2, 100, 60)).toThrow();
  expect(() => normalizeRect(NaN, 1, 2, 3, 100, 60)).toThrow();
});
it("selects DOM metadata, suppresses page click, removes overlay and handlers", () => {
  const button = document.createElement("button"); button.id = "sample"; document.body.append(button);
  button.getBoundingClientRect = () => ({ x: 10, y: 20, width: 120, height: 64, left: 10, top: 20, right: 130, bottom: 84, toJSON() {} });
  let clicks = 0; button.addEventListener("click", () => clicks++);
  const events: GuestEvent[] = [];
  dispose = installDesignOverlay(identity, "element", e => events.push(e));
  button.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  expect(events).toHaveLength(1);
  expect(events[0]).toMatchObject({ type: "selected", selection: { identity, rect: { x: 10, y: 20, width: 120, height: 64 }, element: { tag: "button", id: "sample", selector: "#sample" } } });
  expect(clicks).toBe(0);
  expect(document.querySelector("[data-ferryx-design-overlay]")).toBeNull();
  button.click(); expect(clicks).toBe(1);
});
it.each(["Escape", "resize", "scroll", "pagehide"])("cancels or invalidates on %s once and removes handlers", reason => {
  const events: GuestEvent[] = []; dispose = installDesignOverlay(identity, "rectangle", e => events.push(e));
  if (reason === "Escape") window.dispatchEvent(new KeyboardEvent("keydown", { key: reason }));
  else window.dispatchEvent(new Event(reason));
  window.dispatchEvent(new Event("resize"));
  expect(events).toEqual([{ type: reason === "Escape" ? "cancelled" : "invalidated", identity }]);
  expect(document.querySelector("[data-ferryx-design-overlay]")).toBeNull();
});
