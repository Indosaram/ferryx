import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import type { AnimationEventHandler } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AttentionMascot, AttentionMascotArtwork } from "./AttentionMascot";
import * as sequence from "./mascotSequence";

let reduced = false;
let hidden = false;
let mediaListeners: Set<() => void>;
let mediaAdd: ReturnType<typeof vi.fn>;
let mediaRemove: ReturnType<typeof vi.fn>;
let observers: Observer[];

class Observer {
  readonly targets = new Set<Element>();
  readonly disconnect = vi.fn(() => this.targets.clear());
  constructor(readonly callback: IntersectionObserverCallback) { observers.push(this); }
  observe(target: Element) { this.targets.add(target); }
  unobserve(target: Element) { this.targets.delete(target); }
  emit(visible: boolean) {
    const entries = [...this.targets].map((target) => ({ target, isIntersecting: visible }));
    this.callback(entries as IntersectionObserverEntry[], this as unknown as IntersectionObserver);
  }
}

beforeEach(() => {
  reduced = false;
  hidden = false;
  observers = [];
  mediaListeners = new Set();
  mediaAdd = vi.fn((_type: string, listener: () => void) => mediaListeners.add(listener));
  mediaRemove = vi.fn((_type: string, listener: () => void) => mediaListeners.delete(listener));
  vi.stubGlobal("IntersectionObserver", Observer);
  vi.stubGlobal("matchMedia", vi.fn(() => ({
    get matches() { return reduced; },
    media: "(prefers-reduced-motion: reduce)",
    addEventListener: mediaAdd,
    removeEventListener: mediaRemove,
  })));
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => hidden ? "hidden" : "visible");
  vi.spyOn(Math, "random").mockReturnValue(0.5);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function intersect(visible: boolean) { act(() => observers.forEach((observer) => observer.emit(visible))); }
function visibility(value: boolean) {
  act(() => { hidden = value; document.dispatchEvent(new Event("visibilitychange")); });
}
function motion(value: boolean) {
  act(() => { reduced = value; mediaListeners.forEach((listener) => listener()); });
}
function stage() { return screen.getByTestId("attention-mascot-stage"); }
function cycle() { return screen.getByTestId("qa-mascot-cycle"); }
function id() { return stage().getAttribute("data-mascot-id"); }
function end(node: Element, name = "mascot-dance-cycle") {
  const event = new Event("animationend", { bubbles: true });
  Object.defineProperty(event, "animationName", { value: name });
  fireEvent(node, event);
}
function mount() { const view = render(<AttentionMascot />); intersect(true); return view; }

// Read React's actual attached host handler. Calling it bypasses delegation only,
// not the production predicate, so a retired-node test cannot pass vacuously.
function handler(node: Element) {
  const key = Object.keys(node).find((name) => name.startsWith("__reactProps$"));
  if (!key) throw new Error("React host props unavailable");
  const props = Reflect.get(node, key) as { onAnimationEnd: AnimationEventHandler<SVGGElement> };
  return props.onAnimationEnd;
}
function invoke(callback: AnimationEventHandler<SVGGElement>, node: Element) {
  callback({ target: node, currentTarget: node, animationName: "mascot-dance-cycle" } as Parameters<typeof callback>[0]);
}

describe("AttentionMascot playback", () => {
  it("advances exactly once for same-batch owned completions", () => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    mount();
    const initial = id();
    const node = cycle();
    const production = handler(node);
    const delivered = vi.fn(production);
    const propsKey = Object.keys(node).find((key) => key.startsWith("__reactProps$"));
    if (!propsKey) throw new Error("React host props unavailable");
    const props = Reflect.get(node, propsKey);
    Reflect.set(node, propsKey, { ...props, onAnimationEnd: delivered });
    act(() => {
      end(node);
      expect(cycle()).toBe(node);
      end(node);
      expect(cycle()).toBe(node);
    });
    expect(delivered).toHaveBeenCalledTimes(2);
    expect(advance).toHaveBeenCalledTimes(1);
    expect(id()).not.toBe(initial);
    expect(cycle()).not.toBe(node);
  });

  it("ignores child and unrelated wrapper animation events", () => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    mount();
    const initial = id();
    const child = cycle().querySelector(".mascot-left-arm");
    if (!child) throw new Error("Missing limb");
    end(child);
    end(cycle(), "mascot-01-body");
    expect(advance).not.toHaveBeenCalled();
    expect(id()).toBe(initial);
    end(cycle());
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it("rejects the retired wrapper's actual production closure after restart", () => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    mount();
    const retired = cycle();
    const callback = handler(retired);
    const initial = id();
    visibility(true);
    visibility(false);
    expect(cycle()).not.toBe(retired);
    act(() => invoke(callback, retired));
    expect(advance).not.toHaveBeenCalled();
    expect(id()).toBe(initial);
    act(() => invoke(handler(cycle()), cycle()));
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it.each(["user", "hidden", "offscreen", "reduced"] as const)("blocks %s and restarts the same ID when reopened", (gate) => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    mount();
    const initial = id();
    const originalStage = stage();
    const button = screen.getByRole("button");
    const originalCycle = cycle();
    const toggle = () => {
      if (gate === "user") fireEvent.click(button);
      if (gate === "hidden") visibility(!hidden);
      if (gate === "offscreen") intersect(false);
      if (gate === "reduced") motion(!reduced);
    };
    toggle();
    expect(stage()).toHaveAttribute("data-paused", "true");
    end(cycle());
    expect(advance).not.toHaveBeenCalled();
    if (gate === "offscreen") intersect(true); else toggle();
    expect(id()).toBe(initial);
    expect(advance).not.toHaveBeenCalled();
    expect(stage()).toBe(originalStage);
    expect(screen.getByRole("button")).toBe(button);
    expect(cycle()).not.toBe(originalCycle);
    expect(stage()).toHaveAttribute("data-paused", "false");
    end(cycle());
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it("blocks external pause and restarts the same ID independently of document visibility", () => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    const view = render(<AttentionMascot paused />);
    intersect(true);
    const initial = id();
    const pausedCycle = cycle();
    expect(stage()).toHaveAttribute("data-paused", "true");
    end(pausedCycle);
    expect(id()).toBe(initial);
    expect(advance).not.toHaveBeenCalled();
    expect(cycle()).toBe(pausedCycle);

    view.rerender(<AttentionMascot paused={false} />);
    expect(id()).toBe(initial);
    expect(advance).not.toHaveBeenCalled();
    expect(cycle()).not.toBe(pausedCycle);
    expect(stage()).toHaveAttribute("data-paused", "false");

    visibility(true);
    view.rerender(<AttentionMascot paused />);
    view.rerender(<AttentionMascot paused={false} />);
    const hiddenCycle = cycle();
    expect(stage()).toHaveAttribute("data-paused", "true");
    end(hiddenCycle);
    expect(id()).toBe(initial);
    expect(advance).not.toHaveBeenCalled();

    visibility(false);
    expect(id()).toBe(initial);
    expect(cycle()).not.toBe(hiddenCycle);
    expect(stage()).toHaveAttribute("data-paused", "false");
    end(cycle());
    expect(advance).toHaveBeenCalledTimes(1);
    expect(id()).not.toBe(initial);
  });

  it("keeps combined gates independent until every gate reopens", () => {
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    mount();
    const initial = id();
    fireEvent.click(screen.getByRole("button"));
    visibility(true);
    intersect(false);
    motion(true);
    motion(false);
    end(cycle());
    fireEvent.click(screen.getByRole("button"));
    end(cycle());
    visibility(false);
    end(cycle());
    expect(advance).not.toHaveBeenCalled();
    expect(id()).toBe(initial);
    expect(stage()).toHaveAttribute("data-paused", "true");
    intersect(true);
    end(cycle());
    expect(advance).toHaveBeenCalledTimes(1);
  });

  it.each([false, true])("reduced motion reserves the control and preserves paused=%s", (paused) => {
    mount();
    const button = screen.getByRole("button");
    const name = button.getAttribute("aria-label");
    if (paused) fireEvent.click(button);
    const initial = id();
    motion(true);
    expect(stage()).toHaveAttribute("data-mascot-id", initial);
    expect(stage()).not.toHaveAttribute("data-variant");
    expect(screen.getByTestId("qa-mascot-smile")).toBeInTheDocument();
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("tabindex", "-1");
    expect(button).toHaveStyle({ visibility: "hidden" });
    expect(button).toBeInTheDocument();
    motion(false);
    expect(button).toBeEnabled();
    expect(button).toHaveAttribute("aria-pressed", String(paused));
    expect(button).toHaveAttribute("aria-label", name);
    expect(button.closest('[aria-hidden="true"]')).toBeNull();
    expect(stage()).toHaveAttribute("data-paused", String(paused));
    expect(id()).toBe(initial);
  });

  it("preserves the current wrapper and ID across ordinary parent rerenders", () => {
    const view = mount();
    const node = cycle();
    const initial = id();
    const randomCalls = vi.mocked(Math.random).mock.calls.length;
    view.rerender(<AttentionMascot />);
    expect(cycle()).toBe(node);
    expect(id()).toBe(initial);
    expect(Math.random).toHaveBeenCalledTimes(randomCalls);
  });

  it("creates fresh mount-local state after remount", () => {
    const view = mount();
    const initial = id();
    end(cycle());
    expect(id()).not.toBe(initial);
    view.unmount();
    mount();
    expect(id()).toBe(initial);
  });

  it("keeps independent instances independent", () => {
    render(<><AttentionMascot /><AttentionMascot /></>);
    intersect(true);
    const stages = screen.getAllByTestId("attention-mascot-stage");
    const before = stages.map((node) => node.getAttribute("data-mascot-id"));
    end(screen.getAllByTestId("qa-mascot-cycle")[0]);
    expect(stages[0].getAttribute("data-mascot-id")).not.toBe(before[0]);
    expect(stages[1].getAttribute("data-mascot-id")).toBe(before[1]);
  });

  it("cleans every subscription during StrictMode replay and unmount", () => {
    const add = vi.spyOn(document, "addEventListener");
    const remove = vi.spyOn(document, "removeEventListener");
    const advance = vi.spyOn(sequence, "advanceMascotSequence");
    const view = render(<StrictMode><AttentionMascot /></StrictMode>);
    const added = () => add.mock.calls.filter(([type]) => type === "visibilitychange");
    const removed = () => remove.mock.calls.filter(([type]) => type === "visibilitychange");
    expect(added()).toHaveLength(2);
    expect(removed()).toHaveLength(1);
    expect(mediaAdd).toHaveBeenCalledTimes(2);
    expect(mediaRemove).toHaveBeenCalledTimes(1);
    expect(mediaListeners.size).toBe(1);
    expect(observers.filter((observer) => observer.targets.size === 1)).toHaveLength(1);
    expect(observers[0].disconnect).toHaveBeenCalledTimes(1);
    view.unmount();
    expect(removed().map(([, listener]) => listener)).toEqual(added().map(([, listener]) => listener));
    expect(mediaRemove.mock.calls).toEqual(mediaAdd.mock.calls);
    expect(mediaListeners.size).toBe(0);
    for (const observer of observers) expect(observer.disconnect).toHaveBeenCalledTimes(1);
    visibility(true);
    motion(true);
    intersect(false);
    expect(advance).not.toHaveBeenCalled();
  });

  it("keeps the explicit preview variant paused and independently renders static artwork", () => {
    const view = render(<AttentionMascot variant="07" paused />);
    expect(stage()).toHaveAttribute("data-variant", "07");
    expect(stage()).toHaveAttribute("data-paused", "true");
    end(cycle());
    expect(id()).toBe("07");
    expect(observers).toHaveLength(0);
    view.rerender(<AttentionMascotArtwork />);
    expect(stage()).not.toHaveAttribute("data-variant");
    expect(screen.queryByRole("button")).toBeNull();
  });
});
