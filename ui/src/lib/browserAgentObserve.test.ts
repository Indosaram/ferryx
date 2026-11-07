// @vitest-environment node
//
// Runs the real agent page observer (src-tauri/src/browser/agent/observe.js) in a jsdom page
// the way the host does: `(<source>)(<op>)` evaluated as page script. Covers stable refs
// across node replacement, a page that tampers with the globals the observer would otherwise
// use, password values, frames, postcondition checks and the identity check before acting.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { JSDOM } from "jsdom";
import { describe, expect, it } from "vitest";

const SOURCE = readFileSync(resolve(__dirname, "../../../src-tauri/src/browser/agent/observe.js"), "utf8");
const KEY = "__ferryxAgentTest";
const FIELDS = ["isolated", "max", "ref", "expect", "action", "check"] as const;

type ObservedElement = {
  reference: string;
  role: string;
  name: string;
  tagName: string;
  disabled: boolean;
  checked: boolean | null;
};
type Observation = {
  ok: true;
  url: string;
  title: string;
  doc: string;
  elements: ObservedElement[];
  frames: { kind: string; title: string; src: string }[];
  truncated: boolean;
};
type Failure = { ok: false; code: string; message: string };

function page(body: string, beforeScript = "") {
  const dom = new JSDOM(`<!DOCTYPE html><html><head><title>Fixture</title></head><body>${body}</body></html>`, {
    url: "http://127.0.0.1:18761/replace",
    runScripts: "outside-only",
  });
  const evaluate = (code: string) => (dom.window as unknown as { eval: (code: string) => unknown }).eval(code);
  if (beforeScript) evaluate(beforeScript);
  const run = (op: Record<string, unknown>) => {
    const full: Record<string, unknown> = { key: KEY };
    for (const field of FIELDS) full[field] = null;
    Object.assign(full, op);
    // The host serializes the op with serde_json; the page sees an object literal.
    return evaluate(`(${SOURCE})(${JSON.stringify(full)})`) as Observation | Failure | Record<string, unknown>;
  };
  const observe = () => {
    const result = run({ op: "observe", max: 1000 }) as Observation;
    expect(result.ok).toBe(true);
    return result;
  };
  const fingerprint = (element: ObservedElement) =>
    `${element.tagName}\n${element.role}\n${element.role === "textbox" || element.role === "searchbox" ? "" : element.name}`;
  const act = (element: ObservedElement, action: Record<string, unknown>, expectOverride?: string) =>
    run({ op: "act", ref: element.reference, expect: expectOverride ?? fingerprint(element), action });
  return { dom, window: dom.window, document: dom.window.document, run, observe, act, fingerprint };
}

function find(observation: Observation, role: string, name: string): ObservedElement {
  const matches = observation.elements.filter((element) => element.role === role && element.name === name);
  expect(matches).toHaveLength(1);
  return matches[0];
}

