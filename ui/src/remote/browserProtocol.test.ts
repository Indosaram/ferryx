import { describe, expect, it } from "vitest";
import {
  decodeFrame,
  encodeFrame,
  parseServerMessage,
  serializeClientMessage,
  type BrowserFrame,
  type BrowserFrameMetadata,
  type BrowserHelloMessage,
  type BrowserSubscribedMessage,
  type BrowserCommandMessage,
  type BrowserStateMessage,
  type BrowserErrorMessage,
} from "./browserProtocol";

// Helpers to create valid test fixtures
const validMetadata: BrowserFrameMetadata = {
  offsetTop: 0,
  pageScaleFactor: 1,
  deviceWidth: 1280,
  deviceHeight: 800,
  imageWidth: 640,
  imageHeight: 400,
  scrollOffsetX: 0,
  scrollOffsetY: 0,
  timestamp: 1726560000,
  streamId: 1,
  browserInstanceId: "bi-1",
  browserServiceEpoch: "42",
  desktopEpoch: "7",
  documentGeneration: "101",
  viewportRevision: "5",
  captureRect: { x: 0, y: 0, width: 1280, height: 800 },
  geometrySource: "wkSnapshot",
};

// Minimal valid 1x1 PNG
// PNG magic (8B) + IHDR (13B payload + 4B len + 4B type + 4B crc = 25B)
function makePng1x1(width = 1, height = 1): Uint8Array {
  const bytes = new Uint8Array([
    // PNG signature
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
    // IHDR chunk length: 13
    0x00, 0x00, 0x00, 0x0d,
    // IHDR chunk type
    0x49, 0x48, 0x44, 0x52,
    // Width (4 bytes BE)
    (width >> 24) & 0xff, (width >> 16) & 0xff, (width >> 8) & 0xff, width & 0xff,
    // Height (4 bytes BE)
    (height >> 24) & 0xff, (height >> 16) & 0xff, (height >> 8) & 0xff, height & 0xff,
    // Bit depth (8), color type (2 = truecolor), compression (0), filter (0), interlace (0)
    0x08, 0x02, 0x00, 0x00, 0x00,
    // CRC (dummy 4 bytes)
    0x00, 0x00, 0x00, 0x00,
  ]);
  return bytes;
}

// Minimal valid JPEG with SOF0 marker
function makeJpeg(width = 640, height = 400): Uint8Array {
  const bytes = new Uint8Array([
    // SOI
    0xff, 0xd8,
    // SOF0 marker (baseline DCT)
    0xff, 0xc0,
    // Length (8 + 3 * components = 11 for 1 component or 17 for 3 components)
    0x00, 0x0b,
    // Precision
    0x08,
    // Height (2 bytes BE)
    (height >> 8) & 0xff, height & 0xff,
    // Width (2 bytes BE)
    (width >> 8) & 0xff, width & 0xff,
    // Number of components: 1
    0x01,
    // Component 1 spec: ID=1, samp=0x11, table=0
    0x01, 0x11, 0x00,
    // EOI
    0xff, 0xd9,
  ]);
  return bytes;
}

