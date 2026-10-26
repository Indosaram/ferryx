import { afterEach, describe, expect, it, vi } from "vitest";
import { createLocalGridSocket } from "./localGridSocket";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {},
}));

function deferred() {
  let resolve: () => void = () => { throw new Error("uninitialized deferred"); };
  let reject: (error: unknown) => void = () => { throw new Error("uninitialized deferred"); };
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

afterEach(() => vi.restoreAllMocks());

describe("local grid input", () => {
  it("serializes writes across sockets for the same session without blocking other sessions", async () => {
    const started = deferred();
    const release = deferred();
    const second = deferred();
    const independent = deferred();
    const writes: string[] = [];
    invoke.mockImplementation((command: string, args: { data?: string }) => {
      if (command !== "cmd_terminal_write") return Promise.resolve("stream");
      const data = args.data ?? "";
      writes.push(data);
      if (data === "first") { started.resolve(); return release.promise; }
      if (data === "second") second.resolve();
      if (data === "independent") independent.resolve();
      return Promise.resolve();
    });
    const firstSocket = createLocalGridSocket("ordered");
    const secondSocket = createLocalGridSocket("ordered");
    const other = createLocalGridSocket("other");
    firstSocket.send("first");
    secondSocket.send("second");
    other.send("independent");
    await Promise.all([started.promise, independent.promise]);
    try {
      expect(writes).toEqual(["first", "independent"]);
    } finally {
      release.resolve();
      await second.promise;
      firstSocket.close(); secondSocket.close(); other.close();
    }
    expect(writes).toEqual(["first", "independent", "second"]);
  });

  it("reports a failed write and still delivers the following interrupt", async () => {
    const started = deferred();
    const failed = deferred();
    const reported = deferred();
    const interrupted = deferred();
    const error = new Error("write rejected");
    const writes: string[] = [];
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    invoke.mockImplementation((command: string, args: { data?: string }) => {
      if (command !== "cmd_terminal_write") return Promise.resolve("stream");
      writes.push(args.data ?? "");
      if (args.data === "first") { started.resolve(); return failed.promise; }
      interrupted.resolve();
      return Promise.resolve();
    });
    const socket = createLocalGridSocket("failure");
    const onerror = vi.fn(() => reported.resolve());
    socket.onerror = onerror;
    socket.send("first");
    socket.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
    await started.promise;
    failed.reject(error);
    await Promise.all([reported.promise, interrupted.promise]);
    expect(onerror).toHaveBeenCalledWith(error);
    expect(writes).toEqual(["first", "\u0003"]);
    socket.close();
  });
});
