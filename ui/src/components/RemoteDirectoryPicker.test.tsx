import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RemoteDirectoryListing } from "../lib/remoteDirectories";
import { RemoteDirectoryPicker } from "./RemoteDirectoryPicker";

const remote = vi.hoisted(() => ({ listRemoteDirectories: vi.fn() }));
vi.mock("../lib/remoteDirectories", () => remote);

/** Mirrors the picker's COMPLETION_DEBOUNCE_MS: both sides of the 180ms boundary are asserted. */
const DEBOUNCE_MS = 180;

function listing(path: string, names: string[] = [], homePath = "/home/dev"): RemoteDirectoryListing {
  return {
    path, homePath, parentPath: homePath, truncated: false,
    entries: names.map((name) => ({ name, path: `${path}/${name}`, hidden: name.startsWith(".") })),
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}

async function mount(home = listing("/home/dev", ["code", "Documents", ".config"])) {
  remote.listRemoteDirectories.mockResolvedValueOnce(home);
  const onSelect = vi.fn();
  await act(async () => { render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />); });
  return { input: screen.getByTestId("remote-repo-path-input"), onSelect };
}

/** Serves the remote home, then `repo`/`app`/`other` for any browsed directory, under fake timers. */
async function mountTimed(onSelect: (path: string | null) => void) {
  const home = listing("/home/dev", ["code", "Documents", ".config"]);
  remote.listRemoteDirectories.mockImplementation(async (_host: string, path: string | null) =>
    path === null ? home : listing(path, ["repo", "app", "other"], home.homePath));
  let rendered!: ReturnType<typeof render>;
  await act(async () => {
    rendered = render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />);
  });
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  return { rendered, input: screen.getByTestId("remote-repo-path-input") };
}

/** Fire a change and settle the promises it starts, without running the completion timer. */
async function type(input: HTMLElement, value: string) {
  await act(async () => {
    fireEvent.change(input, { target: { value } });
    await vi.advanceTimersByTimeAsync(0);
  });
}

async function elapse(milliseconds: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(milliseconds); });
}

beforeEach(() => remote.listRemoteDirectories.mockReset());
afterEach(cleanup);