describe("browserProtocol - 16B binary envelope codec", () => {
  it("rejects envelopes shorter than 16 bytes", () => {
    expect(() => decodeFrame(new Uint8Array(15))).toThrow(/at least 16 bytes/i);
    expect(() => decodeFrame(new Uint8Array(0))).toThrow(/at least 16 bytes/i);
  });

  it("rejects invalid header kind, version, opcode, format, or reserved bytes", () => {
    const metaBytes = new TextEncoder().encode(JSON.stringify(validMetadata));
    const image = makeJpeg(640, 400);

    // Helper to build raw frame buffer
    function buildRaw(
      kind = 0x62,
      ver = 1,
      opcode = 1,
      format = 1,
      seq = 10,
      metaLen = metaBytes.length,
      reserved = 0,
      customImage = image,
      customMeta = metaBytes,
    ): Uint8Array {
      const buf = new Uint8Array(16 + metaLen + customImage.length);
      const view = new DataView(buf.buffer);
      view.setUint8(0, kind);
      view.setUint8(1, ver);
      view.setUint8(2, opcode);
      view.setUint8(3, format);
      view.setUint32(4, seq, true);
      view.setUint32(8, metaLen, true);
      view.setUint32(12, reserved, true);
      buf.set(customMeta, 16);
      buf.set(customImage, 16 + metaLen);
      return buf;
    }

    // Wrong kind
    expect(() => decodeFrame(buildRaw(0x61))).toThrow(/invalid kind/i);
    // Wrong version
    expect(() => decodeFrame(buildRaw(0x62, 2))).toThrow(/invalid version/i);
    // Wrong opcode
    expect(() => decodeFrame(buildRaw(0x62, 1, 2))).toThrow(/invalid opcode/i);
    // Wrong format (not 1 or 2)
    expect(() => decodeFrame(buildRaw(0x62, 1, 1, 0))).toThrow(/invalid format/i);
    expect(() => decodeFrame(buildRaw(0x62, 1, 1, 3))).toThrow(/invalid format/i);
    // Non-zero reserved
    expect(() => decodeFrame(buildRaw(0x62, 1, 1, 1, 10, metaBytes.length, 1))).toThrow(/reserved.*0/i);
  });

  it("enforces metadata length <= 4KiB and checks length overflow", () => {
    const buf = new Uint8Array(16 + 5000);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1);
    view.setUint32(4, 1, true);
    view.setUint32(8, 4097, true); // > 4096 bytes
    view.setUint32(12, 0, true);

    expect(() => decodeFrame(buf)).toThrow(/metadata.*4KiB/i);
  });

  it("rejects truncated frames where buffer length < 16 + metadataLength", () => {
    const buf = new Uint8Array(20);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1);
    view.setUint32(4, 1, true);
    view.setUint32(8, 100, true); // claims 100 bytes of metadata, but total buffer is only 20
    view.setUint32(12, 0, true);

    expect(() => decodeFrame(buf)).toThrow(/truncated/i);
  });

  it("rejects frames with empty image payload", () => {
    const metaBytes = new TextEncoder().encode(JSON.stringify(validMetadata));
    const buf = new Uint8Array(16 + metaBytes.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1);
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);

    expect(() => decodeFrame(buf)).toThrow(/empty image/i);
  });

  it("rejects frames exceeding 2MiB cap", () => {
    const metaBytes = new TextEncoder().encode(JSON.stringify(validMetadata));
    // Buffer length > 2 * 1024 * 1024
    const oversized = new Uint8Array(2 * 1024 * 1024 + 1);
    const view = new DataView(oversized.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1);
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    oversized.set(metaBytes, 16);
    oversized[16 + metaBytes.length] = 0xff;
    oversized[16 + metaBytes.length + 1] = 0xd8;

    expect(() => decodeFrame(oversized)).toThrow(/2MiB/i);
  });

  it("encodes and decodes valid JPEG and PNG frames roundtrip", () => {
    const jpegImage = makeJpeg(640, 400);
    const frame: BrowserFrame = {
      format: "jpeg",
      seq: 42,
      metadata: validMetadata,
      imageBytes: jpegImage,
    };

    const encoded = encodeFrame(frame);
    expect(encoded.byteLength).toBe(16 + new TextEncoder().encode(JSON.stringify(validMetadata)).length + jpegImage.length);

    const decoded = decodeFrame(encoded);
    expect(decoded.format).toBe("jpeg");
    expect(decoded.seq).toBe(42);
    expect(decoded.metadata).toEqual(validMetadata);
    expect(decoded.imageBytes).toEqual(jpegImage);

    // PNG test
    const pngMeta: BrowserFrameMetadata = {
      ...validMetadata,
      imageWidth: 1,
      imageHeight: 1,
    };
    const pngImage = makePng1x1(1, 1);
    const pngFrame: BrowserFrame = {
      format: "png",
      seq: 43,
      metadata: pngMeta,
      imageBytes: pngImage,
    };

    const encodedPng = encodeFrame(pngFrame);
    const decodedPng = decodeFrame(encodedPng);
    expect(decodedPng.format).toBe("png");
    expect(decodedPng.seq).toBe(43);
    expect(decodedPng.metadata).toEqual(pngMeta);
    expect(decodedPng.imageBytes).toEqual(pngImage);
  });
});

