import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  FILE_PREVIEW_COMMANDS,
  FILE_PREVIEW_ENCODINGS,
  FILE_PREVIEW_ERROR_REASONS,
  FILE_PREVIEW_KINDS,
  FILE_PREVIEW_LIMITS,
  filePreviewErrorReason,
  isFilePreviewErrorReason,
} from "./filePreviewTypes";

// Vitest runs with `ui/` as its root, so the Rust contract sits one level up.
const rustContract = readFileSync(
  resolve(process.cwd(), "../src-tauri/src/ipc/file_preview_contract.rs"),
  "utf8",
);

describe("file preview shared discriminants", () => {
  it("freezes the four previewable kinds", () => {
    expect([...FILE_PREVIEW_KINDS]).toEqual(["text", "markdown", "image", "video"]);
  });

  it("freezes the nine machine error reasons", () => {
    expect([...FILE_PREVIEW_ERROR_REASONS]).toEqual([
      "MissingFile",
      "PermissionDenied",
      "NotRegularFile",
      "RemoteUnsupported",
      "TooLarge",
      "UnsupportedEncoding",
      "UnsupportedFormat",
      "FileChanged",
      "ExpiredHandle",
    ]);
  });

  it("names the three preview commands", () => {
    expect(FILE_PREVIEW_COMMANDS).toEqual({
      open: "cmd_file_preview_open",
      openChild: "cmd_file_preview_open_child",
      close: "cmd_file_preview_close",
    });
  });

  it("carries the frozen resource bounds", () => {
    expect(FILE_PREVIEW_LIMITS).toEqual({
      textMaxBytes: 2_097_152,
      maxRenderedLines: 50_000,
      imageMaxBytes: 33_554_432,
      imageMaxPixels: 40_000_000,
      maxChildHandles: 32,
      maxConcurrentMediaRequests: 8,
      streamChunkBytes: 65_536,
    });
  });
});

describe("filePreviewErrorReason", () => {
  it("reads the machine reason out of the IpcError envelope", () => {
    expect(filePreviewErrorReason({ code: "UNSUPPORTED", message: "nope", details: { reason: "TooLarge" } }))
      .toBe("TooLarge");
  });

  it("returns null for errors without a preview reason", () => {
    expect(filePreviewErrorReason(new Error("boom"))).toBeNull();
    expect(filePreviewErrorReason({ code: "IO_ERROR", message: "x" })).toBeNull();
    expect(filePreviewErrorReason({ details: { reason: 42 } })).toBeNull();
    expect(filePreviewErrorReason(null)).toBeNull();
  });

  it("never invents a reason from an unknown discriminant", () => {
    expect(filePreviewErrorReason({ details: { reason: "SomethingElse" } })).toBeNull();
  });

  it("guards unknown values", () => {
    expect(isFilePreviewErrorReason("FileChanged")).toBe(true);
    expect(isFilePreviewErrorReason("filechanged")).toBe(false);
    expect(isFilePreviewErrorReason(undefined)).toBe(false);
  });
});

describe("cross-language parity with the Rust contract", () => {
  it("declares the same error reasons as file_preview_contract.rs", () => {
    for (const reason of FILE_PREVIEW_ERROR_REASONS) {
      expect(rustContract).toContain(`Self::${reason} => "${reason}"`);
    }
  });

  it("declares the same encodings as file_preview_contract.rs", () => {
    for (const encoding of FILE_PREVIEW_ENCODINGS) {
      expect(rustContract).toContain(`#[serde(rename = "${encoding}")]`);
    }
  });

  it("keeps the Rust payload DTOs on camelCase wire keys", () => {
    const dtoBlocks = rustContract
      .split("\n")
      .map((line, index) => ({ line, index }))
      .filter(({ line }) => /^pub struct FilePreview(Payload|ChildAsset)\b/.test(line));
    expect(dtoBlocks).toHaveLength(2);
    for (const { index } of dtoBlocks) {
      const attributes = rustContract.split("\n").slice(Math.max(0, index - 3), index).join("\n");
      expect(attributes).toContain('#[serde(rename_all = "camelCase")]');
    }
  });
});