describe("agent observe.js", () => {
  it("names elements by role and accessible name, never reads values, and lists frames", () => {
    const { observe } = page(`
      <label for="name">Name</label><input id="name" value="Ferryx draft">
      <label for="pw">Password</label><input id="pw" type="password" value="ferryx-fixture-password-7f3c9a">
      <input type="hidden" name="csrf" value="token-1">
      <button>Submit</button><a href="/docs">Docs</a>
      <input type="checkbox" aria-label="Remember" checked>
      <div hidden><button>Hidden action</button></div>
      <button disabled>Later</button>
      <iframe title="Other origin frame" src="http://127.0.0.1:18762/frame"></iframe>`);
    const observation = observe();
    const text = JSON.stringify(observation);
    expect(text).not.toContain("ferryx-fixture-password-7f3c9a");
    expect(text).not.toContain("Ferryx draft");
    expect(text).not.toContain("token-1");
    expect(text).not.toContain("Hidden action");
    expect(find(observation, "textbox", "Name").tagName).toBe("input");
    expect(find(observation, "textbox", "Password")).toBeTruthy();
    expect(find(observation, "button", "Submit").disabled).toBe(false);
    expect(find(observation, "button", "Later").disabled).toBe(true);
    expect(find(observation, "link", "Docs")).toBeTruthy();
    expect(find(observation, "checkbox", "Remember").checked).toBe(true);
    expect(observation.frames).toEqual([{ kind: "iframe", title: "Other origin frame", src: "http://127.0.0.1:18762/frame" }]);
    expect(observation.title).toBe("Fixture");
    expect(observation.truncated).toBe(false);
  });

  it("keeps a node's ref across observes and fails a replaced node as stale", () => {
    const { document, window, observe, act } = page(`<section id="slot"><button id="target">Target action</button></section>`);
    const first = find(observe(), "button", "Target action");
    expect(find(observe(), "button", "Target action").reference).toBe(first.reference);

    let clicks = 0;
    const replacement = document.createElement("button");
    replacement.textContent = "Target action";
    replacement.addEventListener("click", () => {
      clicks += 1;
    });
    document.getElementById("target")!.replaceWith(replacement);

    expect(act(first, { type: "click" })).toMatchObject({ ok: false, code: "stale" });
    expect(clicks).toBe(0);
    const next = find(observe(), "button", "Target action");
    expect(next.reference).not.toBe(first.reference);
    expect(act(next, { type: "click" })).toEqual({ ok: true });
    expect(clicks).toBe(1);
    expect(window.document.title).toBe("Fixture");
  });

  it("refuses a node whose identity changed and a ref from another document", () => {
    const { document, observe, act, run } = page(`<button id="go">Pay</button>`);
    const pay = find(observe(), "button", "Pay");
    document.getElementById("go")!.textContent = "Delete account";
    expect(act(pay, { type: "click" })).toMatchObject({ ok: false, code: "changed" });

    const other = page(`<button>Pay</button>`);
    other.observe();
    expect(other.act(pay, { type: "click" })).toMatchObject({ ok: false, code: "stale" });
    expect(run({ op: "act", ref: "nodot", expect: "", action: { type: "click" } })).toMatchObject({ ok: false, code: "stale" });
  });

  it("refuses to act on a document it never observed", () => {
    const { run } = page(`<button>Pay</button>`);
    expect(run({ op: "act", ref: "x.1", expect: "", action: { type: "click" } })).toMatchObject({ ok: false, code: "stale" });
  });

  it("works on a page that replaced the DOM and builtin globals it would use", () => {
    // The /replace fixture's tampering plus builtins the observer's own bookkeeping would use.
    const tamper = `
      window.__events = [];
      window.__valueOf = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').get;
      document.querySelector = function () { return null; };
      document.querySelectorAll = function () { return []; };
      Document.prototype.querySelectorAll = function () { return []; };
      document.getElementById = function () { return null; };
      Element.prototype.getAttribute = function () { return 'tampered'; };
      Element.prototype.getBoundingClientRect = function () { return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0 }; };
      HTMLElement.prototype.click = function () { window.__events.push('tampered-click'); };
      EventTarget.prototype.dispatchEvent = function () { window.__events.push('tampered-dispatch'); return true; };
      Object.defineProperty(HTMLInputElement.prototype, 'value', {
        set: function () { window.__events.push('tampered-value'); },
        get: function () { return ''; },
      });
      Map.prototype.get = function () { return undefined; };
      WeakMap.prototype.get = function () { return 'forged.1'; };
      Array.prototype.push = function () { throw new Error('tampered push'); };
      Object.defineProperty(Object.prototype, 'expect', { get: function () { return 'forged'; }, configurable: true });
      window.JSON = { parse: function () { throw new Error('tampered'); }, stringify: function () { return '"tampered"'; } };
    `;
    const { window, document, observe, act } = page(
      `<button id="target">Target action</button><label for="n">Name</label><input id="n" name="n">`,
      tamper,
    );
    const pageWindow = window as unknown as { __events: string[]; __valueOf: (this: unknown) => string };
    const button = document.body.firstElementChild!;
    const input = document.body.lastElementChild!;
    let clicked = 0;
    button.addEventListener("click", () => {
      clicked += 1;
    });

    const observation = observe();
    const target = find(observation, "button", "Target action");
    const name = find(observation, "textbox", "Name");
    expect(target.reference.startsWith(`${observation.doc}.`)).toBe(true);
    expect(act(target, { type: "click" })).toEqual({ ok: true });
    expect(clicked).toBe(1);
    expect(act(name, { type: "fill", value: "Ferryx" })).toEqual({ ok: true });
    expect(pageWindow.__valueOf.call(input)).toBe("Ferryx");
    expect(pageWindow.__events).toEqual([]);
  });

  it("checks postconditions against page state only", () => {
    const { run, document } = page(`<p id="out">Idle</p>`);
    const check = (kind: string, value: string) => run({ op: "check", check: { kind, value } });
    expect(check("text", "Saved")).toEqual({ ok: true, matched: false });
    document.getElementById("out")!.textContent = "Saved";
    expect(check("text", "Saved")).toEqual({ ok: true, matched: true });
    expect(check("selector", "#out")).toEqual({ ok: true, matched: true });
    expect(check("selector", "#missing")).toEqual({ ok: true, matched: false });
    expect(check("selector", "[[")).toMatchObject({ ok: false, code: "selector" });
    expect(check("url", "/replace")).toEqual({ ok: true, matched: true });
    // The load check follows document.readyState, whatever jsdom reports for this page.
    expect(check("load", "interactive")).toEqual({ ok: true, matched: document.readyState !== "loading" });
    expect(check("script", "true")).toMatchObject({ ok: false, code: "unsupported" });
  });

  it("fills, selects, checks and types through the element's own state", () => {
    const { document, observe, act } = page(`
      <label for="n">Name</label><input id="n">
      <label for="c">Country</label><select id="c"><option value="kr">Korea</option><option value="us">United States</option></select>
      <input id="t" type="checkbox" aria-label="Terms">
      <label for="note">Note</label><textarea id="note">ab</textarea>`);
    const changes: string[] = [];
    for (const id of ["n", "c", "t", "note"]) {
      document.getElementById(id)!.addEventListener("change", () => changes.push(id));
    }
    const observation = observe();
    expect(act(find(observation, "textbox", "Name"), { type: "fill", value: "Ferryx" })).toEqual({ ok: true });
    expect((document.getElementById("n") as HTMLInputElement).value).toBe("Ferryx");
    expect(act(find(observation, "combobox", "Country"), { type: "select", value: "United States" })).toEqual({ ok: true });
    expect((document.getElementById("c") as HTMLSelectElement).value).toBe("us");
    expect(act(find(observation, "combobox", "Country"), { type: "select", value: "Mars" })).toMatchObject({ ok: false, code: "noOption" });
    const terms = find(observation, "checkbox", "Terms");
    expect(act(terms, { type: "check" })).toEqual({ ok: true });
    expect((document.getElementById("t") as HTMLInputElement).checked).toBe(true);
    expect(act(terms, { type: "check" }, `input\ncheckbox\nTerms`)).toEqual({ ok: true });
    expect((document.getElementById("t") as HTMLInputElement).checked).toBe(true);
    expect(act(find(observation, "textbox", "Note"), { type: "type", text: "c😀" })).toEqual({ ok: true });
    expect((document.getElementById("note") as HTMLTextAreaElement).value).toBe("abc😀");
    expect(changes).toEqual(["n", "c", "t", "note"]);
  });

  it("caps the element list and reports truncation", () => {
    const buttons = Array.from({ length: 5 }, (_, index) => `<button>B${index}</button>`).join("");
    const { run } = page(buttons);
    const result = run({ op: "observe", max: 3 }) as Observation;
    expect(result.elements).toHaveLength(3);
    expect(result.truncated).toBe(true);
  });
});
