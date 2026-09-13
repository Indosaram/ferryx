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
let activeResizeObservers: ControlledResizeObserver[] = [];

class ControlledResizeObserver implements ResizeObserver {
  readonly targets = new Set<Element>();
  readonly cb: ResizeCallback;
  constructor(cb: ResizeCallback) {
    this.cb = cb;
    activeResizeObservers.push(this);
  }
  observe(target: Element) {
    this.targets.add(target);
    // In real browsers, ResizeObserver delivery is asynchronous; tests drive sizing
    // via triggerResize / mockViewportDimensions.
  }
  unobserve(target: Element) { this.targets.delete(target); }
  disconnect() {
    this.targets.clear();
    activeResizeObservers = activeResizeObservers.filter((observer) => observer !== this);
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
    for (const observer of activeResizeObservers) {
      if (!observer.targets.has(target)) continue;
      observer.cb(
        [
          {
            target,
            contentRect: { width, height, x: 0, y: 0, top: 0, left: 0, bottom: height, right: width, toJSON: () => {} },
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: [],
          } as unknown as ResizeObserverEntry,
        ],
        observer,
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
    activeResizeObservers = [];
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

    it("rejects pan from all [contenteditable] attribute variants (B4 regression)", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);

      // Test variants: "", "plaintext-only", "true", "false"
      const variants = ["", "plaintext-only", "true", "false"];
      for (let i = 0; i < variants.length; i++) {
        const variant = variants[i];
        const editable = document.createElement("div");
        editable.setAttribute("contenteditable", variant);
        editable.textContent = `Editable ${variant}`;
        viewport.appendChild(editable);

        fireEvent.pointerDown(editable, {
          pointerId: 30 + i,
          button: 0,
          buttons: 1,
          clientX: 200,
          clientY: 200,
        });
        fireEvent.pointerMove(viewport, {
          pointerId: 30 + i,
          buttons: 1,
          clientX: 250,
          clientY: 250,
        });
        fireEvent.pointerUp(viewport, {
          pointerId: 30 + i,
          clientX: 250,
          clientY: 250,
        });

        const tAfter = parseTransform(world.style.transform);
        expect(tAfter.x).toBeCloseTo(initialTransform.x, 1);
        expect(tAfter.y).toBeCloseTo(initialTransform.y, 1);

        editable.remove();
      }
    });

    it("does not start pan when drag starts on an interactive descendant", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const initialTransform = parseTransform(world.style.transform);

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

      const s0 = parseTransform(world.style.transform).scale;

      // 1 line = 16 pixels. deltaY = -1 line => -16 px => factor = exp(0.032)
      fireEvent.wheel(viewport, { clientX: 600, clientY: 400, deltaY: -1, deltaMode: 1 });
      const s1 = parseTransform(world.style.transform).scale;
      expect(s1 / s0).toBeCloseTo(Math.exp(0.032), 4);

      // deltaMode 2 (pages * height): deltaY = -0.1 page => -70 px => multiplier = exp(0.14)
      const sBeforePage = s1;
      fireEvent.wheel(viewport, { clientX: 600, clientY: 400, deltaY: -0.1, deltaMode: 2 });
      const sAfterPage = parseTransform(world.style.transform).scale;
      expect(sAfterPage / sBeforePage).toBeCloseTo(Math.exp(0.14), 4);
    });

