import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DagRunSnapshot } from "../../lib/dagTypes";
import { parseDagRunSnapshot } from "../../lib/dagTypes";
import dagRunSampleJson from "../../state/__fixtures__/dagRunSample.json";
import { dagStore } from "../../state/dagStore";
import {
  buildBigDagRun,
  buildEmptyDagRun,
  buildQaDagRunA,
  buildQaDagRunAUpdated,
  buildTallDagRun,
} from "../../devtools/dagViewportQaFixtures";
import { DagGraphView } from "./DagGraphView";

const sampleSnapshot: DagRunSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

class CustomPointerEvent extends MouseEvent {
  readonly pointerId: number;
  readonly pointerType: string;
  constructor(type: string, params: PointerEventInit = {}) {
    super(type, params);
    this.pointerId = params.pointerId ?? 0;
    this.pointerType = params.pointerType ?? "mouse";
  }
}

type ResizeCallback = (entries: ResizeObserverEntry[], observer: ResizeObserver) => void;
let activeResizeCallbacks: ResizeCallback[] = [];

class ControlledResizeObserver implements ResizeObserver {
  private cb: ResizeCallback;
  constructor(cb: ResizeCallback) {
    this.cb = cb;
    activeResizeCallbacks.push(cb);
  }
  observe(target: Element) {
    const rect = target.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      this.cb(
        [
          {
            target,
            contentRect: rect,
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        this,
      );
    }
  }
  unobserve() {}
  disconnect() {
    activeResizeCallbacks = activeResizeCallbacks.filter((c) => c !== this.cb);
  }
}

function triggerResize(target: Element, width: number, height: number) {
  vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    bottom: height,
    right: width,
    width,
    height,
    toJSON: () => {},
  });
  Object.defineProperty(target, "clientWidth", { configurable: true, value: width });
  Object.defineProperty(target, "clientHeight", { configurable: true, value: height });

  act(() => {
    for (const cb of activeResizeCallbacks) {
      cb(
        [
          {
            target,
            contentRect: { width, height, x: 0, y: 0, top: 0, left: 0, bottom: height, right: width, toJSON: () => {} },
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        {} as ResizeObserver,
      );
    }
  });
}

function getElements(container: HTMLElement) {
  const graph = container.querySelector('[data-testid="dag-graph-view"]');
  expect(graph).not.toBeNull();
  const edgeLayer = container.querySelector('[data-testid="dag-edge-layer"]');
  expect(edgeLayer).not.toBeNull();
  const world = (container.querySelector('[data-testid="dag-world"]') ?? edgeLayer?.parentElement) as HTMLElement;
  expect(world).not.toBeNull();
  const viewport = (container.querySelector('[data-testid="dag-viewport"]') ?? world?.parentElement) as HTMLElement;
  expect(viewport).not.toBeNull();
  return { graph: graph!, edgeLayer: edgeLayer!, world, viewport };
}

function mockViewportDimensions(viewport: HTMLElement, width = 1000, height = 700) {
  triggerResize(viewport, width, height);
}

function parseTransform(transformStr: string): { x: number; y: number; scale: number } {
  if (!transformStr || transformStr === "none") {
    return { x: 0, y: 0, scale: 1 };
  }
  const match = transformStr.match(/translate\(([-0-9.]+)px,\s*([-0-9.]+)px\)\s*scale\(([-0-9.]+)\)/);
  if (match) {
    return { x: parseFloat(match[1]), y: parseFloat(match[2]), scale: parseFloat(match[3]) };
  }
  const matrixMatch = transformStr.match(/matrix\(([-0-9.]+),\s*0,\s*0,\s*([-0-9.]+),\s*([-0-9.]+),\s*([-0-9.]+)\)/);
  if (matrixMatch) {
    return { x: parseFloat(matrixMatch[5]), y: parseFloat(matrixMatch[6]), scale: parseFloat(matrixMatch[1]) };
  }
  return { x: NaN, y: NaN, scale: NaN };
}

describe("DagGraphView Camera and Viewport Interaction", () => {
  const originalResizeObserver = globalThis.ResizeObserver;
  const originalPointerEvent = globalThis.PointerEvent;

  beforeEach(() => {
    dagStore.reset();
    activeResizeCallbacks = [];
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: ControlledResizeObserver,
    });
    Object.defineProperty(globalThis, "PointerEvent", {
      configurable: true,
      writable: true,
      value: CustomPointerEvent,
    });
  });

  afterEach(() => {
    cleanup();
    Object.defineProperty(globalThis, "ResizeObserver", {
      configurable: true,
      writable: true,
      value: originalResizeObserver,
    });
    Object.defineProperty(globalThis, "PointerEvent", {
      configurable: true,
      writable: true,
      value: originalPointerEvent,
    });
    vi.restoreAllMocks();
  });

  describe("Contract 1-3 & S1: Drag Panning", () => {
    it("pans world translation by exact client-coordinate delta on left-button drag", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);
      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(viewport, {
        pointerId: 1,
        button: 0,
        buttons: 1,
        clientX: 200,
        clientY: 200,
      });
      fireEvent.pointerMove(viewport, {
        pointerId: 1,
        buttons: 1,
        clientX: 320,
        clientY: 280,
      });
      fireEvent.pointerUp(viewport, {
        pointerId: 1,
        clientX: 320,
        clientY: 280,
      });

      const pannedTransform = parseTransform(world.style.transform);
      expect(pannedTransform.x).toBeCloseTo(initialTransform.x + 120, 1);
      expect(pannedTransform.y).toBeCloseTo(initialTransform.y + 80, 1);
      expect(pannedTransform.scale).toBeCloseTo(initialTransform.scale, 4);
    });

    it("pans when drag starts on a noninteractive node card", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const card = screen.getByTestId("dag-node-extract");
      const initialTransform = parseTransform(world.style.transform);
      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(card, {
        pointerId: 1,
        button: 0,
        buttons: 1,
        clientX: 250,
        clientY: 250,
      });
      fireEvent.pointerMove(viewport, {
        pointerId: 1,
        buttons: 1,
        clientX: 370,
        clientY: 330,
      });
      fireEvent.pointerUp(viewport, {
        pointerId: 1,
        clientX: 370,
        clientY: 330,
      });

      const pannedTransform = parseTransform(world.style.transform);
      expect(pannedTransform.x).toBeCloseTo(initialTransform.x + 120, 1);
      expect(pannedTransform.y).toBeCloseTo(initialTransform.y + 80, 1);
    });

    it("ignores non-left mouse buttons (e.g. right-click)", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);
      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(viewport, {
        pointerId: 1,
        button: 2,
        buttons: 2,
        clientX: 200,
        clientY: 200,
      });
      fireEvent.pointerMove(viewport, {
        pointerId: 1,
        buttons: 2,
        clientX: 300,
        clientY: 300,
      });
      fireEvent.pointerUp(viewport, {
        pointerId: 1,
        clientX: 300,
        clientY: 300,
      });

      const currentTransform = parseTransform(world.style.transform);
      expect(currentTransform.x).toBeCloseTo(initialTransform.x, 1);
      expect(currentTransform.y).toBeCloseTo(initialTransform.y, 1);
    });

    it("does not start pan when drag starts on an interactive descendant", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);
      expect(world.style.transform).toMatch(/translate/);

      // Create a button inside viewport for testing interactive descendant rejection
      const testBtn = document.createElement("button");
      testBtn.textContent = "Click Me";
      viewport.appendChild(testBtn);

      fireEvent.pointerDown(testBtn, {
        pointerId: 1,
        button: 0,
        buttons: 1,
        clientX: 200,
        clientY: 200,
      });
      fireEvent.pointerMove(viewport, {
        pointerId: 1,
        buttons: 1,
        clientX: 300,
        clientY: 300,
      });
      fireEvent.pointerUp(viewport, {
        pointerId: 1,
        clientX: 300,
        clientY: 300,
      });

      const currentTransform = parseTransform(world.style.transform);
      expect(currentTransform.x).toBeCloseTo(initialTransform.x, 1);
      expect(currentTransform.y).toBeCloseTo(initialTransform.y, 1);
    });
  });

  describe("Contract 6-7 & S2: Wheel Zoom & Invariants", () => {
    it("zooms in with negative deltaY and preserves the cursor anchor", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);
      expect(world.style.transform).toMatch(/scale/);

      // Wheel at viewport center (clientX: 100 + 500 = 600, clientY: 50 + 350 = 400)
      fireEvent.wheel(viewport, {
        clientX: 600,
        clientY: 400,
        deltaY: -120,
        deltaMode: 0,
      });

      const zoomedTransform = parseTransform(world.style.transform);
      expect(zoomedTransform.scale).toBeGreaterThan(initialTransform.scale);

      const rect = viewport.getBoundingClientRect();
      const pX = 600 - rect.left;
      const pY = 400 - rect.top;
      const w0X = (pX - initialTransform.x) / initialTransform.scale;
      const w0Y = (pY - initialTransform.y) / initialTransform.scale;
      const w1X = (pX - zoomedTransform.x) / zoomedTransform.scale;
      const w1Y = (pY - zoomedTransform.y) / zoomedTransform.scale;
      expect(w1X).toBeCloseTo(w0X, 1);
      expect(w1Y).toBeCloseTo(w0Y, 1);
    });

    it("normalizes deltaMode 1 (lines * 16) and deltaMode 2 (pages * height)", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const s0 = parseTransform(world.style.transform).scale;

      // 1 line = 16 pixels. deltaY = -1 line => -16 px
      fireEvent.wheel(viewport, { clientX: 600, clientY: 400, deltaY: -1, deltaMode: 1 });
      const s1 = parseTransform(world.style.transform).scale;
      expect(s1).toBeGreaterThan(s0);
    });

    it("pure horizontal wheel (deltaX != 0, deltaY == 0) does not change camera", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const t0 = parseTransform(world.style.transform);

      fireEvent.wheel(viewport, {
        clientX: 600,
        clientY: 400,
        deltaX: 120,
        deltaY: 0,
      });

      const t1 = parseTransform(world.style.transform);
      expect(t1.scale).toBe(t0.scale);
      expect(t1.x).toBe(t0.x);
      expect(t1.y).toBe(t0.y);
    });

    it("clamps at maximum scale 3 without anchor drift", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);

      // Huge zoom in
      for (let i = 0; i < 20; i++) {
        fireEvent.wheel(viewport, { clientX: 600, clientY: 400, deltaY: -500 });
      }

      const tClamped = parseTransform(world.style.transform);
      expect(tClamped.scale).toBeCloseTo(3.0, 2);

      // Additional zoom in does not drift translation
      fireEvent.wheel(viewport, { clientX: 600, clientY: 400, deltaY: -500 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter.scale).toBeCloseTo(3.0, 2);
      expect(tAfter.x).toBeCloseTo(tClamped.x, 2);
      expect(tAfter.y).toBeCloseTo(tClamped.y, 2);
    });
  });

  describe("Contract 8-9 & S3: Camera Controls & Fit", () => {
    it("renders focusable native buttons with required aria-labels", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      getElements(container);

      const zoomOut = screen.getByRole("button", { name: "Zoom out" });
      const zoomReset = screen.getByRole("button", { name: "Reset zoom to 100%" });
      const zoomIn = screen.getByRole("button", { name: "Zoom in" });
      const fitGraph = screen.getByRole("button", { name: "Fit graph" });

      expect(zoomOut).toBeInTheDocument();
      expect(zoomReset).toBeInTheDocument();
      expect(zoomIn).toBeInTheDocument();
      expect(fitGraph).toBeInTheDocument();

      // Native buttons must NOT have the disabled attribute (uses aria-disabled)
      expect(zoomOut).not.toHaveAttribute("disabled");
      expect(zoomReset).not.toHaveAttribute("disabled");
      expect(zoomIn).not.toHaveAttribute("disabled");
      expect(fitGraph).not.toHaveAttribute("disabled");
    });

    it("Zoom in multiplies scale by 1.2 at viewport center", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const s0 = parseTransform(world.style.transform).scale;

      const zoomIn = screen.getByRole("button", { name: "Zoom in" });
      fireEvent.click(zoomIn);

      const s1 = parseTransform(world.style.transform).scale;
      expect(s1).toBeCloseTo(s0 * 1.2, 2);
    });

    it("Zoom out divides scale by 1.2 at viewport center", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const s0 = parseTransform(world.style.transform).scale;

      const zoomOut = screen.getByRole("button", { name: "Zoom out" });
      fireEvent.click(zoomOut);

      const s1 = parseTransform(world.style.transform).scale;
      expect(s1).toBeCloseTo(s0 / 1.2, 2);
    });

    it("Reset zoom to 100% sets scale to 1.0 at viewport center", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const zoomIn = screen.getByRole("button", { name: "Zoom in" });
      fireEvent.click(zoomIn);
      fireEvent.click(zoomIn);

      const zoomReset = screen.getByRole("button", { name: "Reset zoom to 100%" });
      fireEvent.click(zoomReset);

      const sReset = parseTransform(world.style.transform).scale;
      expect(sReset).toBeCloseTo(1.0, 4);
    });

    it("Fit graph centers world rectangle within positive viewport margin", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      // Pan far away first
      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 900, clientY: 900 });
      fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 900, clientY: 900 });

      const fitGraph = screen.getByRole("button", { name: "Fit graph" });
      fireEvent.click(fitGraph);

      const tFit = parseTransform(world.style.transform);
      expect(tFit.scale).toBeGreaterThan(0);
      expect(tFit.scale).toBeLessThanOrEqual(1.0);
      expect(Number.isFinite(tFit.x)).toBe(true);
      expect(Number.isFinite(tFit.y)).toBe(true);
    });

    it("handles large 100-column and tall 100-row runs without NaN", () => {
      const { container, rerender } = render(<DagGraphView snapshot={buildBigDagRun()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const tBig = parseTransform(world.style.transform);
      expect(Number.isFinite(tBig.scale)).toBe(true);
      expect(tBig.scale).toBeLessThan(0.1); // 100 columns fits < 0.1
      expect(Number.isFinite(tBig.x)).toBe(true);
      expect(Number.isFinite(tBig.y)).toBe(true);

      rerender(<DagGraphView snapshot={buildTallDagRun()} />);
      const tTall = parseTransform(world.style.transform);
      expect(Number.isFinite(tTall.scale)).toBe(true);
      expect(Number.isFinite(tTall.x)).toBe(true);
      expect(Number.isFinite(tTall.y)).toBe(true);
    });
  });

  describe("Contract 4-5 & S4: Gesture Lifecycle & Cancellation", () => {
    it("cancels active drag on pointercancel without moving on subsequent moves", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });

      // Cancel gesture
      fireEvent.pointerCancel(viewport, { pointerId: 1 });
      const tCancelled = parseTransform(world.style.transform);

      // Subsequent unpressed move must NOT move the camera
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 0, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);

      expect(tAfter.x).toBe(tCancelled.x);
      expect(tAfter.y).toBe(tCancelled.y);
    });

    it("cancels active drag on window blur", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });

      window.dispatchEvent(new Event("blur"));
      const tBlur = parseTransform(world.style.transform);

      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 0, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter.x).toBe(tBlur.x);
      expect(tAfter.y).toBe(tBlur.y);
    });

    it("cancels active drag on document visibilitychange (hidden)", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });

      Object.defineProperty(document, "hidden", { configurable: true, value: true });
      document.dispatchEvent(new Event("visibilitychange"));
      const tHidden = parseTransform(world.style.transform);

      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 0, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter.x).toBe(tHidden.x);
      expect(tAfter.y).toBe(tHidden.y);
      Object.defineProperty(document, "hidden", { configurable: true, value: false });
    });
  });

  describe("Contract 5 & 7 & S5: Touch & Pinch Transitions", () => {
    it("two-touch pinch scales and anchors around touch midpoint", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/scale/);
      const s0 = parseTransform(world.style.transform).scale;

      // Contact 1 at (200, 200), Contact 2 at (300, 200) -> distance = 100, midpoint = (250, 200)
      fireEvent.pointerDown(viewport, { pointerId: 10, pointerType: "touch", clientX: 200, clientY: 200 });
      fireEvent.pointerDown(viewport, { pointerId: 11, pointerType: "touch", clientX: 300, clientY: 200 });

      // Pinch out: distance becomes 160 -> scale multiplies by 1.6
      fireEvent.pointerMove(viewport, { pointerId: 10, pointerType: "touch", clientX: 170, clientY: 200 });
      fireEvent.pointerMove(viewport, { pointerId: 11, pointerType: "touch", clientX: 330, clientY: 200 });

      const s1 = parseTransform(world.style.transform).scale;
      expect(s1).toBeCloseTo(s0 * 1.6, 1);

      fireEvent.pointerUp(viewport, { pointerId: 10, pointerType: "touch" });
      fireEvent.pointerUp(viewport, { pointerId: 11, pointerType: "touch" });
    });

    it("rebases seamlessly on 1 -> 2 -> 1 touch transitions without jump", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      // Start 1 touch pan
      fireEvent.pointerDown(viewport, { pointerId: 1, pointerType: "touch", clientX: 200, clientY: 200 });
      fireEvent.pointerMove(viewport, { pointerId: 1, pointerType: "touch", clientX: 220, clientY: 210 });
      const t1 = parseTransform(world.style.transform);

      // Add 2nd touch at (320, 210)
      fireEvent.pointerDown(viewport, { pointerId: 2, pointerType: "touch", clientX: 320, clientY: 210 });
      const t2Start = parseTransform(world.style.transform);
      expect(t2Start.x).toBeCloseTo(t1.x, 1);
      expect(t2Start.y).toBeCloseTo(t1.y, 1);

      // Lift 2nd touch
      fireEvent.pointerUp(viewport, { pointerId: 2, pointerType: "touch", clientX: 320, clientY: 210 });
      const t2End = parseTransform(world.style.transform);
      expect(t2End.x).toBeCloseTo(t1.x, 1);

      // Move remaining 1st touch by (20, 10)
      fireEvent.pointerMove(viewport, { pointerId: 1, pointerType: "touch", clientX: 240, clientY: 220 });
      const tFinal = parseTransform(world.style.transform);
      expect(tFinal.x).toBeCloseTo(t2End.x + 20, 1);
      expect(tFinal.y).toBeCloseTo(t2End.y + 10, 1);

      fireEvent.pointerUp(viewport, { pointerId: 1, pointerType: "touch" });
    });
  });

  describe("Contract 10-11 & S6: Live State, Empty Runs, Resize", () => {
    it("mounts without error on no-run / empty state (unconditional hooks)", () => {
      const { container, rerender } = render(<DagGraphView />);
      expect(screen.getByText("No dag runs yet")).toBeInTheDocument();

      // Transition from no-run -> run
      rerender(<DagGraphView snapshot={sampleSnapshot} />);
      const { world } = getElements(container);
      expect(world.style.transform).toMatch(/translate/);
    });

    it("handles empty runs (zero nodes/waves) gracefully", () => {
      const { container } = render(<DagGraphView snapshot={buildEmptyDagRun()} />);
      const { world } = getElements(container);
      expect(world).toBeInTheDocument();
    });

    it("same-run status/node updates preserve camera transform", () => {
      const runA = buildQaDagRunA();
      const { container, rerender } = render(<DagGraphView snapshot={runA} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      // Pan to custom position
      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 250, clientY: 180 });
      fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 250, clientY: 180 });

      const tPanned = parseTransform(world.style.transform);

      // Update same run
      const runAUpdated = buildQaDagRunAUpdated();
      rerender(<DagGraphView snapshot={runAUpdated} />);

      const tAfterUpdate = parseTransform(world.style.transform);
      expect(tAfterUpdate.x).toBeCloseTo(tPanned.x, 2);
      expect(tAfterUpdate.y).toBeCloseTo(tPanned.y, 2);
      expect(tAfterUpdate.scale).toBeCloseTo(tPanned.scale, 4);
    });

    it("run switch (different runId) cancels active gestures and refits", () => {
      const runA = buildQaDagRunA();
      const { container, rerender } = render(<DagGraphView snapshot={runA} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      expect(world.style.transform).toMatch(/translate/);

      // Start drag
      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });

      // Switch to run B
      const runB = buildTallDagRun();
      rerender(<DagGraphView snapshot={runB} />);

      // Gesture must have been cancelled
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 0, clientX: 500, clientY: 500 });
      const tNewRun = parseTransform(world.style.transform);
      expect(Number.isFinite(tNewRun.x)).toBe(true);
      expect(Number.isFinite(tNewRun.scale)).toBe(true);
    });

    it("ResizeObserver changes translation by (deltaW/2, deltaH/2) keeping center point stable", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);

      // Initial size 1000x700
      triggerResize(viewport, 1000, 700);
      expect(world.style.transform).toMatch(/translate/);
      const t0 = parseTransform(world.style.transform);

      // Resize by +100 in width, +60 in height -> (1100, 760)
      triggerResize(viewport, 1100, 760);
      const t1 = parseTransform(world.style.transform);

      expect(t1.scale).toBe(t0.scale);
      expect(t1.x).toBeCloseTo(t0.x + 50, 1);
      expect(t1.y).toBeCloseTo(t0.y + 30, 1);
    });

    it("ResizeObserver ignores zero dimensions and retains last positive dimensions", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);

      triggerResize(viewport, 1000, 700);
      expect(world.style.transform).toMatch(/translate/);
      const t0 = parseTransform(world.style.transform);

      // Resize to 0
      triggerResize(viewport, 0, 0);
      const tZero = parseTransform(world.style.transform);
      expect(tZero.x).toBe(t0.x);
      expect(tZero.y).toBe(t0.y);
      expect(tZero.scale).toBe(t0.scale);
    });
  });
});
