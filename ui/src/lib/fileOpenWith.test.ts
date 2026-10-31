import { afterEach, expect, it, vi } from "vitest";
import { printCurrentWebview } from "./fileOpenWith";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

afterEach(() => {
  document.body.replaceChildren();
  vi.clearAllMocks();
});

it("prints only the initiating root after synchronous print preparation", async () => {
  const first = document.createElement("div");
  first.textContent = "first";
  const other = document.createElement("div");
  other.textContent = "other";
  document.body.append(first, other);
  const prepare = () => { first.textContent = "first middle last"; };
  window.addEventListener("beforeprint", prepare, { once: true });
  let finish!: () => void;
  invoke.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
  const pending = printCurrentWebview(first);
  const printable = document.querySelector("[data-file-preview-print-document]");
  expect(printable?.textContent).toBe("first middle last");
  expect(invoke).toHaveBeenCalledWith("cmd_webview_print");
  finish();
  await pending;
  expect(printable?.isConnected).toBe(true);
  window.dispatchEvent(new Event("afterprint"));
  expect(document.querySelector("[data-file-preview-print-document]")).toBeNull();
  expect(other.isConnected).toBe(true);
});

it("restores print state when the native request fails", async () => {
  const root = document.createElement("div");
  document.body.append(root);
  const after = vi.fn();
  window.addEventListener("afterprint", after, { once: true });
  invoke.mockRejectedValue(new Error("print failed"));
  await expect(printCurrentWebview(root)).rejects.toThrow("print failed");
  expect(after).toHaveBeenCalledOnce();
  expect(document.querySelector("[data-file-preview-print-document]")).toBeNull();
});

it("finalizes after confirmed native completion even without a DOM afterprint event", async () => {
  const root = document.createElement("div");
  document.body.append(root);
  const after = vi.fn();
  window.addEventListener("afterprint", after, { once: true });
  invoke.mockResolvedValue(true);
  await printCurrentWebview(root);
  expect(after).toHaveBeenCalledOnce();
  expect(document.querySelector("[data-file-preview-print-document]")).toBeNull();
  await printCurrentWebview(root);
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("cleans up once when print media exits and permits another print", async () => {
  const root = document.createElement("div");
  document.body.append(root);
  const media = new EventTarget();
  const matchMedia = vi.spyOn(window, "matchMedia").mockReturnValue(media as MediaQueryList);
  const after = vi.fn();
  window.addEventListener("afterprint", after);
  invoke.mockResolvedValue(false);
  try {
    await printCurrentWebview(root);
    const change = (matches: boolean) => {
      const event = new Event("change");
      Object.defineProperty(event, "matches", { value: matches });
      media.dispatchEvent(event);
    };
    change(false);
    expect(document.querySelector("[data-file-preview-print-document]")).not.toBeNull();
    change(true);
    change(false);
    change(false);
    expect(after).toHaveBeenCalledOnce();
    expect(document.querySelector("[data-file-preview-print-document]")).toBeNull();
    await printCurrentWebview(root);
    expect(invoke).toHaveBeenCalledTimes(2);
    window.dispatchEvent(new Event("afterprint"));
  } finally {
    window.removeEventListener("afterprint", after);
    matchMedia.mockRestore();
  }
});
