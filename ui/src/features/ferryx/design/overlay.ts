import type { DesignIdentity, GuestEvent } from "./model";

/** Self-contained: serialized into the child page, never installed in the app/terminal DOM. */
export function installDesignOverlay(identity: DesignIdentity, mode: "element" | "rectangle", emit: (event: GuestEvent) => void): () => void {
  const hostWindow = window as Window & { __ferryxDesignDispose?: () => void };
  hostWindow.__ferryxDesignDispose?.();
  const overlay = document.createElement("div");
  overlay.dataset.ferryxDesignOverlay = "";
  overlay.style.cssText = "all:initial;position:fixed;pointer-events:none;z-index:2147483647;outline:2px solid Highlight;background:transparent;display:none";
  document.documentElement.append(overlay);
  let active = true;
  let start: { x: number; y: number } | undefined;
  const listeners: [string, EventListener][] = [];
  const dispose = () => {
    if (!active) return;
    active = false;
    for (const [name, listener] of listeners) window.removeEventListener(name, listener, true);
    overlay.remove();
    if (hostWindow.__ferryxDesignDispose === dispose) delete hostWindow.__ferryxDesignDispose;
  };
  hostWindow.__ferryxDesignDispose = dispose;
  const on = (name: string, listener: EventListener) => { listeners.push([name, listener]); window.addEventListener(name, listener, true); };
  const finish = (event: GuestEvent) => { if (!active) return; dispose(); emit(event); };
  const rect = (x1: number, y1: number, x2: number, y2: number) => {
    const x = Math.max(0, Math.min(x1, x2)), y = Math.max(0, Math.min(y1, y2));
    return { x, y, width: Math.min(innerWidth, Math.max(x1, x2)) - x, height: Math.min(innerHeight, Math.max(y1, y2)) - y };
  };
  const paint = (r: { x: number; y: number; width: number; height: number }) => {
    Object.assign(overlay.style, { display: "block", left: `${r.x}px`, top: `${r.y}px`, width: `${Math.max(0, r.width)}px`, height: `${Math.max(0, r.height)}px` });
  };
  const selector = (element: Element): string => {
    const escape = (s: string) => s.replace(/[^a-zA-Z0-9_-]/g, c => `\\${c.codePointAt(0)!.toString(16)} `).replace(/^[0-9]/, c => `\\3${c} `);
    if (element.id) return `#${escape(element.id)}`;
    const parts: string[] = [];
    for (let el: Element | null = element; el && parts.length < 20; el = el.parentElement) {
      const siblings = el.parentElement ? [...el.parentElement.children].filter(e => e.tagName === el!.tagName) : [el];
      parts.unshift(`${el.localName}:nth-of-type(${siblings.indexOf(el) + 1})`);
    }
    return parts.join(" > ");
  };
  const selected = (r: { x: number; y: number; width: number; height: number }, element?: Element) => {
    if (r.width <= 0 || r.height <= 0) { finish({ type: "cancelled", identity }); return; }
    const ancestry: string[] = [];
    for (let el = element?.parentElement; el && ancestry.length < 20; el = el.parentElement) ancestry.push(el.localName);
    const css: Record<string, string> = {};
    if (element) { const computed = getComputedStyle(element); for (const key of ["color", "background-color", "font-family", "font-size", "font-weight", "display", "margin", "padding", "border"]) css[key] = computed.getPropertyValue(key); }
    finish({ type: "selected", selection: { identity, mode, rect: r, viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio, zoom: window.visualViewport?.scale ?? 1 }, url: location.href, title: document.title,
      ...(element ? { element: { tag: element.localName, id: element.id, classes: [...element.classList], selector: selector(element), ancestry, css, ...(element.localName === "iframe" ? { contextUnavailable: "FRAME_INTERNALS_UNAVAILABLE" } : {}) } } : {}) } });
  };
  on("pointermove", event => {
    const e = event as PointerEvent;
    if (mode === "rectangle" && start) paint(rect(start.x, start.y, e.clientX, e.clientY));
    else if (mode === "element" && e.target instanceof Element) { const r = e.target.getBoundingClientRect(); paint(rect(r.left, r.top, r.right, r.bottom)); }
  });
  on("pointerdown", event => { const e = event as PointerEvent; if (e.button !== 0) return; e.preventDefault(); e.stopImmediatePropagation(); if (mode === "rectangle") start = { x: e.clientX, y: e.clientY }; });
  on("pointerup", event => { const e = event as PointerEvent; if (mode === "rectangle" && start) { e.preventDefault(); e.stopImmediatePropagation(); selected(rect(start.x, start.y, e.clientX, e.clientY)); } });
  on("click", event => { event.preventDefault(); event.stopImmediatePropagation(); if (mode === "element" && event.target instanceof Element) { const r = event.target.getBoundingClientRect(); selected(rect(r.left, r.top, r.right, r.bottom), event.target); } });
  on("keydown", event => { if ((event as KeyboardEvent).key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); finish({ type: "cancelled", identity }); } });
  for (const name of ["resize", "scroll", "pagehide", "blur"]) on(name, () => finish({ type: "invalidated", identity }));
  return dispose;
}
export function overlayScript(identity: DesignIdentity, mode: "element" | "rectangle", bridgeName: string): string {
  return `(${installDesignOverlay.toString()})(${JSON.stringify(identity)},${JSON.stringify(mode)},event=>window[${JSON.stringify(bridgeName)}](event))`;
}
