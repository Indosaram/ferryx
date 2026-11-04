import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ImportSection, type ImportPreview } from "./ImportSection";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), update: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("../../lib/terminalSettings", () => ({
  useTerminalSettings: () => ({ updateSettings: mocks.update }),
}));
beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function chooseFile() {
  fireEvent.change(screen.getByLabelText("Configuration file"), {
    target: { files: [{ size: 3, arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) }] },
  });
}

it("previews without mutation and applies only after confirmation", async () => {
  const started = deferred<void>();
  const result = deferred<ImportPreview>();
  mocks.invoke.mockImplementation(() => { started.resolve(); return result.promise; });
  render(<ImportSection />);
  await act(async () => { chooseFile(); await started.promise; });
  expect(mocks.invoke).toHaveBeenCalledWith("cmd_config_import", { format: "ghostty", bytes: [1, 2, 3] });
  const settings = { fontFamily: "Menlo", fontSize: 16, cursorStyle: "bar" as const, scrollback: null, theme: { background: "#123456" } };
  await act(async () => { result.resolve({ settings, skipped: ["command"] }); await result.promise; });
  expect(mocks.update).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Apply imported settings" }));
  expect(mocks.update).toHaveBeenCalledWith({ fontFamily: "Menlo", fontSize: 16, cursorStyle: "bar", theme: { background: "#123456" } });
});

it("keeps settings unchanged on malformed plist", async () => {
  const started = deferred<void>();
  const result = deferred<ImportPreview>();
  mocks.invoke.mockImplementation(() => { started.resolve(); return result.promise; });
  render(<ImportSection />);
  fireEvent.change(screen.getByLabelText("Terminal format"), { target: { value: "iterm2" } });
  await act(async () => { chooseFile(); await started.promise; });
  await act(async () => { result.reject(new Error("IMPORT_PARSE_ERROR")); await result.promise.catch(() => undefined); });
  expect(screen.getByRole("alert")).toHaveTextContent("IMPORT_PARSE_ERROR");
  expect(mocks.update).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Apply imported settings" })).toBeNull();
});
