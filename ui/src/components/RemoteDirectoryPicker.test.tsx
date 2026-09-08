import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RemoteDirectoryListing } from "../lib/remoteDirectories";
import { RemoteDirectoryPicker } from "./RemoteDirectoryPicker";

const remote = vi.hoisted(() => ({ listRemoteDirectories: vi.fn() }));
vi.mock("../lib/remoteDirectories", () => remote);

function listing(path: string, names: string[] = [], homePath = "/home/dev"): RemoteDirectoryListing {
  return {
    path, homePath, parentPath: homePath, truncated: false,
    entries: names.map((name) => ({ name, path: `${path}/${name}`, hidden: name.startsWith(".") })),
  };
}

async function mount(home = listing("/home/dev", ["code", "Documents", ".config"])) {
  remote.listRemoteDirectories.mockResolvedValueOnce(home);
  const onSelect = vi.fn();
  await act(async () => { render(<RemoteDirectoryPicker hostId="host" disabled={false} onSelect={onSelect} />); });
  return { input: screen.getByTestId("remote-repo-path-input"), onSelect };
}

beforeEach(() => remote.listRemoteDirectories.mockReset());
afterEach(cleanup);

describe("remote path autocomplete", () => {
  it("loads children when a separator is typed without submitting navigation", async () => {
    const { input, onSelect } = await mount();
    expect(input).toHaveValue("/home/dev");
    remote.listRemoteDirectories.mockResolvedValue(listing("/home/dev/code", ["app"]));
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/code/" } }); });
    expect(screen.getByRole("option", { name: "app" })).toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith("/home/dev/code");
  });

  it("filters the final segment and completes with Tab into the next directory", async () => {
    const { input, onSelect } = await mount();
    await act(async () => { fireEvent.change(input, { target: { value: "/home/dev/co" } }); });
    expect(screen.getByRole("option", { name: "code" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Documents" })).not.toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith(null);
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

  it("does not let an old directory response overwrite a newer prefix", async () => {
    const { input, onSelect } = await mount();
    let resolve: ((value: RemoteDirectoryListing) => void) | undefined;
    const pending = new Promise<RemoteDirectoryListing>((complete) => { resolve = complete; });
    remote.listRemoteDirectories.mockReturnValue(pending);
    fireEvent.change(input, { target: { value: "/srv/" } });
    fireEvent.change(input, { target: { value: "/srv/ap" } });
    await act(async () => { resolve?.(listing("/srv", ["app", "other"])); });
    expect(input).toHaveValue("/srv/ap");
    expect(screen.getByRole("option", { name: "app" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "other" })).not.toBeInTheDocument();
    expect(onSelect).toHaveBeenLastCalledWith(null);
    expect(remote.listRemoteDirectories).toHaveBeenCalledTimes(2);
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
