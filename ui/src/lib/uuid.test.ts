import { describe, expect, it, vi, afterEach } from "vitest";
import { safeRandomUUID, installCryptoPolyfill } from "./uuid";

const UUID_V4_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

describe("safeRandomUUID", () => {
  const originalCrypto = globalThis.crypto;

  afterEach(() => {
    Object.defineProperty(globalThis, "crypto", {
      value: originalCrypto,
      writable: true,
      configurable: true,
    });
  });

  it("generates valid RFC 4122 v4 UUID with native crypto.randomUUID", () => {
    const id = safeRandomUUID();
    expect(id).toMatch(UUID_V4_REGEX);
  });

  it("generates unique IDs across successive invocations", () => {
    const set = new Set<string>();
    for (let i = 0; i < 100; i++) {
      const id = safeRandomUUID();
      expect(set.has(id)).toBe(false);
      set.add(id);
    }
  });

  it("falls back to getRandomValues when randomUUID is absent", () => {
    const mockGetRandomValues = vi.fn((buffer: Uint8Array) => {
      for (let i = 0; i < buffer.length; i++) {
        buffer[i] = i * 17 % 256;
      }
      return buffer;
    });

    Object.defineProperty(globalThis, "crypto", {
      value: {
        getRandomValues: mockGetRandomValues,
      },
      writable: true,
      configurable: true,
    });

    const id = safeRandomUUID();
    expect(mockGetRandomValues).toHaveBeenCalled();
    expect(id).toMatch(UUID_V4_REGEX);
  });

  it("falls back to Math.random when crypto is completely absent", () => {
    Object.defineProperty(globalThis, "crypto", {
      value: undefined,
      writable: true,
      configurable: true,
    });

    const id = safeRandomUUID();
    expect(id).toMatch(UUID_V4_REGEX);
  });

  it("handles crypto.randomUUID throwing an exception gracefully", () => {
    Object.defineProperty(globalThis, "crypto", {
      value: {
        randomUUID: () => {
          throw new Error("SecurityError: not allowed");
        },
        getRandomValues: (buffer: Uint8Array) => {
          for (let i = 0; i < buffer.length; i++) buffer[i] = 42;
          return buffer;
        },
      },
      writable: true,
      configurable: true,
    });

    const id = safeRandomUUID();
    expect(id).toMatch(UUID_V4_REGEX);
  });
});

describe("installCryptoPolyfill", () => {
  const originalCrypto = globalThis.crypto;

  afterEach(() => {
    Object.defineProperty(globalThis, "crypto", {
      value: originalCrypto,
      writable: true,
      configurable: true,
    });
  });

  it("attaches randomUUID when crypto exists but randomUUID is missing", () => {
    const customCrypto = {
      getRandomValues: (buffer: Uint8Array) => buffer,
    };
    Object.defineProperty(globalThis, "crypto", {
      value: customCrypto,
      writable: true,
      configurable: true,
    });

    installCryptoPolyfill();
    expect(typeof globalThis.crypto.randomUUID).toBe("function");
    const id = globalThis.crypto.randomUUID();
    expect(id).toMatch(UUID_V4_REGEX);
  });

  it("creates globalThis.crypto with randomUUID when crypto is undefined", () => {
    Object.defineProperty(globalThis, "crypto", {
      value: undefined,
      writable: true,
      configurable: true,
    });

    installCryptoPolyfill();
    expect(typeof globalThis.crypto).toBe("object");
    expect(typeof globalThis.crypto.randomUUID).toBe("function");
    const id = globalThis.crypto.randomUUID();
    expect(id).toMatch(UUID_V4_REGEX);
  });
});
