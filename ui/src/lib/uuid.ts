const byteToHex: string[] = [];
for (let i = 0; i < 256; i++) {
  byteToHex[i] = (i < 16 ? "0" : "") + i.toString(16);
}

export function safeRandomUUID(): string {
  if (typeof globalThis !== "undefined" && typeof globalThis.crypto?.randomUUID === "function") {
    try {
      return globalThis.crypto.randomUUID();
    } catch {
    }
  }

  const bytes = new Uint8Array(16);
  if (typeof globalThis !== "undefined" && typeof globalThis.crypto?.getRandomValues === "function") {
    try {
      globalThis.crypto.getRandomValues(bytes);
    } catch {
      for (let i = 0; i < 16; i++) {
        bytes[i] = Math.floor(Math.random() * 256);
      }
    }
  } else {
    for (let i = 0; i < 16; i++) {
      bytes[i] = Math.floor(Math.random() * 256);
    }
  }

  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  return (
    byteToHex[bytes[0]] +
    byteToHex[bytes[1]] +
    byteToHex[bytes[2]] +
    byteToHex[bytes[3]] +
    "-" +
    byteToHex[bytes[4]] +
    byteToHex[bytes[5]] +
    "-" +
    byteToHex[bytes[6]] +
    byteToHex[bytes[7]] +
    "-" +
    byteToHex[bytes[8]] +
    byteToHex[bytes[9]] +
    "-" +
    byteToHex[bytes[10]] +
    byteToHex[bytes[11]] +
    byteToHex[bytes[12]] +
    byteToHex[bytes[13]] +
    byteToHex[bytes[14]] +
    byteToHex[bytes[15]]
  );
}

export function installCryptoPolyfill(): void {
  if (typeof globalThis === "undefined") return;

  try {
    if (!globalThis.crypto) {
      try {
        Object.defineProperty(globalThis, "crypto", {
          value: {},
          writable: true,
          configurable: true,
        });
      } catch {
        (globalThis as unknown as Record<string, unknown>).crypto = {};
      }
    }

    if (typeof globalThis.crypto?.randomUUID !== "function") {
      try {
        Object.defineProperty(globalThis.crypto, "randomUUID", {
          value: safeRandomUUID,
          writable: true,
          configurable: true,
        });
      } catch {
        (globalThis.crypto as unknown as Record<string, unknown>).randomUUID = safeRandomUUID;
      }
    }
  } catch {
  }
}

installCryptoPolyfill();