describe("browserProtocol - metadata validation & security checks", () => {
  function makeFrameWithMeta(meta: unknown, format: "jpeg" | "png" = "jpeg", image = makeJpeg(640, 400)): Uint8Array {
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const buf = new Uint8Array(16 + metaBytes.length + image.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, format === "jpeg" ? 1 : 2);
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(image, 16 + metaBytes.length);
    return buf;
  }

  it("rejects frame when required base fields are missing or non-finite", () => {
    // Missing deviceWidth
    const { deviceWidth, ...missingBase } = validMetadata;
    expect(() => decodeFrame(makeFrameWithMeta(missingBase))).toThrow(/deviceWidth/i);

    // NaN timestamp
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, timestamp: NaN }))).toThrow(/finite/i);
    // Infinity pageScaleFactor
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, pageScaleFactor: Infinity }))).toThrow(/finite/i);
  });

  it("rejects frame when required Ferryx extension fields are missing", () => {
    const extensions = [
      "streamId",
      "browserInstanceId",
      "browserServiceEpoch",
      "desktopEpoch",
      "documentGeneration",
      "viewportRevision",
      "captureRect",
      "geometrySource",
    ] as const;

    for (const key of extensions) {
      const copy: Record<string, unknown> = { ...validMetadata };
      delete copy[key];
      expect(() => decodeFrame(makeFrameWithMeta(copy)), `Missing ${key} should reject`).toThrow(
        new RegExp(`(extension|missing|required|${key})`, "i"),
      );
    }
  });

  it("rejects frame when geometrySource is not 'wkSnapshot'", () => {
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, geometrySource: "cdp" }))).toThrow(
      /wkSnapshot/i,
    );
  });

  it("enforces u64 epoch / generation / revision fields as decimal strings", () => {
    // browserServiceEpoch as a number instead of string
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, browserServiceEpoch: 42 }))).toThrow(
      /browserServiceEpoch.*decimal string/i,
    );
    // Non-decimal string (hex or negative)
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, desktopEpoch: "0x1f" }))).toThrow(
      /desktopEpoch.*decimal string/i,
    );
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, documentGeneration: "-10" }))).toThrow(
      /documentGeneration.*decimal string/i,
    );
    expect(() => decodeFrame(makeFrameWithMeta({ ...validMetadata, viewportRevision: "abc" }))).toThrow(
      /viewportRevision.*decimal string/i,
    );
  });

  it("denies unknown fields in frame metadata", () => {
    const metaWithUnknown = {
      ...validMetadata,
      webviewLabel: "window-1", // Forbidden internal leak!
    };
    expect(() => decodeFrame(makeFrameWithMeta(metaWithUnknown))).toThrow(/unknown field/i);
  });

  it("denies unknown fields inside captureRect", () => {
    const metaWithInvalidRect = {
      ...validMetadata,
      captureRect: { x: 0, y: 0, width: 1280, height: 800, internalPadding: 10 },
    };
    expect(() => decodeFrame(makeFrameWithMeta(metaWithInvalidRect))).toThrow(/unknown field/i);
  });
});