    it.each([
      { deltaMode: 0, ctrlKey: false, deltaY: -112 },
      { deltaMode: 0, ctrlKey: true, deltaY: -112 },
      { deltaMode: 1, ctrlKey: false, deltaY: -7 },
      { deltaMode: 1, ctrlKey: true, deltaY: -7 },
      { deltaMode: 2, ctrlKey: false, deltaY: -0.16 },
      { deltaMode: 2, ctrlKey: true, deltaY: -0.16 },
    ])("deltaMode $deltaMode with ctrlKey $ctrlKey matches equivalent pixel zoom at an off-center anchor", ({ deltaMode, ctrlKey, deltaY }) => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);
      const initial = parseTransform(world.style.transform);
      const anchor = { x: 230, y: 170 };
      const worldAnchor = {
        x: (anchor.x - initial.x) / initial.scale,
        y: (anchor.y - initial.y) / initial.scale,
      };
      // Independent contract oracle: -112 px = -7 lines * 16 = -0.16 pages * 700.
      const equivalentPixels = -112;
      const expectedScale = initial.scale * Math.exp(-0.002 * equivalentPixels);
      const wheel = new WheelEvent("wheel", {
        bubbles: true,
        cancelable: true,
        clientX: anchor.x,
        clientY: anchor.y,
        deltaY,
        deltaMode,
        ctrlKey,
      });
      const ancestorWheel = vi.fn();
      container.addEventListener("wheel", ancestorWheel);
      try {
        expect(fireEvent(viewport, wheel)).toBe(false);
        const zoomed = parseTransform(world.style.transform);
        expect(zoomed.scale).toBeCloseTo(expectedScale, 8);
        expect(zoomed.x).toBeCloseTo(anchor.x - expectedScale * worldAnchor.x, 8);
        expect(zoomed.y).toBeCloseTo(anchor.y - expectedScale * worldAnchor.y, 8);
        expect(zoomed.x + zoomed.scale * worldAnchor.x).toBeCloseTo(anchor.x, 8);
        expect(zoomed.y + zoomed.scale * worldAnchor.y).toBeCloseTo(anchor.y, 8);
        expect(wheel.defaultPrevented).toBe(true);
        expect(ancestorWheel).not.toHaveBeenCalled();
      } finally {
        container.removeEventListener("wheel", ancestorWheel);
      }
    });

    it("pure horizontal wheel (deltaX != 0, deltaY == 0) does not change camera", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

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

    it("current measured viewport drives button minimum and aria state without stale memo (B3 regression)", () => {
      const bigRun = buildBigDagRun();
      const { container } = render(<DagGraphView snapshot={bigRun} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const tInitial = parseTransform(world.style.transform);
      expect(tInitial.scale).toBeLessThan(0.1);

      const zoomInBtn = screen.getByRole("button", { name: "Zoom in" });
      const zoomOutBtn = screen.getByRole("button", { name: "Zoom out" });

      // Zoom in once: scale should increase by 1.2
      fireEvent.click(zoomInBtn);
      const tZoomedIn = parseTransform(world.style.transform);
      expect(tZoomedIn.scale).toBeCloseTo(tInitial.scale * 1.2, 4);

      // At tZoomedIn, scale is above the fitted minimum (~0.038), so Zoom out should NOT be disabled
      expect(zoomOutBtn).toHaveAttribute("aria-disabled", "false");

      // Click Zoom out: scale must decrease back toward initial
      fireEvent.click(zoomOutBtn);
      const tZoomedOut = parseTransform(world.style.transform);
      expect(tZoomedOut.scale).toBeCloseTo(tInitial.scale, 3);
    });

    it("handles large 100-column and tall 100-row runs without NaN", () => {
      const { container, rerender } = render(<DagGraphView snapshot={buildBigDagRun()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

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
    it("cancels active drag on pointercancel without moving on subsequent pressed moves", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });
      expect(viewport.hasPointerCapture(1)).toBe(true);

      // Cancel gesture
      fireEvent.pointerCancel(viewport, { pointerId: 1 });
      expect(viewport.hasPointerCapture(1)).toBe(false);
      const tCancelled = parseTransform(world.style.transform);

      // Subsequent pressed move must NOT move the camera
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);

      expect(tAfter.x).toBe(tCancelled.x);
      expect(tAfter.y).toBe(tCancelled.y);
    });

    it("cancels active drag on window blur and ignores subsequent pressed moves", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });
      expect(viewport.hasPointerCapture(1)).toBe(true);

      window.dispatchEvent(new Event("blur"));
      expect(viewport.hasPointerCapture(1)).toBe(false);
      const tBlur = parseTransform(world.style.transform);

      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter.x).toBe(tBlur.x);
      expect(tAfter.y).toBe(tBlur.y);
    });

    it("cancels active drag on document visibilitychange (hidden) and ignores pressed moves", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });
      expect(viewport.hasPointerCapture(1)).toBe(true);

      Object.defineProperty(document, "hidden", { configurable: true, value: true });
      document.dispatchEvent(new Event("visibilitychange"));
      expect(viewport.hasPointerCapture(1)).toBe(false);
      const tHidden = parseTransform(world.style.transform);

      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 300, clientY: 300 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter.x).toBe(tHidden.x);
      expect(tAfter.y).toBe(tHidden.y);
      Object.defineProperty(document, "hidden", { configurable: true, value: false });
    });

    it("cancels active touch pinch on window blur and ignores subsequent moves", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      fireEvent.pointerDown(viewport, { pointerId: 10, pointerType: "touch", clientX: 200, clientY: 200 });
      fireEvent.pointerDown(viewport, { pointerId: 11, pointerType: "touch", clientX: 300, clientY: 200 });
      expect(viewport.hasPointerCapture(10)).toBe(true);
      expect(viewport.hasPointerCapture(11)).toBe(true);

      window.dispatchEvent(new Event("blur"));
      expect(viewport.hasPointerCapture(10)).toBe(false);
      expect(viewport.hasPointerCapture(11)).toBe(false);
      const tBlur = parseTransform(world.style.transform);

      fireEvent.pointerMove(viewport, { pointerId: 10, pointerType: "touch", clientX: 150, clientY: 200 });
      fireEvent.pointerMove(viewport, { pointerId: 11, pointerType: "touch", clientX: 350, clientY: 200 });
      const tAfter = parseTransform(world.style.transform);
      expect(tAfter).toEqual(tBlur);
    });

    it("releases pointer capture on unmount without throwing errors", () => {
      const { container, unmount } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      expect(viewport.hasPointerCapture(1)).toBe(true);

      expect(() => unmount()).not.toThrow();
      expect(viewport.hasPointerCapture(1)).toBe(false);
    });
  });

  describe("Contract 5 & 7 & S5: Touch & Pinch Transitions", () => {
    it("outward pinch at newly raised minimum obeys direction-preserving recovery without jumping (B2 regression)", () => {
      const bigRun = buildBigDagRun();
      const { container, rerender } = render(<DagGraphView snapshot={bigRun} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const fitBtn = screen.getByRole("button", { name: "Fit graph" });
      fireEvent.click(fitBtn);
      const tBig = parseTransform(world.style.transform);
      expect(tBig.scale).toBeLessThan(0.1);

      // Update snapshot to fixture A under the same runId (raises minimum to 0.1)
      const smallRun = { ...buildQaDagRunA(), runId: bigRun.runId };
      rerender(<DagGraphView snapshot={smallRun} />);

      // Topology retention preserves camera
      const tBeforePinch = parseTransform(world.style.transform);
      expect(tBeforePinch.scale).toBeCloseTo(tBig.scale, 4);

      // Outward pinch: distance 100 -> distance 90 (zooming out)
      fireEvent.pointerDown(viewport, { pointerId: 21, pointerType: "touch", clientX: 200, clientY: 200 });
      fireEvent.pointerDown(viewport, { pointerId: 22, pointerType: "touch", clientX: 300, clientY: 200 });

      fireEvent.pointerMove(viewport, { pointerId: 21, pointerType: "touch", clientX: 205, clientY: 200 });
      fireEvent.pointerMove(viewport, { pointerId: 22, pointerType: "touch", clientX: 295, clientY: 200 });

      const tAfterPinch = parseTransform(world.style.transform);
      // Outward request below minimum must be a NO-OP; must NOT jump up to 0.1!
      expect(tAfterPinch.scale).toBeCloseTo(tBeforePinch.scale, 4);
      expect(tAfterPinch.x).toBeCloseTo(tBeforePinch.x, 1);
      expect(tAfterPinch.y).toBeCloseTo(tBeforePinch.y, 1);

      fireEvent.pointerUp(viewport, { pointerId: 21, pointerType: "touch" });
      fireEvent.pointerUp(viewport, { pointerId: 22, pointerType: "touch" });
    });

    it("two-touch pinch scales and anchors around touch midpoint", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const t0 = parseTransform(world.style.transform);
      const s0 = t0.scale;

      // Contact 1 at (200, 200), Contact 2 at (300, 200) -> distance = 100, midpoint = (250, 200)
      fireEvent.pointerDown(viewport, { pointerId: 10, pointerType: "touch", clientX: 200, clientY: 200 });
      fireEvent.pointerDown(viewport, { pointerId: 11, pointerType: "touch", clientX: 300, clientY: 200 });

      // Pinch out: distance becomes 160 -> scale multiplies by 1.6
      fireEvent.pointerMove(viewport, { pointerId: 10, pointerType: "touch", clientX: 170, clientY: 200 });
      fireEvent.pointerMove(viewport, { pointerId: 11, pointerType: "touch", clientX: 330, clientY: 200 });

      const t1 = parseTransform(world.style.transform);
      expect(t1.scale).toBeCloseTo(s0 * 1.6, 1);

      // Verify anchor preservation: local midpoint (250, 200) relative to viewport
      const rect = viewport.getBoundingClientRect();
      const m0x = 250 - rect.left;
      const m0y = 200 - rect.top;
      const worldAnchorX = (m0x - t0.x) / t0.scale;
      const worldAnchorY = (m0y - t0.y) / t0.scale;
      const mappedX = t1.x + t1.scale * worldAnchorX;
      const mappedY = t1.y + t1.scale * worldAnchorY;
      expect(mappedX).toBeCloseTo(m0x, 1);
      expect(mappedY).toBeCloseTo(m0y, 1);

      fireEvent.pointerUp(viewport, { pointerId: 10, pointerType: "touch" });
      fireEvent.pointerUp(viewport, { pointerId: 11, pointerType: "touch" });
    });

    it("rebases seamlessly on 1 -> 2 -> 1 touch transitions without jump", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

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

  describe("B5 complete capture and camera regressions", () => {
    it.each([1, 2])("rebases a real pinch when contact %i survives", (survivor) => {
      const { container } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport);
      const initial = parseTransform(world.style.transform);
      fireEvent.pointerDown(viewport, { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
      fireEvent.pointerMove(document.body, { pointerId: 1, pointerType: "touch", clientX: 120, clientY: 110 });
      const panned = parseTransform(world.style.transform);
      expect(panned).toEqual({ ...initial, x: initial.x + 20, y: initial.y + 10 });
      fireEvent.pointerDown(viewport, { pointerId: 2, pointerType: "touch", clientX: 220, clientY: 110 });
      expect(parseTransform(world.style.transform)).toEqual(panned);
      fireEvent.pointerMove(document.body, { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 130 });
      fireEvent.pointerMove(document.body, { pointerId: 2, pointerType: "touch", clientX: 260, clientY: 130 });
      const pinched = parseTransform(world.style.transform);
      expect(pinched.scale).toBeCloseTo(panned.scale * 1.6, 8);
      expect(pinched.x).toBeCloseTo(180 - 1.6 * (170 - panned.x), 8);
      expect(pinched.y).toBeCloseTo(130 - 1.6 * (110 - panned.y), 8);
      fireEvent.pointerUp(document.body, { pointerId: 3 - survivor, pointerType: "touch" });
      expect(viewport.hasPointerCapture(3 - survivor)).toBe(false);
      expect(viewport.hasPointerCapture(survivor)).toBe(true);
      expect(parseTransform(world.style.transform)).toEqual(pinched);
      fireEvent.pointerMove(document.body, { pointerId: survivor, pointerType: "touch", clientX: (survivor === 1 ? 100 : 260) + 20, clientY: 140 });
      expect(parseTransform(world.style.transform)).toEqual({ ...pinched, x: pinched.x + 20, y: pinched.y + 10 });
      fireEvent.pointerUp(document.body, { pointerId: survivor, pointerType: "touch" });
      expect(viewport.hasPointerCapture(survivor)).toBe(false);
    });

    it("defers a subpixel pinch baseline then anchors the first nondegenerate distance", () => {
      const { container } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport);
      const initial = parseTransform(world.style.transform);
      fireEvent.pointerDown(viewport, { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
      fireEvent.pointerDown(viewport, { pointerId: 2, pointerType: "touch", clientX: 100, clientY: 100 });
      fireEvent.pointerMove(document.body, { pointerId: 2, pointerType: "touch", clientX: 100.5, clientY: 100 });
      expect(parseTransform(world.style.transform)).toEqual(initial);
      fireEvent.pointerMove(document.body, { pointerId: 2, pointerType: "touch", clientX: 110, clientY: 100 });
      expect(parseTransform(world.style.transform)).toEqual(initial);
      fireEvent.pointerMove(document.body, { pointerId: 2, pointerType: "touch", clientX: 120, clientY: 100 });
      const actual = parseTransform(world.style.transform);
      expect(actual.scale).toBeCloseTo(initial.scale * 2, 8);
      expect(actual.x).toBeCloseTo(110 - 2 * (105 - initial.x), 8);
      expect(actual.y).toBeCloseTo(100 - 2 * (100 - initial.y), 8);
    });

    it("ignores extra contact motion and release without replacing the selected pair", () => {
      const { container } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport);
      const initial = parseTransform(world.style.transform);
      fireEvent.pointerDown(viewport, { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
      fireEvent.pointerDown(viewport, { pointerId: 2, pointerType: "touch", clientX: 200, clientY: 100 });
      fireEvent.pointerDown(viewport, { pointerId: 3, pointerType: "touch", clientX: 300, clientY: 300 });
      expect(viewport.hasPointerCapture(3)).toBe(false);
      fireEvent.pointerMove(viewport, { pointerId: 3, pointerType: "touch", clientX: 400, clientY: 450 });
      expect(parseTransform(world.style.transform)).toEqual(initial);
      fireEvent.pointerUp(viewport, { pointerId: 3, pointerType: "touch" });
      expect(parseTransform(world.style.transform)).toEqual(initial);
      fireEvent.pointerMove(document.body, { pointerId: 1, pointerType: "touch", clientX: 80, clientY: 120 });
      fireEvent.pointerMove(document.body, { pointerId: 2, pointerType: "touch", clientX: 240, clientY: 120 });
      const actual = parseTransform(world.style.transform);
      expect(actual.scale).toBeCloseTo(initial.scale * 1.6, 8);
      expect(actual.x).toBeCloseTo(160 - 1.6 * (150 - initial.x), 8);
      expect(actual.y).toBeCloseTo(120 - 1.6 * (100 - initial.y), 8);
    });

    it.each(["cancel", "lost capture", "blur", "hidden", "run switch", "resize", "unmount"] as const)(
      "releases both touch captures on %s and ignores continuing contacts", (interruption) => {
        const { container, rerender, unmount } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
        const { viewport, world } = getElements(container);
        mockViewportDimensions(viewport);
        fireEvent.pointerDown(viewport, { pointerId: 10, pointerType: "touch", clientX: 100, clientY: 100 });
        fireEvent.pointerDown(viewport, { pointerId: 11, pointerType: "touch", clientX: 200, clientY: 100 });
        fireEvent.pointerMove(document.body, { pointerId: 11, pointerType: "touch", clientX: 240, clientY: 130 });
        expect(viewport.hasPointerCapture(10)).toBe(true);
        expect(viewport.hasPointerCapture(11)).toBe(true);
        act(() => {
          switch (interruption) {
            case "cancel":
              fireEvent.pointerCancel(document.body, { pointerId: 10, pointerType: "touch" });
              fireEvent.pointerCancel(document.body, { pointerId: 11, pointerType: "touch" });
              break;
            case "lost capture": viewport.releasePointerCapture(10); viewport.releasePointerCapture(11); break;
            case "blur": window.dispatchEvent(new Event("blur")); break;
            case "hidden":
              vi.spyOn(document, "hidden", "get").mockReturnValue(true);
              document.dispatchEvent(new Event("visibilitychange"));
              break;
            case "run switch": rerender(<DagGraphView snapshot={buildTallDagRun()} />); break;
            case "resize": triggerResize(viewport, 800, 600); break;
            case "unmount": unmount(); break;
          }
        });
        // Soft assertions keep the continuation oracle reachable if release fails.
        expect.soft(viewport.hasPointerCapture(10)).toBe(false);
        expect.soft(viewport.hasPointerCapture(11)).toBe(false);
        const stopped = parseTransform(world.style.transform);
        for (const target of [document.body, viewport]) {
          fireEvent.pointerMove(target, { pointerId: 10, pointerType: "touch", buttons: 1, clientX: 50, clientY: 300 });
          fireEvent.pointerMove(target, { pointerId: 11, pointerType: "touch", buttons: 1, clientX: 450, clientY: 350 });
          expect(parseTransform(world.style.transform)).toEqual(stopped);
          fireEvent.pointerMove(target, { pointerId: 10, buttons: 0, clientX: 500, clientY: 500 });
          expect(parseTransform(world.style.transform)).toEqual(stopped);
        }
      },
    );

    it("delivers lost capture to React and makes pressed mouse continuation inert", () => {
      const { container } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport);
      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      fireEvent.pointerMove(document.body, { pointerId: 1, buttons: 1, clientX: 150, clientY: 130 });
      const stopped = parseTransform(world.style.transform);
      act(() => { viewport.releasePointerCapture(1); });
      expect(viewport.hasPointerCapture(1)).toBe(false);
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 500, clientY: 500 });
      expect(parseTransform(world.style.transform)).toEqual(stopped);
    });

    it("fits a tiny 20x20 viewport with a five-pixel margin and no minimum clamp jump", () => {
      const { container } = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 20, 20);
      // Fixture A world is 1160 x 214; margin = min(24, 20/4, 20/4).
      const expectedScale = 10 / 1160;
      const fitted = parseTransform(world.style.transform);
      expect(fitted.scale).toBeCloseTo(expectedScale, 10);
      expect(fitted.x).toBeCloseTo(5, 8);
      expect(fitted.y).toBeCloseTo((20 - 214 * expectedScale) / 2, 8);
      fireEvent.wheel(viewport, { clientX: 7, clientY: 13, deltaY: 120 });
      expect(parseTransform(world.style.transform)).toEqual(fitted);
    });

    it("delivers resize only to the observed viewport", () => {
      const first = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const second = render(<DagGraphView snapshot={buildQaDagRunA()} />);
      const a = getElements(first.container);
      const b = getElements(second.container);
      mockViewportDimensions(a.viewport);
      mockViewportDimensions(b.viewport, 600, 400);
      const before = parseTransform(a.world.style.transform);
      triggerResize(b.viewport, 800, 500);
      expect(parseTransform(a.world.style.transform)).toEqual(before);
    });
  });

  describe("Contract 10-11 & S6: Live State, Empty Runs, Resize", () => {
    it("mounts without error on no-run / empty state (unconditional hooks)", () => {
      const { container, rerender } = render(<DagGraphView />);
      expect(screen.getByText("No dag runs yet")).toBeInTheDocument();

      // Transition from no-run -> run
      rerender(<DagGraphView snapshot={sampleSnapshot} />);
      const { world, viewport } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);
      const t = parseTransform(world.style.transform);
      expect(t.scale).toBeGreaterThan(0);
    });

    it("fits camera on same-run empty -> first nonempty transition (B1 regression)", () => {
      const emptyRun = buildEmptyDagRun();
      const { container, rerender } = render(<DagGraphView snapshot={emptyRun} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      const fitBtn = screen.getByRole("button", { name: "Fit graph" });
      fireEvent.click(fitBtn);
      const tEmpty = parseTransform(world.style.transform);

      // Same runId, now populated with 100 columns (big)
      const bigRun = { ...buildBigDagRun(), runId: emptyRun.runId };
      rerender(<DagGraphView snapshot={bigRun} />);

      const tPopulated = parseTransform(world.style.transform);
      // Must have fitted the big graph (< 0.1 scale), not retained the empty camera scale (~1.0)
      expect(tPopulated.scale).toBeLessThan(0.1);
      expect(tPopulated.scale).not.toBeCloseTo(tEmpty.scale, 2);
    });

    it("defers fit when populated at zero dimensions and fits upon positive measurement (B1 zero regression)", () => {
      const emptyRun = buildEmptyDagRun();
      const { container, rerender } = render(<DagGraphView snapshot={emptyRun} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);
      triggerResize(viewport, 0, 0);

      const bigRun = { ...buildBigDagRun(), runId: emptyRun.runId };
      rerender(<DagGraphView snapshot={bigRun} />);

      // Resize to positive (200, 200)
      triggerResize(viewport, 200, 200);
      const tPos = parseTransform(world.style.transform);

      // Must fit to 200x200 (scale for big graph in 200x200 is ~0.0052)
      expect(tPos.scale).toBeLessThan(0.01);
      expect(tPos.x).toBeCloseTo(24, 1);
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

    it("run switch (different runId) cancels active gestures, releases capture, and refits", () => {
      const runA = buildQaDagRunA();
      const { container, rerender } = render(<DagGraphView snapshot={runA} />);
      const { viewport, world } = getElements(container);
      mockViewportDimensions(viewport, 1000, 700);

      // Start drag
      fireEvent.pointerDown(viewport, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 100 });
      expect(viewport.hasPointerCapture(1)).toBe(true);

      // Switch to run B
      const runB = buildTallDagRun();
      rerender(<DagGraphView snapshot={runB} />);

      // Gesture must have been cancelled and capture released
      expect(viewport.hasPointerCapture(1)).toBe(false);
      const tNewRun = parseTransform(world.style.transform);
      expect(Number.isFinite(tNewRun.x)).toBe(true);
      expect(Number.isFinite(tNewRun.scale)).toBe(true);

      // Subsequent pressed move must not pan the new run's camera
      fireEvent.pointerMove(viewport, { pointerId: 1, buttons: 1, clientX: 500, clientY: 500 });
      const tAfterMove = parseTransform(world.style.transform);
      expect(tAfterMove.x).toBeCloseTo(tNewRun.x, 1);
      expect(tAfterMove.y).toBeCloseTo(tNewRun.y, 1);
    });

    it("ResizeObserver changes translation by (deltaW/2, deltaH/2) keeping center point stable", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);

      // Initial size 1000x700
      triggerResize(viewport, 1000, 700);
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
      const t0 = parseTransform(world.style.transform);

      // Resize to 0
      triggerResize(viewport, 0, 0);
      const tZero = parseTransform(world.style.transform);
      expect(tZero.x).toBe(t0.x);
      expect(tZero.y).toBe(t0.y);
      expect(tZero.scale).toBe(t0.scale);
    });

    it("measured width crossing 399 -> 400 -> 399 preserves camera scale, adjusts center translation by deltaW/2 without refitting, and keeps header structure intact", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      const header = screen.getByTestId("dag-header");
      const controls = screen.getByTestId("dag-controls");
      const legend = screen.getByTestId("dag-legend");

      // Initial layout at 399px width (compact mode)
      triggerResize(viewport, 399, 600);
      const t399 = parseTransform(world.style.transform);
      expect(t399.scale).toBeGreaterThan(0);
      expect(header).toContainElement(controls);
      expect(header).toContainElement(legend);

      // Resize crossing threshold to 400px width (wide mode)
      triggerResize(viewport, 400, 600);
      const t400 = parseTransform(world.style.transform);
      expect(t400.scale).toBe(t399.scale);
      expect(t400.x).toBeCloseTo(t399.x + 0.5, 2);
      expect(t400.y).toBeCloseTo(t399.y, 2);
      expect(header).toContainElement(controls);
      expect(header).toContainElement(legend);

      // Resize back across threshold to 399px width (compact mode)
      triggerResize(viewport, 399, 600);
      const tBack399 = parseTransform(world.style.transform);
      expect(tBack399.scale).toBe(t399.scale);
      expect(tBack399.x).toBeCloseTo(t399.x, 2);
      expect(tBack399.y).toBeCloseTo(t399.y, 2);
      expect(header).toContainElement(controls);
      expect(header).toContainElement(legend);
    });

    it("header wheel and pointer inputs do not alter camera transform in either compact or wide mode", () => {
      const { container } = render(<DagGraphView snapshot={sampleSnapshot} />);
      const { viewport, world } = getElements(container);
      const header = screen.getByTestId("dag-header");

      // 1. Compact mode (width < 400)
      triggerResize(viewport, 350, 600);
      const tCompact0 = parseTransform(world.style.transform);

      fireEvent.pointerDown(header, { pointerId: 1, button: 0, buttons: 1, clientX: 100, clientY: 20 });
      fireEvent.pointerMove(header, { pointerId: 1, buttons: 1, clientX: 200, clientY: 50 });
      fireEvent.pointerUp(header, { pointerId: 1, clientX: 200, clientY: 50 });

      const tCompactAfterPointer = parseTransform(world.style.transform);
      expect(tCompactAfterPointer.x).toBe(tCompact0.x);
      expect(tCompactAfterPointer.y).toBe(tCompact0.y);
      expect(tCompactAfterPointer.scale).toBe(tCompact0.scale);

      fireEvent.wheel(header, { deltaY: -120, clientX: 100, clientY: 20 });
      const tCompactAfterWheel = parseTransform(world.style.transform);
      expect(tCompactAfterWheel.x).toBe(tCompact0.x);
      expect(tCompactAfterWheel.y).toBe(tCompact0.y);
      expect(tCompactAfterWheel.scale).toBe(tCompact0.scale);

      // 2. Wide mode (width >= 400)
      triggerResize(viewport, 800, 600);
      const tWide0 = parseTransform(world.style.transform);

      fireEvent.pointerDown(header, { pointerId: 2, button: 0, buttons: 1, clientX: 150, clientY: 20 });
      fireEvent.pointerMove(header, { pointerId: 2, buttons: 1, clientX: 300, clientY: 50 });
      fireEvent.pointerUp(header, { pointerId: 2, clientX: 300, clientY: 50 });

      const tWideAfterPointer = parseTransform(world.style.transform);
      expect(tWideAfterPointer.x).toBe(tWide0.x);
      expect(tWideAfterPointer.y).toBe(tWide0.y);
      expect(tWideAfterPointer.scale).toBe(tWide0.scale);

      fireEvent.wheel(header, { deltaY: 200, clientX: 150, clientY: 20 });
      const tWideAfterWheel = parseTransform(world.style.transform);
      expect(tWideAfterWheel.x).toBe(tWide0.x);
      expect(tWideAfterWheel.y).toBe(tWide0.y);
      expect(tWideAfterWheel.scale).toBe(tWide0.scale);
    });
  });
});
