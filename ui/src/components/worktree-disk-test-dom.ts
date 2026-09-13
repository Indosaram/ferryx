import { JSDOM } from "jsdom";

if (typeof document === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body><div id=\"root\"></div></body></html>", {
    url: "http://localhost",
  });
  globalThis.document = dom.window.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.navigator = dom.window.navigator;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
  if (!globalThis.ResizeObserver) {
    globalThis.ResizeObserver = class {
      disconnect() {}
      observe() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver;
  }
}

import * as tldom from "@testing-library/dom";
if (tldom && tldom.screen && typeof document !== "undefined" && document.body) {
  const queries = tldom.getQueriesForElement(document.body);
  Object.assign(tldom.screen, queries);
}
