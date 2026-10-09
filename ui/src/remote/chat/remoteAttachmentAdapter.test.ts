import { describe, expect, it, vi } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import { ATTACHMENT_MAX_FILE_BYTES } from "../../lib/scopedContracts";
import {
  AttachmentCancellationError,
  AttachmentReceiptMismatchError,
  AttachmentUploadError,
  AttachmentValidationError,
  cancelRemoteAttachment,
  computeSha256Hex,
  createRemoteAttachmentStage,
  stageRemoteAttachment,
  uint8ArrayToBase64,
  validateAttachmentFileName,
  validateAttachmentMediaType,
} from "./remoteAttachmentAdapter";

const testTarget: TargetRef = {
  hostId: "host-alpha",
  ownerId: "daemon",
  epoch: "1",
  backendSessionId: "session-42",
};

function createMockFile(name: string, content: string | Uint8Array, type = ""): File {
  const sourceBytes = typeof content === "string" ? new TextEncoder().encode(content) : content;
  const copy = new Uint8Array(sourceBytes.byteLength);
  copy.set(sourceBytes);

  const file = new File([copy], name, { type });
  Object.defineProperty(file, "size", { value: copy.byteLength, writable: false });
  Object.defineProperty(file, "name", { value: name, writable: false });
  Object.defineProperty(file, "type", { value: type, writable: false });

  Object.defineProperty(file, "arrayBuffer", {
    value: async () => copy.buffer.slice(copy.byteOffset, copy.byteOffset + copy.byteLength),
    writable: true,
    configurable: true,
  });

  Object.defineProperty(file, "text", {
    value: async () => new TextDecoder().decode(copy),
    writable: true,
    configurable: true,
  });

  Object.defineProperty(file, "slice", {
    value: (start?: number, end?: number, contentType?: string) => {
      const len = copy.byteLength;
      const s = start !== undefined ? (start < 0 ? Math.max(len + start, 0) : Math.min(start, len)) : 0;
      const e = end !== undefined ? (end < 0 ? Math.max(len + end, 0) : Math.min(end, len)) : len;
      const sliced = copy.subarray(s, Math.max(s, e));
      return createMockFile(name, sliced, contentType ?? type);
    },
    writable: true,
    configurable: true,
  });

  return file;
}

describe("remoteAttachmentAdapter - Filename Validation", () => {
  it("accepts valid ASCII and Unicode file names", () => {
    expect(validateAttachmentFileName("report.pdf")).toBe("report.pdf");
    expect(validateAttachmentFileName("설계_다이어그램.png")).toBe("설계_다이어그램.png");
    expect(validateAttachmentFileName("résumé.txt")).toBe("résumé.txt");
    expect(validateAttachmentFileName("project_v1.2..final.png")).toBe("project_v1.2..final.png");
  });

  it("rejects empty or whitespace-only names", () => {
    expect(() => validateAttachmentFileName("")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("   ")).toThrow(AttachmentValidationError);
  });

  it("rejects forward and backward slash path separators", () => {
    expect(() => validateAttachmentFileName("foo/bar.png")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("foo\\bar.png")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("../../etc/passwd")).toThrow(AttachmentValidationError);
  });

  it("rejects relative directory components", () => {
    expect(() => validateAttachmentFileName(".")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("..")).toThrow(AttachmentValidationError);
  });

  it("rejects control characters and null bytes", () => {
    expect(() => validateAttachmentFileName("test\0file.png")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("test\x1bfile.png")).toThrow(AttachmentValidationError);
    expect(() => validateAttachmentFileName("test\r\n.png")).toThrow(AttachmentValidationError);
  });

  it("rejects file names exceeding 255 bytes in UTF-8", () => {
    const longName = "a".repeat(256) + ".png";
    expect(() => validateAttachmentFileName(longName)).toThrow(AttachmentValidationError);

    const longUnicode = "한".repeat(86) + ".png";
    expect(() => validateAttachmentFileName(longUnicode)).toThrow(AttachmentValidationError);
  });
});