describe("remote path autocomplete", () => {
  it("routes generic sources through keyboard/IME navigation and discards old generations", async () => {
    const directories = vi.fn(async (path: string | null) => listing(path ?? "/home/paired", ["秘密 folder"]));
    const onSelect = vi.fn();
    const source = { key: "paired:7", directories };
    let rendered!: ReturnType<typeof render>;
    await act(async () => { rendered = render(<RemoteDirectoryPicker source={source} disabled={false} onSelect={onSelect} />); });
    const input = screen.getByRole("combobox");
    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "/home/paired/秘" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(directories).toHaveBeenCalledTimes(1);
    await act(async () => { fireEvent.compositionEnd(input); });
    await act(async () => { fireEvent.keyDown(input, { key: "Tab" }); });
    expect(onSelect).toHaveBeenLastCalledWith("/home/paired/秘密 folder");
    const next = { key: "paired:8", directories: vi.fn(async () => listing("/new/home")) };
    await act(async () => { rendered.rerender(<RemoteDirectoryPicker source={next} disabled={false} onSelect={onSelect} />); });
    expect(next.directories).toHaveBeenCalledWith(null, false);
    expect(input).toHaveValue("/new/home");
    expect(remote.listRemoteDirectories).not.toHaveBeenCalled();
  });

  it("loads children when a separator is typed without submitting navigation", async () => {
    const { input, onSelect } = await mount();
    expect(input).toHaveValue("/home/dev");
    remote.listRemoteDirectories.mockResolvedValue(listing("/home/dev/code", ["app"]));
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/code/" } }); });
    expect(screen.getByRole("option", { name: "app" })).toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith("/home/dev/code");
  });

  it("filters the final segment, selects the typed path, and completes with Tab", async () => {
    const { input, onSelect } = await mount();
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/co" } }); });
    expect(screen.getByRole("option", { name: "code" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Documents" })).not.toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith("/home/dev/co");
    remote.listRemoteDirectories.mockResolvedValue(listing("/home/dev/code", ["app"]));
    await act(async () => { fireEvent.keyDown(input, { key: "Tab" }); });
    expect(input).toHaveValue("/home/dev/code/");
    expect(screen.getByRole("option", { name: "app" })).toBeInTheDocument();
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(2);
  });

  it("keeps focus in the input while arrow keys and Enter browse a candidate", async () => {
    const { input } = await mount();
    remote.listRemoteDirectories.mockResolvedValue(listing("/home/dev/Documents", []));
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: "Documents" })).toHaveAttribute("aria-selected", "true");
    await act(async () => { fireEvent.keyDown(input, { key: "Enter" }); });
    expect(input).toHaveValue("/home/dev/Documents/");
    expect(input).toHaveFocus();
  });

  it("closes suggestions with Escape and reopens on input click", async () => {
    const { input } = await mount();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    fireEvent.click(input);
    expect(screen.getByRole("option", { name: "code" })).toBeInTheDocument();
  });

  it.each(["C:/Users/dev/code/", "C:\\Users\\dev\\code\\"])("browses Windows separator form %s", async (path) => {
    const home = listing("C:\\Users\\dev", [], "C:\\Users\\dev");
    const { input, onSelect } = await mount(home);
    expect(input).toHaveValue(home.path);
    remote.listRemoteDirectories.mockResolvedValue(listing("C:\\Users\\dev\\code", ["app"], home.path));
    await act(async () => { fireEvent.change(input, { target: { value: path } }); });
    expect(screen.getByRole("option", { name: "app" })).toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith("C:\\Users\\dev\\code");
  });

  it("treats a POSIX backslash as a literal name character", async () => {
    const { input } = await mount(listing("/home/dev", ["a\\b", "another"]));
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/a\\" } }); });
    expect(screen.getByRole("option", { name: "a\\b" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "another" })).not.toBeInTheDocument();
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
  });

  it("debounces completions to a single RPC per typing burst", async () => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      const { input } = await mountTimed(onSelect);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
      for (const value of ["/srv/r", "/srv/re", "/srv/rep", "/srv/repo"]) await type(input, value);
      await elapse(DEBOUNCE_MS - 1);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
      await elapse(1);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(2);
      expect(remote.listRemoteDirectories).toHaveBeenLastCalledWith("host", "/srv");
      expect(screen.getByRole("option", { name: "repo" })).toBeInTheDocument();
      expect(onSelect).toHaveBeenLastCalledWith("/srv/repo");
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not let a late completion overwrite a newer typed path", async () => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      const { input } = await mountTimed(onSelect);
      const late = deferred<RemoteDirectoryListing>();
      remote.listRemoteDirectories.mockImplementation(async (_host: string, path: string | null) =>
        path === "/srv" ? late.promise : listing(path ?? "/home/dev", ["app"], "/home/dev"));
      await type(input, "/srv/ap");
      await elapse(DEBOUNCE_MS);
      await type(input, "/srv/ap/other");
      await act(async () => { late.resolve(listing("/srv", ["app"])); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(input).toHaveValue("/srv/ap/other");
      expect(onSelect).toHaveBeenLastCalledWith("/srv/ap/other");
      await elapse(DEBOUNCE_MS);
      expect(remote.listRemoteDirectories).toHaveBeenLastCalledWith("host", "/srv/ap");
    } finally {
      vi.useRealTimers();
    }
  });

  it.each([
    ["/srv/repo", "/home/dev", "/srv/repo"],
    ["C:\\code\\app", "C:\\Users\\dev", "C:\\code\\app"],
    ["~/app", "/home/dev", "/home/dev/app"],
  ])("keeps the directly typed path %s selected after its completion settles", async (typed, homePath, expected) => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      remote.listRemoteDirectories.mockImplementation(async (_host: string, path: string | null) =>
        listing(path ?? homePath, ["app", "other"], homePath));
      await act(async () => { render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />); });
      await elapse(0);
      const input = screen.getByTestId("remote-repo-path-input");
      onSelect.mockClear();
      await type(input, typed);
      await elapse(DEBOUNCE_MS);
      expect(input).toHaveValue(typed);
      expect(onSelect).toHaveBeenLastCalledWith(expected);
      expect(onSelect).not.toHaveBeenCalledWith(null);
    } finally {
      vi.useRealTimers();
    }
  });

  it("expands a tilde typed before the remote home arrived", async () => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      const home = deferred<RemoteDirectoryListing>();
      remote.listRemoteDirectories.mockImplementation(async () => home.promise);
      await act(async () => { render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />); });
      const input = screen.getByTestId("remote-repo-path-input");
      await type(input, "~/app");
      expect(onSelect).not.toHaveBeenCalledWith("~/app");
      await act(async () => { home.resolve(listing("/home/dev", ["app"], "/home/dev")); });
      await elapse(0);
      expect(onSelect).toHaveBeenLastCalledWith("/home/dev/app");
    } finally {
      vi.useRealTimers();
    }
  });

  it("cancels a queued completion and ignores an in-flight one when the host changes", async () => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      const { rendered, input } = await mountTimed(onSelect);
      const stale = deferred<RemoteDirectoryListing>();
      remote.listRemoteDirectories.mockReturnValueOnce(stale.promise);
      await type(input, "/srv/");
      await type(input, "/srv/ap");
      await elapse(DEBOUNCE_MS);
      // The debounced completion shares the in-flight `/srv` listing instead of asking again.
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(2);
      expect(remote.listRemoteDirectories).toHaveBeenLastCalledWith("host", "/srv");
      // Queue a second completion and leave it un-fired, so the host switch has a debounce to cancel.
      await type(input, "/srv/other");
      const next = deferred<RemoteDirectoryListing>();
      remote.listRemoteDirectories.mockReturnValueOnce(next.promise);
      await act(async () => {
        rendered.rerender(<RemoteDirectoryPicker hostId="host2" disabled={false} onSelect={onSelect} />);
      });
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(3);
      expect(remote.listRemoteDirectories).toHaveBeenLastCalledWith("host2", null);
      await elapse(DEBOUNCE_MS * 4);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(3);
      await act(async () => { stale.resolve(listing("/srv", ["app"])); });
      await elapse(0);
      expect(input).toHaveValue("~");
      expect(screen.queryByRole("option", { name: "app" })).not.toBeInTheDocument();
      expect(onSelect).toHaveBeenLastCalledWith(null);
      await act(async () => { next.resolve(listing("/home/other")); });
      await elapse(0);
      expect(input).toHaveValue("/home/other");
      expect(onSelect).toHaveBeenLastCalledWith("/home/other");
    } finally {
      vi.useRealTimers();
    }
  });

  it("defers the completion request until IME composition ends", async () => {
    vi.useFakeTimers();
    try {
      const onSelect = vi.fn();
      const { input } = await mountTimed(onSelect);
      fireEvent.compositionStart(input);
      await type(input, "/srv/한");
      await elapse(DEBOUNCE_MS * 4);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
      await act(async () => { fireEvent.compositionEnd(input); });
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
      await elapse(DEBOUNCE_MS);
      expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(2);
      expect(remote.listRemoteDirectories).toHaveBeenLastCalledWith("host", "/srv");
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not navigate on an IME confirmation key", async () => {
    const { input } = await mount();
    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "/home/dev/한" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
  });

  it("fills the actual home path when a failed initial connection is retried", async () => {
    remote.listRemoteDirectories.mockRejectedValueOnce({ code: "IO_ERROR", message: "Connection failed" });
    const onSelect = vi.fn();
    await act(async () => {
      render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />);
    });
    remote.listRemoteDirectories.mockResolvedValue(listing("C:\\Users\\dev", [], "C:\\Users\\dev"));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
    expect(screen.getByRole("combobox")).toHaveValue("C:\\Users\\dev");
    expect(onSelect).toHaveBeenLastCalledWith("C:\\Users\\dev");
  });

  it("clears the selection when the remote rejects the typed path but keeps it on a transient error", async () => {
    // Given a mounted picker with a typed path selected.
    const { input, onSelect } = await mount();
    // When the remote answers the browse of it with a transient failure.
    remote.listRemoteDirectories.mockRejectedValueOnce({ code: "IO_ERROR", message: "Connection failed" });
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/missing/" } }); });
    // Then the typed path stays selected.
    expect(onSelect).toHaveBeenLastCalledWith("/home/dev/missing/");

    // When the remote explicitly rejects the path as nonexistent (paired DIRECTORY_NOT_FOUND or SSH NOT_FOUND).
    remote.listRemoteDirectories.mockRejectedValueOnce({ code: "DIRECTORY_NOT_FOUND", message: "No such directory" });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Refresh folders" })); });
    // Then the selection is cleared.
    expect(onSelect).toHaveBeenLastCalledWith(null);

    // And SSH NOT_FOUND / PERMISSION_DENIED likewise clear the selection.
    remote.listRemoteDirectories.mockRejectedValueOnce({ code: "NOT_FOUND", message: "Remote directory does not exist" });
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/gone/" } }); });
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });

  it.each(["", "   ", " \t "])("emits no selection and asks nothing for the blank input %j", async (value) => {
    const { input, onSelect } = await mount();
    await act(async () => { fireEvent.change(input, { target: { value } }); });
    expect(onSelect).toHaveBeenLastCalledWith(null);
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
  });

  it("does not offer stale candidates or refresh a selected folder for an empty input", async () => {
    const { input, onSelect } = await mount();
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.click(input);
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh folders" })).toBeDisabled();
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });

  it("does not navigate by clicking an option while registration disables the picker", async () => {
    remote.listRemoteDirectories.mockResolvedValue(listing("/home/dev", ["code"]));
    const onSelect = vi.fn();
    const props = { hostId: "host", onSelect };
    const rendered = render(<RemoteDirectoryPicker {...props} disabled={false} />);
    await act(async () => { await Promise.resolve(); });
    rendered.rerender(<RemoteDirectoryPicker {...props} disabled />);
    fireEvent.click(screen.getByRole("option", { name: "code" }));
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenLastCalledWith("/home/dev");
  });
});
