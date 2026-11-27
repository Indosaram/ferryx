import { describe, expect, it, vi } from "vitest";
import {
  animatePageScroll,
  pageScrollDistancePx,
  pageScrollTarget,
  timelinePageScrollKey,
  type PageScrollEnv,
} from "./pageScroll";

const metrics = { clientHeight: 300, scrollHeight: 1200, scrollTop: 400 };
const maxScrollTop = metrics.scrollHeight - metrics.clientHeight;

const plainKey = { key: "PageDown", defaultPrevented: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };

describe("timelinePageScrollKey", () => {
  it("accepts a bare PageUp/PageDown the timeline can honour", () => {
    expect(timelinePageScrollKey({ ...plainKey }, metrics)).toBe("PageDown");
    expect(timelinePageScrollKey({ ...plainKey, key: "PageUp" }, metrics)).toBe("PageUp");
  });

  it("refuses modified, already-handled and boundary keys", () => {
    expect(timelinePageScrollKey({ ...plainKey, defaultPrevented: true }, metrics)).toBeNull();
    expect(timelinePageScrollKey({ ...plainKey, shiftKey: true }, metrics)).toBeNull();
    expect(timelinePageScrollKey({ ...plainKey, metaKey: true }, metrics)).toBeNull();
    expect(timelinePageScrollKey({ ...plainKey, key: "PageUp" }, { ...metrics, scrollTop: 0 })).toBeNull();
    expect(timelinePageScrollKey({ ...plainKey }, { ...metrics, scrollTop: maxScrollTop })).toBeNull();
  });

  it("refuses IME-buffered keydowns, matching imeKeys.ts", () => {
    expect(timelinePageScrollKey({ ...plainKey, isComposing: true }, metrics)).toBeNull();
    expect(timelinePageScrollKey({ ...plainKey, keyCode: 229 }, metrics)).toBeNull();
  });
});

describe("page scroll target", () => {
  it("pages by the viewport minus the alignment inset", () => {
    expect(pageScrollDistancePx(300)).toBe(264);
    expect(pageScrollTarget(metrics, "PageDown", 0)).toBe(400 + 264);
    expect(pageScrollTarget(metrics, "PageUp", 0)).toBe(400 - 264);
  });

  it("clamps into the document range", () => {
    expect(pageScrollTarget({ ...metrics, scrollTop: 900 }, "PageDown", 0)).toBe(maxScrollTop);
    expect(pageScrollTarget({ ...metrics, scrollTop: 50 }, "PageUp", 0)).toBe(0);
  });

  it("accelerates while the key is held, up to double the page", () => {
    const headroom = { clientHeight: 300, scrollHeight: 1200, scrollTop: 100 };
    expect(pageScrollTarget(headroom, "PageDown", 400)).toBe(100 + 2 * 264);
    expect(pageScrollTarget(headroom, "PageDown", 200)).toBe(100 + 264 * 1.5);
  });
});

describe("animatePageScroll", () => {
  it("jumps immediately for a reduced-motion reader", () => {
    const scrollTo = vi.fn();
    animatePageScroll({ ...metrics, scrollTo }, "PageDown", true, {
      now: () => 0,
      requestFrame: () => 0,
      cancelFrame: () => undefined,
    });
    expect(scrollTo).toHaveBeenCalledWith({ top: 400 + pageScrollDistancePx(300) });
  });

  it("animates along the eased path without fake timers", () => {
    const scrollTo = vi.fn();
    const frames: (() => void)[] = [];
    let clock = 0;
    const driver: PageScrollEnv = {
      now: () => clock,
      requestFrame: (callback: () => void) => {
        frames.push(callback);
        return frames.length;
      },
      cancelFrame: () => undefined,
    };
    const cancel = animatePageScroll({ ...metrics, scrollTo }, "PageDown", false, driver);
    const startTop = 400;
    const target = pageScrollTarget(metrics, "PageDown", 0);
    // Drive the scheduled animation frames with advancing time; no fake timers.
    while (frames.length > 0) {
      clock += 50;
      for (const frame of frames.splice(0)) frame();
    }
    cancel();
    const writes = scrollTo.mock.calls.map((call) => (call[0] as { top: number }).top);
    expect(writes.length).toBeGreaterThanOrEqual(2);
    expect(writes[0]).toBeGreaterThan(startTop);
    expect(writes[0]).toBeLessThan(target);
    expect(writes[writes.length - 1]).toBeCloseTo(target, 5);
  });
});