describe("remoteAttachmentAdapter - Media Type Validation", () => {
  it("accepts explicitly allowed MIME types", () => {
    expect(validateAttachmentMediaType("image/png", "file.bin")).toBe("image/png");
    expect(validateAttachmentMediaType("IMAGE/JPEG", "file.bin")).toBe("image/jpeg");
    expect(validateAttachmentMediaType("image/webp", "file.bin")).toBe("image/webp");
    expect(validateAttachmentMediaType("text/plain", "file.bin")).toBe("text/plain");
    expect(validateAttachmentMediaType("application/pdf", "file.bin")).toBe("application/pdf");
  });

  it("infers media type from file extension when type is empty", () => {
    expect(validateAttachmentMediaType("", "photo.png")).toBe("image/png");
    expect(validateAttachmentMediaType("", "photo.jpg")).toBe("image/jpeg");
    expect(validateAttachmentMediaType("", "photo.jpeg")).toBe("image/jpeg");
    expect(validateAttachmentMediaType("", "photo.webp")).toBe("image/webp");
    expect(validateAttachmentMediaType("", "notes.txt")).toBe("text/plain");
    expect(validateAttachmentMediaType("", "build.log")).toBe("text/plain");
    expect(validateAttachmentMediaType("", "doc.pdf")).toBe("application/pdf");
  });

  it("rejects unsupported media types", () => {
    expect(() => validateAttachmentMediaType("application/zip", "archive.zip")).toThrow(
      AttachmentValidationError
    );
    expect(() => validateAttachmentMediaType("video/mp4", "movie.mp4")).toThrow(
      AttachmentValidationError
    );
    expect(() => validateAttachmentMediaType("image/gif", "anim.gif")).toThrow(
      AttachmentValidationError
    );
  });
});