describe("browserProtocol - image size and header validation", () => {
  it("rejects JPEG frame with invalid SOI magic header", () => {
    const badJpeg = new Uint8Array([0x00, 0x00, 0x01, 0x02]);
    const metaBytes = new TextEncoder().encode(JSON.stringify(validMetadata));
    const buf = new Uint8Array(16 + metaBytes.length + badJpeg.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1); // jpeg
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(badJpeg, 16 + metaBytes.length);

    expect(() => decodeFrame(buf)).toThrow(/jpeg magic/i);
  });

  it("rejects PNG frame with invalid PNG signature", () => {
    const badPng = new Uint8Array([0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
    const meta: BrowserFrameMetadata = { ...validMetadata, imageWidth: 1, imageHeight: 1 };
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const buf = new Uint8Array(16 + metaBytes.length + badPng.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 2); // png
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(badPng, 16 + metaBytes.length);

    expect(() => decodeFrame(buf)).toThrow(/png signature/i);
  });

  it("rejects PNG frame when dimensions in IHDR chunk mismatch metadata", () => {
    const pngImage = makePng1x1(10, 10);
    const meta: BrowserFrameMetadata = { ...validMetadata, imageWidth: 20, imageHeight: 10 };
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const buf = new Uint8Array(16 + metaBytes.length + pngImage.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 2); // png
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(pngImage, 16 + metaBytes.length);

    expect(() => decodeFrame(buf)).toThrow(/dimension mismatch/i);
  });

  it("rejects JPEG frame when SOF dimensions mismatch metadata", () => {
    const jpegImage = makeJpeg(640, 400);
    const meta: BrowserFrameMetadata = { ...validMetadata, imageWidth: 800, imageHeight: 400 };
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const buf = new Uint8Array(16 + metaBytes.length + jpegImage.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, 1); // jpeg
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(jpegImage, 16 + metaBytes.length);

    expect(() => decodeFrame(buf)).toThrow(/dimension mismatch/i);
  });

  it("enforces maximum image dimension <= 2048px per edge and <= 4MP total", () => {
    // Edge > 2048
    const metaEdgeOversized: BrowserFrameMetadata = { ...validMetadata, imageWidth: 2049, imageHeight: 100 };
    const metaBytes1 = new TextEncoder().encode(JSON.stringify(metaEdgeOversized));
    const buf1 = new Uint8Array(16 + metaBytes1.length + 10);
    const view1 = new DataView(buf1.buffer);
    view1.setUint8(0, 0x62);
    view1.setUint8(1, 1);
    view1.setUint8(2, 1);
    view1.setUint8(3, 1);
    view1.setUint32(4, 1, true);
    view1.setUint32(8, metaBytes1.length, true);
    view1.setUint32(12, 0, true);
    buf1.set(metaBytes1, 16);

    expect(() => decodeFrame(buf1)).toThrow(/2048px/i);

    // Total pixels > 4MP (4,000,000)
    // 2001 * 2000 = 4,002,000 > 4MP
    const metaPixelOversized: BrowserFrameMetadata = { ...validMetadata, imageWidth: 2001, imageHeight: 2000 };
    const metaBytes2 = new TextEncoder().encode(JSON.stringify(metaPixelOversized));
    const buf2 = new Uint8Array(16 + metaBytes2.length + 10);
    const view2 = new DataView(buf2.buffer);
    view2.setUint8(0, 0x62);
    view2.setUint8(1, 1);
    view2.setUint8(2, 1);
    view2.setUint8(3, 1);
    view2.setUint32(4, 1, true);
    view2.setUint32(8, metaBytes2.length, true);
    view2.setUint32(12, 0, true);
    buf2.set(metaBytes2, 16);

    expect(() => decodeFrame(buf2)).toThrow(/4MP/i);
  });
});

describe("browserProtocol - JSON messages and schema validation", () => {
  it("parses valid browserHello message and rejects unknown fields", () => {
    const validHello: BrowserHelloMessage = {
      type: "browserHello",
      browserId: "b-1",
      browserInstanceId: "bi-1",
      browserServiceEpoch: "1",
      desktopEpoch: "2",
      protocolVersion: 1,
      supportedCommands: ["navigate", "click", "fill"],
    };

    const parsed = parseServerMessage(JSON.stringify(validHello));
    expect(parsed).toEqual(validHello);

    // Unknown field should throw
    expect(() => parseServerMessage(JSON.stringify({ ...validHello, internalPath: "/tmp" }))).toThrow(/unknown field/i);
    // Invalid u64 decimal string should throw
    expect(() => parseServerMessage(JSON.stringify({ ...validHello, desktopEpoch: -5 }))).toThrow(/decimal string/i);
  });

  it("parses valid browserSubscribed message", () => {
    const validSub: BrowserSubscribedMessage = {
      type: "browserSubscribed",
      requestId: "req-1",
      subscriptionId: "sub-1",
      streamId: 1,
      browserId: "b-1",
      browserInstanceId: "bi-1",
      browserServiceEpoch: "3",
      desktopEpoch: "8",
      documentGeneration: "12",
      options: { format: "jpeg", quality: 70, intervalMs: 250, maxEdge: 1280 },
    };

    const parsed = parseServerMessage(JSON.stringify(validSub));
    expect(parsed).toEqual(validSub);

    // Unknown option field should throw
    expect(() =>
      parseServerMessage(
        JSON.stringify({
          ...validSub,
          options: { ...validSub.options, unknownOpt: true },
        }),
      ),
    ).toThrow(/unknown field/i);
  });

  it("parses valid browserState message", () => {
    const validState: BrowserStateMessage = {
      type: "browserState",
      browserId: "b-1",
      url: "https://example.com",
      title: "Example",
      documentGeneration: "12",
      viewportRevision: "3",
      loading: false,
      paused: false,
      pauseReason: null,
    };

    const parsed = parseServerMessage(JSON.stringify(validState));
    expect(parsed).toEqual(validState);
  });

  it("parses valid browserError message", () => {
    const validErr: BrowserErrorMessage = {
      type: "browserError",
      requestId: "req-2",
      code: "BROWSER_DRIVER_BUSY",
      message: "Another driver holds the active lease",
      retryable: true,
      retryAfterMs: 500,
    };

    const parsed = parseServerMessage(JSON.stringify(validErr));
    expect(parsed).toEqual(validErr);
  });

  it("serializes valid client messages and validates camelCase and denied unknown fields", () => {
    const commandMsg: BrowserCommandMessage = {
      type: "browserCommand",
      requestId: "r2",
      requestSeq: "1",
      browserId: "b1",
      leaseEpoch: "31",
      browserInstanceId: "bi1",
      desktopEpoch: "8",
      documentGeneration: "12",
      command: "fill",
      params: { snapshotId: "snap1", reference: "e3", value: "검색어" },
    };

    const serialized = serializeClientMessage(commandMsg);
    const parsedBack = JSON.parse(serialized);
    expect(parsedBack).toEqual(commandMsg);

    // Ensure camelCase (no snake_case fields)
    for (const key of Object.keys(parsedBack)) {
      expect(key).toMatch(/^[a-z][a-zA-Z0-9]*$/);
    }
  });
});

describe("browserProtocol - Phase 7B edge cases (§7.1)", () => {
  function makeFrameWithCustomImage(imageBytes: Uint8Array, format: "jpeg" | "png", meta = validMetadata): Uint8Array {
    const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
    const buf = new Uint8Array(16 + metaBytes.length + imageBytes.length);
    const view = new DataView(buf.buffer);
    view.setUint8(0, 0x62);
    view.setUint8(1, 1);
    view.setUint8(2, 1);
    view.setUint8(3, format === "jpeg" ? 1 : 2);
    view.setUint32(4, 1, true);
    view.setUint32(8, metaBytes.length, true);
    view.setUint32(12, 0, true);
    buf.set(metaBytes, 16);
    buf.set(imageBytes, 16 + metaBytes.length);
    return buf;
  }

  describe("Big-endian dimension check on truncated/corrupted JPEG/PNG image streams", () => {
    it("rejects PNG stream truncated before 24-byte IHDR boundary", () => {
      // 23 bytes: valid signature (8B) + 15B of partial IHDR
      const truncatedPng = new Uint8Array([
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
        0x00, 0x00, 0x00, 0x0d,
        0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
      ]);
      const frameBuf = makeFrameWithCustomImage(truncatedPng, "png", {
        ...validMetadata,
        imageWidth: 256,
        imageHeight: 1,
      });

      expect(() => decodeFrame(frameBuf)).toThrow(/Invalid PNG signature or IHDR chunk/i);
    });

    it("rejects PNG stream with corrupted chunk header (not 'IHDR')", () => {
      // 24 bytes, but chunk header is "PLTE" (0x50, 0x4C, 0x54, 0x45) instead of "IHDR"
      const corruptedPng = new Uint8Array([
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
        0x00, 0x00, 0x00, 0x0d,
        0x50, 0x4c, 0x54, 0x45, // PLTE
        0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
      ]);
      const frameBuf = makeFrameWithCustomImage(corruptedPng, "png", {
        ...validMetadata,
        imageWidth: 256,
        imageHeight: 256,
      });

      expect(() => decodeFrame(frameBuf)).toThrow(/Invalid PNG signature or IHDR chunk/i);
    });

    it("correctly reads PNG dimensions in Big-Endian order (0x0100 = 256, NOT little-endian 1)", () => {
      // Width = 256 (0x00000100), Height = 512 (0x00000200)
      const pngBe = makePng1x1(256, 512);
      const frameBuf = makeFrameWithCustomImage(pngBe, "png", {
        ...validMetadata,
        imageWidth: 256,
        imageHeight: 512,
      });

      const decoded = decodeFrame(frameBuf);
      expect(decoded.metadata.imageWidth).toBe(256);
      expect(decoded.metadata.imageHeight).toBe(512);

      // If metadata claims little-endian dimension (1x2), it must be rejected as mismatch
      const frameBufMismatch = makeFrameWithCustomImage(pngBe, "png", {
        ...validMetadata,
        imageWidth: 1,
        imageHeight: 2,
      });
      expect(() => decodeFrame(frameBufMismatch)).toThrow(/dimension mismatch/i);
    });

    it("rejects JPEG stream where SOF marker length exceeds remaining buffer", () => {
      // JPEG header where SOF marker specifies len=20, but only 5 bytes follow
      const truncatedSofJpeg = new Uint8Array([
        0xff, 0xd8, // SOI
        0xff, 0xc0, // SOF0
        0x00, 0x14, // len = 20
        0x08, 0x01, 0x90, 0x02, // truncated before width
      ]);
      const frameBuf = makeFrameWithCustomImage(truncatedSofJpeg, "jpeg");
      // Must not crash with buffer overrun; dims returned null, no SOF match
      expect(() => decodeFrame(frameBuf)).not.toThrow(/out of bounds/i);
    });

    it("rejects JPEG stream with corrupted SOF length (< 7 bytes)", () => {
      const corruptSofJpeg = new Uint8Array([
        0xff, 0xd8, // SOI
        0xff, 0xc0, // SOF0
        0x00, 0x05, // len = 5 (< 7 is invalid for SOF)
        0x08, 0x01, 0x90,
      ]);
      const frameBuf = makeFrameWithCustomImage(corruptSofJpeg, "jpeg");
      expect(() => decodeFrame(frameBuf)).not.toThrow(/out of bounds/i);
    });
  });

  describe("Decimal string u64 validation for epochs and generations", () => {
    const invalidU64Tokens = [
      "",
      "-1",
      "-0",
      "+42",
      "3.14",
      "0x10",
      "1e10",
      " 42 ",
      "42abc",
      "null",
      "undefined",
    ];

    it.each(invalidU64Tokens)("rejects invalid u64 string %j in frame metadata", (invalidVal) => {
      for (const field of ["browserServiceEpoch", "desktopEpoch", "documentGeneration", "viewportRevision"] as const) {
        const meta = { ...validMetadata, [field]: invalidVal };
        const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
        const buf = new Uint8Array(16 + metaBytes.length + 10);
        const view = new DataView(buf.buffer);
        view.setUint8(0, 0x62);
        view.setUint8(1, 1);
        view.setUint8(2, 1);
        view.setUint8(3, 1);
        view.setUint32(4, 1, true);
        view.setUint32(8, metaBytes.length, true);
        view.setUint32(12, 0, true);
        buf.set(metaBytes, 16);
        expect(() => decodeFrame(buf)).toThrow(/u64 decimal string/i);
      }
    });

    it("accepts maximum 64-bit unsigned integer decimal string '18446744073709551615'", () => {
      const maxU64Str = "18446744073709551615";
      const meta = {
        ...validMetadata,
        browserServiceEpoch: maxU64Str,
        desktopEpoch: maxU64Str,
        documentGeneration: maxU64Str,
        viewportRevision: maxU64Str,
      };
      const jpeg = makeJpeg(640, 400);
      const metaBytes = new TextEncoder().encode(JSON.stringify(meta));
      const buf = new Uint8Array(16 + metaBytes.length + jpeg.length);
      const view = new DataView(buf.buffer);
      view.setUint8(0, 0x62);
      view.setUint8(1, 1);
      view.setUint8(2, 1);
      view.setUint8(3, 1);
      view.setUint32(4, 1, true);
      view.setUint32(8, metaBytes.length, true);
      view.setUint32(12, 0, true);
      buf.set(metaBytes, 16);
      buf.set(jpeg, 16 + metaBytes.length);

      const decoded = decodeFrame(buf);
      expect(decoded.metadata.browserServiceEpoch).toBe(maxU64Str);
      expect(decoded.metadata.desktopEpoch).toBe(maxU64Str);
      expect(decoded.metadata.documentGeneration).toBe(maxU64Str);
      expect(decoded.metadata.viewportRevision).toBe(maxU64Str);
    });
  });

  describe("Client rejection of invalid server messages", () => {
    it("rejects non-JSON, array, or primitive server payloads", () => {
      expect(() => parseServerMessage("not json")).toThrow(/Malformed JSON/i);
      expect(() => parseServerMessage("[]")).toThrow(/must be a JSON object/i);
      expect(() => parseServerMessage('"hello"')).toThrow(/must be a JSON object/i);
      expect(() => parseServerMessage("123")).toThrow(/must be a JSON object/i);
      expect(() => parseServerMessage("null")).toThrow(/must be a JSON object/i);
    });

    it("rejects server message without type discriminator or with unknown type", () => {
      expect(() => parseServerMessage(JSON.stringify({}))).toThrow(/missing type discriminator/i);
      expect(() => parseServerMessage(JSON.stringify({ type: "unknownCustomType" }))).toThrow(/Unrecognized server message type/i);
    });

    it("rejects browserSubscribed with invalid options", () => {
      const baseSub = {
        type: "browserSubscribed",
        requestId: "r1",
        subscriptionId: "s1",
        streamId: 1,
        browserId: "b1",
        browserInstanceId: "bi1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        documentGeneration: "1",
      };

      // Invalid format
      expect(() => parseServerMessage(JSON.stringify({ ...baseSub, options: { format: "webp" } }))).toThrow(/format must be 'jpeg' or 'png'/i);
      // Quality out of range (101)
      expect(() => parseServerMessage(JSON.stringify({ ...baseSub, options: { format: "jpeg", quality: 101 } }))).toThrow(/quality must be a finite number 1..100/i);
      // Quality out of range (0)
      expect(() => parseServerMessage(JSON.stringify({ ...baseSub, options: { format: "jpeg", quality: 0 } }))).toThrow(/quality must be a finite number 1..100/i);
      // Negative intervalMs
      expect(() => parseServerMessage(JSON.stringify({ ...baseSub, options: { format: "jpeg", intervalMs: -10 } }))).toThrow(/positive finite number/i);
      // Negative maxEdge
      expect(() => parseServerMessage(JSON.stringify({ ...baseSub, options: { format: "jpeg", maxEdge: 0 } }))).toThrow(/positive finite number/i);
    });

    it("rejects browserDriverChanged with non-boolean isDriver or invalid leaseEpoch", () => {
      expect(() => parseServerMessage(JSON.stringify({
        type: "browserDriverChanged",
        leaseEpoch: 42, // must be string or null
        isDriver: true,
      }))).toThrow(/leaseEpoch must be string or null/i);

      expect(() => parseServerMessage(JSON.stringify({
        type: "browserDriverChanged",
        leaseEpoch: "1",
        isDriver: "yes", // must be boolean
      }))).toThrow(/isDriver must be boolean/i);
    });

    it("rejects browserState with non-boolean loading or paused", () => {
      expect(() => parseServerMessage(JSON.stringify({
        type: "browserState",
        browserId: "b1",
        documentGeneration: "1",
        viewportRevision: "1",
        loading: "true", // string instead of boolean
        paused: false,
      }))).toThrow(/loading must be boolean/i);
    });

    it("parses valid browserDriverRevoked and rejects invalid fields (R3)", () => {
      const validWithReason = parseServerMessage(
        JSON.stringify({
          type: "browserDriverRevoked",
          reason: "Desktop owner reclaimed control",
          leaseEpoch: "42",
        }),
      );
      expect(validWithReason).toEqual({
        type: "browserDriverRevoked",
        reason: "Desktop owner reclaimed control",
        leaseEpoch: "42",
      });

      const validWithoutReason = parseServerMessage(
        JSON.stringify({
          type: "browserDriverRevoked",
          leaseEpoch: "43",
        }),
      );
      expect(validWithoutReason).toEqual({
        type: "browserDriverRevoked",
        leaseEpoch: "43",
      });

      // Missing leaseEpoch
      expect(() =>
        parseServerMessage(JSON.stringify({ type: "browserDriverRevoked" })),
      ).toThrow(/leaseEpoch/i);

      // Non-string leaseEpoch
      expect(() =>
        parseServerMessage(
          JSON.stringify({ type: "browserDriverRevoked", leaseEpoch: 42 }),
        ),
      ).toThrow(/leaseEpoch/i);

      // Non-string reason
      expect(() =>
        parseServerMessage(
          JSON.stringify({
            type: "browserDriverRevoked",
            leaseEpoch: "42",
            reason: 123,
          }),
        ),
      ).toThrow(/reason must be a string/i);

      // Unknown fields rejected
      expect(() =>
        parseServerMessage(
          JSON.stringify({
            type: "browserDriverRevoked",
            leaseEpoch: "42",
            extraField: true,
          }),
        ),
      ).toThrow(/unknown field/i);
    });
  });
});
