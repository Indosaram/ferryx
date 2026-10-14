import { describe, expect, it, vi } from "vitest";
import type { FilePreviewResolved } from "./filePreviewCommands";
import { createPathExistenceCache } from "./pathExistenceCache";

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("createPathExistenceCache", () => {
  it("dedupes concurrent calls for the same key so resolve is called once", async () => {
    const deferred = createDeferred<FilePreviewResolved>();
    const mockResolve = vi.fn().mockImplementation(() => deferred.promise);

    const currentTime = 1000;
    const cache = createPathExistenceCache(mockResolve, {
      ttlMs: 5000,
      now: () => currentTime,
    });

    const call1 = cache.check("src/App.tsx", "session-1");
    const call2 = cache.check("src/App.tsx", "session-1");

    expect(mockResolve).toHaveBeenCalledTimes(1);
    expect(mockResolve).toHaveBeenCalledWith("src/App.tsx", "session-1", null);

    const resolvedValue: FilePreviewResolved = {
      resolvedPath: "/path/to/src/App.tsx",
      exists: true,
      isDirectory: false,
    };
    deferred.resolve(resolvedValue);

    const [res1, res2] = await Promise.all([call1, call2]);
    expect(res1).toEqual(resolvedValue);
    expect(res2).toEqual(resolvedValue);
    expect(mockResolve).toHaveBeenCalledTimes(1);
  });

  it("does not dedupe calls for different keys concurrently", async () => {
    const d1 = createDeferred<FilePreviewResolved>();
    const d2 = createDeferred<FilePreviewResolved>();
    const mockResolve = vi.fn().mockImplementation((path: string) => {
      return path === "a.ts" ? d1.promise : d2.promise;
    });

    const cache = createPathExistenceCache(mockResolve);

    const call1 = cache.check("a.ts", "session-1");
    const call2 = cache.check("b.ts", "session-1");

    expect(mockResolve).toHaveBeenCalledTimes(2);

    d1.resolve({ resolvedPath: "/a.ts", exists: true, isDirectory: false });
    d2.resolve({ resolvedPath: "/b.ts", exists: true, isDirectory: false });

    await Promise.all([call1, call2]);
  });

  it("caches resolved values within TTL", async () => {
    let currentTime = 1000;
    const mockResolve = vi.fn().mockResolvedValue({
      resolvedPath: "/repo/file.ts",
      exists: true,
      isDirectory: false,
    });

    const cache = createPathExistenceCache(mockResolve, {
      ttlMs: 5000,
      now: () => currentTime,
    });

    const res1 = await cache.check("file.ts", "session-1");
    expect(res1?.exists).toBe(true);
    expect(mockResolve).toHaveBeenCalledTimes(1);

    currentTime = 4000;
    const res2 = await cache.check("file.ts", "session-1");
    expect(res2?.exists).toBe(true);
    expect(mockResolve).toHaveBeenCalledTimes(1);
  });

  it("refetches after TTL using injected now counter", async () => {
    let currentTime = 1000;
    const mockResolve = vi.fn().mockResolvedValue({
      resolvedPath: "/repo/file.ts",
      exists: true,
      isDirectory: false,
    });

    const cache = createPathExistenceCache(mockResolve, {
      ttlMs: 5000,
      now: () => currentTime,
    });

    await cache.check("file.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(1);

    currentTime = 6001;
    const resAfterExpiry = await cache.check("file.ts", "session-1");
    expect(resAfterExpiry?.exists).toBe(true);
    expect(mockResolve).toHaveBeenCalledTimes(2);
  });

  it("yields null on rejected resolve and does not cache the failure", async () => {
    const currentTime = 1000;
    const mockResolve = vi.fn();
    mockResolve.mockRejectedValueOnce(new Error("File not found or IPC error"));

    const cache = createPathExistenceCache(mockResolve, {
      ttlMs: 5000,
      now: () => currentTime,
    });

    const resFail = await cache.check("missing.txt", "session-1");
    expect(resFail).toBeNull();
    expect(mockResolve).toHaveBeenCalledTimes(1);

    mockResolve.mockResolvedValueOnce({
      resolvedPath: "/repo/missing.txt",
      exists: true,
      isDirectory: false,
    });

    const resSuccess = await cache.check("missing.txt", "session-1");
    expect(resSuccess).toEqual({
      resolvedPath: "/repo/missing.txt",
      exists: true,
      isDirectory: false,
    });
    expect(mockResolve).toHaveBeenCalledTimes(2);
  });

  it("evicts oldest entries when cache size exceeds maxEntries", async () => {
    const currentTime = 1000;
    const mockResolve = vi.fn().mockImplementation((path: string) =>
      Promise.resolve({
        resolvedPath: `/repo/${path}`,
        exists: true,
        isDirectory: false,
      }),
    );

    const cache = createPathExistenceCache(mockResolve, {
      ttlMs: 10000,
      maxEntries: 2,
      now: () => currentTime,
    });

    await cache.check("file1.ts", "session-1");
    await cache.check("file2.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(2);

    await cache.check("file3.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(3);

    await cache.check("file2.ts", "session-1");
    await cache.check("file3.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(3);

    await cache.check("file1.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(4);
  });

  it("clears cached entries when clear() is invoked", async () => {
    const mockResolve = vi.fn().mockResolvedValue({
      resolvedPath: "/repo/file.ts",
      exists: true,
      isDirectory: false,
    });

    const cache = createPathExistenceCache(mockResolve, { ttlMs: 5000 });

    await cache.check("file.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(1);

    cache.clear();

    await cache.check("file.ts", "session-1");
    expect(mockResolve).toHaveBeenCalledTimes(2);
  });
});