describe("remoteAttachmentAdapter - Utilities", () => {
  it("encodes uint8 array to base64 deterministically", () => {
    const input = new TextEncoder().encode("Hello, World!");
    expect(uint8ArrayToBase64(input)).toBe("SGVsbG8sIFdvcmxkIQ==");
  });

  it("computes lowercase hex sha256 correctly", async () => {
    const input = new TextEncoder().encode("hello world").buffer;
    const hash = await computeSha256Hex(input);
    expect(hash).toBe("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
  });
});

describe("remoteAttachmentAdapter - stageRemoteAttachment", () => {
  it("rejects files exceeding ATTACHMENT_MAX_FILE_BYTES before fetch", async () => {
    const hugeFile = {
      name: "huge.pdf",
      size: ATTACHMENT_MAX_FILE_BYTES + 1,
      type: "application/pdf",
      arrayBuffer: vi.fn(),
    } as unknown as File;

    const mockFetch = vi.fn();
    const abortController = new AbortController();

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-1", fetchFn: mockFetch },
        testTarget,
        hugeFile,
        abortController.signal
      )
    ).rejects.toThrow(AttachmentValidationError);

    expect(mockFetch).not.toHaveBeenCalled();
  });

  it("stages single chunk file successfully and returns validated receipt", async () => {
    const textContent = "Ferryx Attachment Test Content";
    const file = createMockFile("test.txt", textContent, "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    let capturedPayload: Record<string, unknown> | null = null;
    let capturedAuth: string | null = null;

    const mockFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      capturedAuth = (init.headers as Record<string, string>)?.Authorization ?? null;
      capturedPayload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: {
            hostId: testTarget.hostId,
            attachmentId: capturedPayload?.attachmentId,
            sha256: localHash,
            sizeBytes: file.size,
            mediaType: "text/plain",
          },
          requestId: "req-1",
        }),
      } as unknown as Response;
    });

    const abortController = new AbortController();
    const receipt = await stageRemoteAttachment(
      { baseUrl: "http://127.0.0.1:8899", token: "tok-secret", fetchFn: mockFetch },
      testTarget,
      file,
      abortController.signal
    );

    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(capturedAuth).toBe("Bearer tok-secret");
    expect(capturedPayload).not.toBeNull();
    expect(capturedPayload?.chunkIndex).toBe(0);
    expect(capturedPayload?.totalChunks).toBe(1);
    expect(capturedPayload?.offset).toBe(0);
    expect(capturedPayload?.totalBytes).toBe(file.size);
    expect(capturedPayload?.fileName).toBe("test.txt");
    expect(capturedPayload?.mediaType).toBe("text/plain");

    expect(receipt).toEqual({
      hostId: testTarget.hostId,
      attachmentId: capturedPayload?.attachmentId,
      sha256: localHash,
      sizeBytes: file.size,
      mediaType: "text/plain",
    });
  });

  it("stages multi-chunk file sending sequential chunks with exact offsets and requires ok:true,data envelope", async () => {
    const data = new Uint8Array(50 * 1024);
    for (let i = 0; i < data.length; i++) data[i] = i % 256;
    const file = createMockFile("large.png", data, "image/png");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const chunkSize = 20 * 1024;
    const chunkCalls: Record<string, unknown>[] = [];

    const mockFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      chunkCalls.push(payload);

      if (payload.chunkIndex === 2) {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            ok: true,
            data: {
              hostId: testTarget.hostId,
              attachmentId: payload.attachmentId,
              sha256: localHash,
              sizeBytes: file.size,
              mediaType: "image/png",
            },
          }),
        } as unknown as Response;
      }

      return {
        ok: true,
        status: 200,
        json: async () => ({ ok: true, chunkIndex: payload.chunkIndex }),
      } as unknown as Response;
    });

    const abortController = new AbortController();
    const receipt = await stageRemoteAttachment(
      { baseUrl: "http://127.0.0.1:8899", token: "tok-2", chunkSize, fetchFn: mockFetch },
      testTarget,
      file,
      abortController.signal
    );

    expect(mockFetch).toHaveBeenCalledTimes(3);
    expect(chunkCalls[0].chunkIndex).toBe(0);
    expect(chunkCalls[0].offset).toBe(0);
    expect(chunkCalls[0].totalChunks).toBe(3);

    expect(chunkCalls[1].chunkIndex).toBe(1);
    expect(chunkCalls[1].offset).toBe(20 * 1024);
    expect(chunkCalls[1].totalChunks).toBe(3);

    expect(chunkCalls[2].chunkIndex).toBe(2);
    expect(chunkCalls[2].offset).toBe(40 * 1024);
    expect(chunkCalls[2].totalChunks).toBe(3);

    expect(receipt.sha256).toBe(localHash);
    expect(receipt.sizeBytes).toBe(50 * 1024);
  });

  it("rejects server response using fallback receipt field or bare record", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const mockFetchFallback = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          receipt: {
            hostId: testTarget.hostId,
            attachmentId: payload.attachmentId,
            sha256: localHash,
            sizeBytes: file.size,
            mediaType: "text/plain",
          },
        }),
      } as unknown as Response;
    });

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-fallback", fetchFn: mockFetchFallback },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);

    const mockFetchBare = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          hostId: testTarget.hostId,
          attachmentId: payload.attachmentId,
          sha256: localHash,
          sizeBytes: file.size,
          mediaType: "text/plain",
        }),
      } as unknown as Response;
    });

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-bare", fetchFn: mockFetchBare },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);
  });

  it("rejects server receipt when hostId mismatches target", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        ok: true,
        data: {
          hostId: "rogue-host-id",
          attachmentId: "att-1",
          sha256: localHash,
          sizeBytes: file.size,
          mediaType: "text/plain",
        },
      }),
    } as unknown as Response);

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-3", fetchFn: mockFetch },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);
  });

  it("rejects server receipt when sha256 checksum mismatches", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");

    const mockFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: {
            hostId: testTarget.hostId,
            attachmentId: payload.attachmentId,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            sizeBytes: file.size,
            mediaType: "text/plain",
          },
        }),
      } as unknown as Response;
    });

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-4", fetchFn: mockFetch },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);
  });

  it("rejects server receipt when sizeBytes mismatches", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const mockFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: {
            hostId: testTarget.hostId,
            attachmentId: payload.attachmentId,
            sha256: localHash,
            sizeBytes: file.size + 999,
            mediaType: "text/plain",
          },
        }),
      } as unknown as Response;
    });

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-5", fetchFn: mockFetch },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);
  });

  it("rejects response leaking raw host path in envelope or receipt", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const mockFetchWithPath = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        remotePath: "/tmp/ferryx-paste/secret.txt",
        data: {
          hostId: testTarget.hostId,
          attachmentId: payload.attachmentId,
          sha256: localHash,
          sizeBytes: file.size,
          mediaType: "text/plain",
        },
      };
    });

    const mockFetchWrapper = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => mockFetchWithPath("", {} as RequestInit),
    } as unknown as Response);

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-6", fetchFn: mockFetchWrapper },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentReceiptMismatchError);
  });

  it("handles HTTP error status from gateway", async () => {
    const file = createMockFile("notes.txt", "data", "text/plain");

    const mockFetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 413,
      json: async () => ({
        ok: false,
        error: { code: "PAYLOAD_TOO_LARGE", message: "File exceeds server quota" },
      }),
    } as unknown as Response);

    await expect(
      stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-7", fetchFn: mockFetch },
        testTarget,
        file,
        new AbortController().signal
      )
    ).rejects.toThrow(AttachmentUploadError);
  });
});

