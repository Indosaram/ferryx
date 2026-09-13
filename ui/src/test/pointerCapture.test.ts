import { afterEach, describe, expect, it } from "vitest";

function pointer(type: string, pointerId = 7) {
  return Object.assign(new MouseEvent(type, {
    bubbles: true, cancelable: true, clientX: 123, clientY: 45, buttons: 1,
  }), { pointerId, pointerType: "touch" });
}

afterEach(() => { document.body.replaceChildren(); });

describe("JSDOM pointer capture model", () => {
  it.each(["pointermove", "pointerup", "pointercancel"])("retargets captured %s before bubbling and preserves the event", (type) => {
    const owner = document.createElement("div");
    const outside = document.createElement("div");
    document.body.append(owner, outside);
    const received: Event[] = [];
    owner.addEventListener(type, (event) => { received.push(event); event.preventDefault(); });
    owner.setPointerCapture(7);
    const event = pointer(type);
    expect(outside.dispatchEvent(event)).toBe(false);
    expect(received).toEqual([event]);
    expect(event.target).toBe(owner);
    expect(event.clientX).toBe(123);
    expect(owner.hasPointerCapture(7)).toBe(type === "pointermove");
  });

  it("bubbles loss once and transfers ownership before the next move", () => {
    const parent = document.createElement("div");
    const first = document.createElement("div");
    const second = document.createElement("div");
    parent.append(first, second);
    document.body.append(parent);
    const losses: EventTarget[] = [];
    parent.addEventListener("lostpointercapture", (event) => {
      if (event.target) losses.push(event.target);
      expect(first.hasPointerCapture(7)).toBe(false);
    });
    first.setPointerCapture(7);
    first.setPointerCapture(7);
    second.setPointerCapture(7);
    expect(losses).toEqual([first]);
    first.releasePointerCapture(7);
    expect(second.hasPointerCapture(7)).toBe(true);
    const event = pointer("pointermove");
    document.body.dispatchEvent(event);
    expect(event.target).toBe(second);
    second.releasePointerCapture(7);
    second.releasePointerCapture(7);
    expect(losses).toEqual([first, second]);
  });

  it("keeps disconnected ownership queries pure so explicit cleanup cannot be hidden", () => {
    const owner = document.createElement("div");
    document.body.append(owner);
    let losses = 0;
    owner.addEventListener("lostpointercapture", () => { losses++; });
    owner.setPointerCapture(7);
    owner.remove();
    expect(owner.hasPointerCapture(7)).toBe(true);
    expect(owner.hasPointerCapture(7)).toBe(true);
    expect(losses).toBe(0);
    owner.releasePointerCapture(7);
    expect(owner.hasPointerCapture(7)).toBe(false);
    expect(losses).toBe(1);
  });
});
