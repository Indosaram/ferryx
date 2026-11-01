// @vitest-environment node
//
// Runs the real password fill script (src-tauri/src/browser/vault/fill.js) in jsdom fixture
// pages the way the host does: the saved login embedded as JSON in place of __FERRYX_FILL__,
// evaluated in the frame the engine targets. Covers the exact-origin gate (http never gets an
// https login), cross-origin and same-origin iframes (never filled), and fields the user cannot
// see (never filled).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { JSDOM } from "jsdom";
import { describe, expect, it } from "vitest";

const SOURCE = readFileSync(resolve(__dirname, "../../../src-tauri/src/browser/vault/fill.js"), "utf8");
const SAVED = { origin: "https://example.com", username: "ada@example.com", password: "ferryx-fixture-pw-7f3c" };

type Evaluating = { eval: (code: string) => unknown };

function fillScript(login: { origin: string; username: string; password: string } = SAVED): string {
  return SOURCE.replace("__FERRYX_FILL__", JSON.stringify(login));
}

/** The frame's window with `body` as its content. jsdom does not fetch frame URLs, so a framed
 *  page keeps its URL and origin but starts without a document element. */
function framed(frame: HTMLIFrameElement, body: string): Window {
  const frameWindow = frame.contentWindow!;
  const doc = frameWindow.document;
  if (!doc.documentElement) doc.appendChild(doc.createElement("html")).appendChild(doc.createElement("body"));
  doc.body.innerHTML = body;
  return frameWindow;
}

function page(body: string, url = "https://example.com/login") {
  const dom = new JSDOM(`<!DOCTYPE html><html><head><title>Login</title></head><body>${body}</body></html>`, {
    url,
    runScripts: "outside-only",
  });
  const document = dom.window.document;
  const value = (selector: string, root: Document = document) =>
    (root.querySelector(selector) as HTMLInputElement | null)?.value;
  const run = (target: Window = dom.window as unknown as Window) => (target as unknown as Evaluating).eval(fillScript());
  return { dom, window: dom.window, document, value, run };
}

const LOGIN_FORM = `
  <form id="login">
    <input id="user" type="email" autocomplete="username">
    <input id="pw" type="password" autocomplete="current-password">
    <button>Sign in</button>
  </form>`;

describe("vault fill.js", () => {
  it("fills the visible login form on the saved origin and announces the values", () => {
    const { document, value, run } = page(LOGIN_FORM);
    const seen: string[] = [];
    for (const id of ["user", "pw"]) {
      const input = document.getElementById(id)!;
      input.addEventListener("input", () => seen.push(`input:${id}`));
      input.addEventListener("change", () => seen.push(`change:${id}`));
    }
    expect(run()).toBe("filled");
    expect(value("#user")).toBe(SAVED.username);
    expect(value("#pw")).toBe(SAVED.password);
    expect(seen).toEqual(["input:user", "change:user", "input:pw", "change:pw"]);
  });

  it("never fills an http page with a login saved for https", () => {
    const { value, run } = page(LOGIN_FORM, "http://example.com/login");
    expect(run()).toBe("origin-mismatch");
    expect(value("#user")).toBe("");
    expect(value("#pw")).toBe("");
  });

  it("never fills another host, a subdomain or another port", () => {
    for (const url of ["https://login.example.com/", "https://example.com:8443/", "https://example.com.evil.test/"]) {
      const { value, run } = page(LOGIN_FORM, url);
      expect(run(), url).toBe("origin-mismatch");
      expect(value("#pw"), url).toBe("");
    }
  });

  it("never fills a cross-origin iframe, even one on the saved origin inside another site", () => {
    // The saved origin framed by an attacker page: the frame matches the origin but is not the
    // top frame, so it is refused before any field is touched.
    const outer = page(`<iframe id="frame" src="https://example.com/embedded-login"></iframe>`, "https://evil.test/");
    const frameWindow = framed(outer.document.getElementById("frame") as HTMLIFrameElement, LOGIN_FORM);
    expect(frameWindow.location.origin).toBe("https://example.com");
    expect(outer.run(frameWindow)).toBe("not-top");
    expect(outer.value("#pw", frameWindow.document)).toBe("");
    // The top frame itself is on another origin.
    expect(outer.run()).toBe("origin-mismatch");
  });

  it("fills the top frame only and leaves its iframes alone", () => {
    const top = page(`${LOGIN_FORM}<iframe id="frame" src="https://ads.test/widget"></iframe>`);
    const frameWindow = framed(
      top.document.getElementById("frame") as HTMLIFrameElement,
      `<input id="pw" type="password"><input id="user" type="text">`,
    );
    expect(top.run()).toBe("filled");
    expect(top.value("#pw")).toBe(SAVED.password);
    expect(top.value("#pw", frameWindow.document)).toBe("");
    expect(top.value("#user", frameWindow.document)).toBe("");
    // Run inside that frame directly (the engine never does): still refused.
    expect(top.run(frameWindow)).toBe("not-top");
  });

  it("refuses a same-origin iframe too: only the top frame is filled", () => {
    const top = page(`<iframe id="frame" src="about:blank"></iframe>`);
    const frameWindow = framed(top.document.getElementById("frame") as HTMLIFrameElement, LOGIN_FORM);
    expect(top.run(frameWindow)).toBe("not-top");
    expect(top.value("#pw", frameWindow.document)).toBe("");
  });

  it("refuses when the password field is not visible", () => {
    const hidden = [
      `<form><input id="user" type="text"><input id="pw" type="password" hidden></form>`,
      `<form style="display:none"><input id="user" type="text"><input id="pw" type="password"></form>`,
      `<div style="opacity:0"><form><input id="user" type="text"><input id="pw" type="password"></form></div>`,
      `<form><input id="user" type="text"><input id="pw" type="password" style="visibility:hidden"></form>`,
      `<div inert><form><input id="user" type="text"><input id="pw" type="password"></form></div>`,
    ];
    for (const body of hidden) {
      const { value, run } = page(body);
      expect(run(), body).toBe("not-visible");
      expect(value("#user"), body).toBe("");
      expect(value("#pw"), body).toBe("");
    }
  });

  it("picks the visible form over a hidden decoy and skips a hidden username field", () => {
    const { value, run } = page(`
      <form id="decoy" style="display:none"><input id="decoy-user" type="text"><input id="decoy-pw" type="password"></form>
      <form id="real">
        <input id="trap" type="text" style="display:none">
        <input id="user" type="text">
        <input id="pw" type="password">
      </form>`);
    expect(run()).toBe("filled");
    expect(value("#decoy-pw")).toBe("");
    expect(value("#decoy-user")).toBe("");
    expect(value("#trap")).toBe("");
    expect(value("#user")).toBe(SAVED.username);
    expect(value("#pw")).toBe(SAVED.password);
  });

  it("reports a page without a login form and touches nothing", () => {
    const { run } = page(`<p>No form here</p>`);
    expect(run()).toBe("no-fields");
  });
});