describe("remoteAttachmentAdapter - Cancellation & Cleanup", () => {
  it("calls authenticated cancel on abort and marks serverStateCleaned true when acknowledged with cleaned:true", async () => {
    const file = createMockFile("doc.pdf", "sample pdf content", "application/pdf");
    const abortController = new AbortController();

    const fetchMock = vi.fn().mockImplementation(async (url: string) => {
      if (url.endsWith("/cancel")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({ ok: true, cleaned: true }),
        } as unknown as Response;
      }
      abortController.abort();
      throw new DOMException("The user aborted a request.", "AbortError");
    });

    let caughtError: unknown = null;
    try {
      await stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-cancel", fetchFn: fetchMock },
        testTarget,
        file,
        abortController.signal
      );
    } catch (err) {
      caughtError = err;
    }

    expect(caughtError).toBeInstanceOf(AttachmentCancellationError);
    const cancelErr = caughtError as AttachmentCancellationError;
    expect(cancelErr.serverStateCleaned).toBe(true);

    const cancelCall = fetchMock.mock.calls.find((call) =>
      (call[0] as string).endsWith("/api/v1/chat/attachments/cancel")
    );
    expect(cancelCall).toBeDefined();
    const cancelBody = JSON.parse(cancelCall![1].body as string) as Record<string, unknown>;
    expect(cancelBody.target).toEqual(testTarget);
    expect(typeof cancelBody.attachmentId).toBe("string");
  });

  it("does not mislabel serverStateCleaned as true when cancel response omits cleaned or sets cleaned:false", async () => {
    const file = createMockFile("doc.pdf", "sample pdf content", "application/pdf");
    const abortController = new AbortController();

    const fetchMockOmitted = vi.fn().mockImplementation(async (url: string) => {
      if (url.endsWith("/cancel")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({ ok: true }),
        } as unknown as Response;
      }
      abortController.abort();
      throw new DOMException("The user aborted a request.", "AbortError");
    });

    let caughtOmitted: unknown = null;
    try {
      await stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-cancel", fetchFn: fetchMockOmitted },
        testTarget,
        file,
        abortController.signal
      );
    } catch (err) {
      caughtOmitted = err;
    }

    expect(caughtOmitted).toBeInstanceOf(AttachmentCancellationError);
    expect((caughtOmitted as AttachmentCancellationError).serverStateCleaned).toBe(false);

    const abortController2 = new AbortController();
    const fetchMockFalse = vi.fn().mockImplementation(async (url: string) => {
      if (url.endsWith("/cancel")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({ ok: true, cleaned: false }),
        } as unknown as Response;
      }
      abortController2.abort();
      throw new DOMException("The user aborted a request.", "AbortError");
    });

    let caughtFalse: unknown = null;
    try {
      await stageRemoteAttachment(
        { baseUrl: "http://127.0.0.1:8899", token: "tok-cancel", fetchFn: fetchMockFalse },
        testTarget,
        file,
        abortController2.signal
      );
    } catch (err) {
      caughtFalse = err;
    }

    expect(caughtFalse).toBeInstanceOf(AttachmentCancellationError);
    expect((caughtFalse as AttachmentCancellationError).serverStateCleaned).toBe(false);
  });

  it("bounds cancelRemoteAttachment with explicit timeout signal and fails cleanly on timeout", async () => {
    const hangingFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      return new Promise<Response>((_, reject) => {
        if (init.signal) {
          init.signal.addEventListener("abort", () => {
            reject(new DOMException("The operation was aborted due to timeout.", "AbortError"));
          });
        }
      });
    });

    const startTime = Date.now();
    const acknowledged = await cancelRemoteAttachment(
      {
        baseUrl: "http://127.0.0.1:8899",
        token: "tok-timeout",
        cancelTimeoutMs: 25,
        fetchFn: hangingFetch,
      },
      testTarget,
      "att-hanging"
    );

    const elapsed = Date.now() - startTime;
    expect(acknowledged).toBe(false);
    expect(elapsed).toBeGreaterThanOrEqual(20);
    expect(hangingFetch).toHaveBeenCalledTimes(1);
  });

  it("cancelRemoteAttachment helper handles network drops gracefully", async () => {
    const failingFetch = vi.fn().mockRejectedValue(new Error("Network connection dropped"));
    const acknowledged = await cancelRemoteAttachment(
      { baseUrl: "http://127.0.0.1:8899", token: "tok", fetchFn: failingFetch },
      testTarget,
      "att-99"
    );
    expect(acknowledged).toBe(false);
  });
});

describe("remoteAttachmentAdapter - createRemoteAttachmentStage", () => {
  it("returns bound stage function conforming to ChatService.stage interface", async () => {
    const file = createMockFile("notes.txt", "bound stage content", "text/plain");
    const localHash = await computeSha256Hex(await file.arrayBuffer());

    const mockFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const payload = JSON.parse(init.body as string) as Record<string, unknown>;
      return {
        ok: true,
        status: 200,
        json: async () => ({
          ok: true,
          data: {
            hostId: testTarget.hostId,
            attachmentId: payload.attachmentId,
            sha256: localHash,
            sizeBytes: file.size,
            mediaType: "text/plain",
          },
        }),
      } as unknown as Response;
    });

    const stageFn = createRemoteAttachmentStage({
      baseUrl: "http://127.0.0.1:8899",
      token: "tok-bound",
      fetchFn: mockFetch,
    });

    const receipt = await stageFn(testTarget, file, new AbortController().signal);
    expect(receipt.hostId).toBe(testTarget.hostId);
    expect(receipt.sha256).toBe(localHash);
  });
});
