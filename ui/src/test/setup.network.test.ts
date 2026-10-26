import { describe, expect, it, vi } from "vitest";

describe("unit test network isolation", () => {
  it("rejects unmocked fetch without contacting a server", async () => {
    await expect(fetch("https://example.invalid/api")).rejects.toThrow("mock fetch explicitly");
  });

  it("rejects unmocked XMLHttpRequest before sending", () => {
    const request = new XMLHttpRequest();
    request.open("GET", "https://example.invalid/api");
    expect(() => request.send()).toThrow("mock XMLHttpRequest explicitly");
  });

  it("allows explicit response mocks and restores the offline default", async () => {
    const mock = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("offline fixture"));
    expect(await (await fetch("https://example.invalid/api")).text()).toBe("offline fixture");
    mock.mockRestore();
    await expect(fetch("https://example.invalid/api")).rejects.toThrow("mock fetch explicitly");
  });
});
