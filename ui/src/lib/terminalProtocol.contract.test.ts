import { describe, expect, it } from "vitest";
import fixture from "./__fixtures__/terminalProtocol.fixtures.json";
import {
  ENGINE_IDENTIFIER,
  GHOSTTY_CLEAN_REVISION,
  GHOSTTY_SNAPSHOT_FORMAT_VERSION,
  GHOSTTY_SNAPSHOT_MAGIC,
  MAX_COMPRESSED_PAYLOAD_BYTES,
  MAX_DECODED_BYTES,
  MAX_PASTE_BYTES,
  MAX_RESIZE_COLS,
  MAX_RESIZE_ROWS,
  MAX_SNAPSHOT_CHUNK_BYTES,
  SnapshotTransferAssembler,
  chunkSnapshotPayload,
  crc32c,
  decodeBase64,
  encodeBase64,
  formatBracketedPaste,
  validateEngineCompatibility,
  validateGhosttySnapshotEnvelope,
  validateInputMessage,
  validateMetadataPreAllocation,
  validatePasteMessage,
  validateResizeLeaseMessage,
} from "./terminalProtocol";

describe("terminalProtocol", () => {
  describe("constants and compatibility", () => {
    it("matches fixture constants", () => {
      expect(GHOSTTY_SNAPSHOT_MAGIC).toBe(fixture.constants.ghosttySnapshotMagic);
      expect(GHOSTTY_SNAPSHOT_FORMAT_VERSION).toBe(fixture.constants.ghosttySnapshotFormatVersion);
      expect(GHOSTTY_CLEAN_REVISION).toBe(fixture.constants.ghosttyCleanRevision);
      expect(ENGINE_IDENTIFIER).toBe(fixture.constants.engineIdentifier);
      expect(MAX_SNAPSHOT_CHUNK_BYTES).toBe(fixture.constants.maxSnapshotChunkBytes);
      expect(MAX_COMPRESSED_PAYLOAD_BYTES).toBe(fixture.constants.maxCompressedPayloadBytes);
      expect(MAX_DECODED_BYTES).toBe(fixture.constants.maxDecodedBytes);
      expect(MAX_PASTE_BYTES).toBe(fixture.constants.maxPasteBytes);
      expect(MAX_RESIZE_COLS).toBe(fixture.constants.maxResizeCols);
      expect(MAX_RESIZE_ROWS).toBe(fixture.constants.maxResizeRows);
    });

    it("validates engine compatibility and rejects mismatches", () => {
      expect(() =>
        validateEngineCompatibility({
          engine: ENGINE_IDENTIFIER,
          sourceRevision: GHOSTTY_CLEAN_REVISION,
          snapshotVersion: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
        }),
      ).not.toThrow();

      expect(() =>
        validateEngineCompatibility({
          engine: fixture.malformedRejections.incompatibleEngine,
          sourceRevision: GHOSTTY_CLEAN_REVISION,
          snapshotVersion: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
        }),
      ).toThrow("Incompatible terminal engine");

      expect(() =>
        validateEngineCompatibility({
          engine: ENGINE_IDENTIFIER,
          sourceRevision: fixture.malformedRejections.incompatibleRevision,
          snapshotVersion: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
        }),
      ).toThrow("Incompatible engine source revision");

      expect(() =>
        validateEngineCompatibility({
          engine: ENGINE_IDENTIFIER,
          sourceRevision: GHOSTTY_CLEAN_REVISION,
          snapshotVersion: 2,
        }),
      ).toThrow("Unsupported Ghostty snapshot version");
    });
  });

  describe("CRC32C checksum", () => {
    it("matches RFC 3720 Castagnoli test vectors from fixture", () => {
      for (const vec of fixture.crc32cVectors) {
        const bytes = new TextEncoder().encode(vec.inputAscii);
        const computed = crc32c(bytes);
        expect(computed).toBe(vec.expectedCrc32c);
      }
    });
  });

  describe("Ghostty snapshot envelope validation", () => {
    it("validates well-formed envelope", () => {
      const bytes = decodeBase64(fixture.envelope.base64);
      expect(bytes.length).toBe(fixture.envelope.byteLength);
      const version = validateGhosttySnapshotEnvelope(bytes);
      expect(version).toBe(1);
      expect(crc32c(bytes)).toBe(fixture.envelope.crc32c);
    });

    it("rejects truncated envelope", () => {
      const truncated = decodeBase64(fixture.malformedRejections.truncatedEnvelopeBase64);
      expect(() => validateGhosttySnapshotEnvelope(truncated)).toThrow("Snapshot envelope too short");
    });

    it("rejects invalid magic bytes", () => {
      const badMagic = decodeBase64(fixture.malformedRejections.invalidMagicBase64);
      expect(() => validateGhosttySnapshotEnvelope(badMagic)).toThrow(
        "Invalid Ghostty snapshot magic bytes",
      );
    });

    it("rejects unsupported format version", () => {
      const badVersion = decodeBase64(fixture.malformedRejections.unsupportedVersionBase64);
      expect(() => validateGhosttySnapshotEnvelope(badVersion)).toThrow(
        "Unsupported Ghostty snapshot version",
      );
    });
  });

  describe("SnapshotMetadata pre-allocation validation", () => {
    it("accepts valid metadata from fixture", () => {
      expect(() =>
        validateMetadataPreAllocation(fixture.validSnapshotAssembly.metadata),
      ).not.toThrow();
    });

    it("rejects blank snapshot ID", () => {
      expect(() =>
        validateMetadataPreAllocation({
          ...fixture.validSnapshotAssembly.metadata,
          snapshotId: "   ",
        }),
      ).toThrow("Snapshot ID cannot be blank");
    });

    it("rejects zero chunk count before allocation", () => {
      expect(() =>
        validateMetadataPreAllocation({
          ...fixture.validSnapshotAssembly.metadata,
          totalChunks: fixture.malformedRejections.zeroChunks,
        }),
      ).toThrow("Total chunks must be a positive integer");
    });

    it("rejects compressed payload exceeding 4MiB before allocation", () => {
      expect(() =>
        validateMetadataPreAllocation({
          ...fixture.validSnapshotAssembly.metadata,
          totalCompressedBytes: fixture.malformedRejections.aggregateExceedsLimitBytes,
        }),
      ).toThrow("exceeds limit");
    });

    it("rejects decoded size exceeding 32MiB safety cap before allocation", () => {
      expect(() =>
        validateMetadataPreAllocation({
          ...fixture.validSnapshotAssembly.metadata,
          totalDecodedBytes: fixture.malformedRejections.decodedExceedsLimitBytes,
        }),
      ).toThrow("exceeds 32MiB safety cap");
    });
  });

  describe("SnapshotTransferAssembler multi-chunk lifecycle", () => {
    it("assembles multi-chunk payload matching fixture", () => {
      const assembler = new SnapshotTransferAssembler();
      const meta = fixture.validSnapshotAssembly.metadata;

      assembler.handleBegin(meta);
      expect(assembler.currentGeneration).toBe(meta.generation);
      expect(assembler.activeMetadata).toEqual(meta);

      for (const chunk of fixture.validSnapshotAssembly.chunks) {
        assembler.handleChunk(
          chunk.snapshotId,
          chunk.generation,
          chunk.chunkIndex,
          chunk.chunkBase64,
          chunk.chunkCrc32c,
        );
      }

      const commit = fixture.validSnapshotAssembly.commit;
      const assembled = assembler.handleCommit(
        commit.snapshotId,
        commit.generation,
        commit.totalChunks,
        commit.totalCompressedBytes,
        commit.aggregateCrc32c,
      );

      expect(encodeBase64(assembled)).toBe(fixture.validSnapshotAssembly.expectedAssembledBase64);
      expect(assembler.activeMetadata).toBeNull();
    });

    it("enforces stale generation rejection on begin and chunk", () => {
      const assembler = new SnapshotTransferAssembler();
      assembler.handleBegin({
        ...fixture.validSnapshotAssembly.metadata,
        generation: fixture.malformedRejections.staleGenerationCurrent,
      });

      expect(() =>
        assembler.handleBegin({
          ...fixture.validSnapshotAssembly.metadata,
          generation: fixture.malformedRejections.staleGenerationIncoming,
        }),
      ).toThrow("Stale generation");

      expect(() =>
        assembler.handleChunk(
          "snap-9874-abcd",
          fixture.malformedRejections.staleGenerationIncoming,
          0,
          new Uint8Array([1, 2, 3]),
          crc32c(new Uint8Array([1, 2, 3])),
        ),
      ).toThrow("Stale generation");
    });

    it("rejects chunk exceeding 64KiB upper bound", () => {
      const assembler = new SnapshotTransferAssembler();
      assembler.handleBegin(fixture.validSnapshotAssembly.metadata);

      const oversized = new Uint8Array(fixture.malformedRejections.chunkExceedsLimitBytes);
      expect(() =>
        assembler.handleChunk("snap-9874-abcd", 1, 0, oversized, crc32c(oversized)),
      ).toThrow("exceeds maximum upper bound");
    });

    it("rejects chunk with mismatched checksum", () => {
      const assembler = new SnapshotTransferAssembler();
      assembler.handleBegin(fixture.validSnapshotAssembly.metadata);

      const chunk = fixture.validSnapshotAssembly.chunks[0];
      expect(() =>
        assembler.handleChunk(chunk.snapshotId, chunk.generation, chunk.chunkIndex, chunk.chunkBase64, 99999999),
      ).toThrow("Chunk CRC32C mismatch");
    });

    it("handles abort and clears active metadata", () => {
      const assembler = new SnapshotTransferAssembler();
      assembler.handleBegin(fixture.validSnapshotAssembly.metadata);

      expect(() =>
        assembler.handleAbort("snap-9874-abcd", 1, {
          reason: "clientCancelled",
        }),
      ).toThrow("Snapshot transfer was aborted: reason=clientCancelled");

      expect(assembler.activeMetadata).toBeNull();
    });
  });

  describe("chunkSnapshotPayload helper", () => {
    it("slices raw snapshot into compliant chunks", () => {
      const raw = decodeBase64(fixture.validSnapshotAssembly.expectedAssembledBase64);
      const split = chunkSnapshotPayload(raw, "snap-auto-1", 1);

      expect(split.metadata.snapshotId).toBe("snap-auto-1");
      expect(split.metadata.totalCompressedBytes).toBe(raw.length);
      expect(split.chunks.length).toBe(1);
      expect(split.commit.totalChunks).toBe(1);
      expect(split.commit.totalCompressedBytes).toBe(raw.length);
    });
  });

  describe("InputMessage contract", () => {
    it("accepts valid input message", () => {
      expect(() =>
        validateInputMessage({
          inputEpoch: fixture.input.valid.inputEpoch,
          inputSeq: fixture.input.valid.inputSeq,
          data: fixture.input.valid.dataBase64,
        }),
      ).not.toThrow();
    });

    it("rejects non-positive input sequence numbers", () => {
      expect(() =>
        validateInputMessage({
          inputEpoch: fixture.input.zeroSeq.inputEpoch,
          inputSeq: fixture.input.zeroSeq.inputSeq,
          data: fixture.input.zeroSeq.dataBase64,
        }),
      ).toThrow("Invalid input sequence number 0: must be positive");
    });
  });

  describe("PasteMessage and 512KiB staging cap contract", () => {
    it("accepts valid paste within cap", () => {
      expect(() =>
        validatePasteMessage({
          inputEpoch: fixture.paste.valid.inputEpoch,
          pasteId: fixture.paste.valid.pasteId,
          pasteSeq: fixture.paste.valid.pasteSeq,
          bracketed: fixture.paste.valid.bracketed,
          payload: fixture.paste.valid.payloadBase64,
        }),
      ).not.toThrow();
    });

    it("rejects blank pasteId", () => {
      expect(() =>
        validatePasteMessage({
          inputEpoch: fixture.paste.emptyId.inputEpoch,
          pasteId: fixture.paste.emptyId.pasteId,
          pasteSeq: fixture.paste.emptyId.pasteSeq,
          bracketed: fixture.paste.emptyId.bracketed,
          payload: fixture.paste.emptyId.payloadBase64,
        }),
      ).toThrow("Paste ID cannot be blank");
    });

    it("rejects payload exceeding 512KiB staging cap before staging", () => {
      const oversized = new Uint8Array(fixture.malformedRejections.pasteExceedsLimitBytes);
      expect(() =>
        validatePasteMessage({
          inputEpoch: 1,
          pasteId: "paste-oversized",
          pasteSeq: 1,
          bracketed: true,
          payload: oversized,
        }),
      ).toThrow("exceeds 512KiB staging cap");
    });

    it("formats bracketed paste markers correctly", () => {
      const payload = new TextEncoder().encode("echo test");
      const wrapped = formatBracketedPaste(payload);
      const text = new TextDecoder().decode(wrapped);
      expect(text).toBe("\x1b[200~echo test\x1b[201~");
    });
  });

  describe("ResizeLeaseMessage authenticated contract", () => {
    it("accepts valid resize lease", () => {
      expect(() =>
        validateResizeLeaseMessage(
          fixture.resizeLease.valid,
          1000,
          fixture.resizeLease.valid.signatureOrToken,
        ),
      ).not.toThrow();
    });

    it("rejects zero columns or rows", () => {
      expect(() =>
        validateResizeLeaseMessage(fixture.resizeLease.zeroCols, 1000),
      ).toThrow("Invalid terminal dimensions: cols=0, rows=40");
    });

    it("rejects rows exceeding upper bound", () => {
      expect(() =>
        validateResizeLeaseMessage(fixture.resizeLease.exceededRows, 1000),
      ).toThrow("Invalid terminal dimensions: cols=120, rows=1001");
    });

    it("rejects expired lease", () => {
      expect(() =>
        validateResizeLeaseMessage(fixture.resizeLease.expired, 5000),
      ).toThrow("Resize lease expired");
    });

    it("rejects unauthorized token mismatch", () => {
      expect(() =>
        validateResizeLeaseMessage(fixture.resizeLease.valid, 1000, "different_token"),
      ).toThrow("Resize lease token rejected as unauthorized");
    });
  });
});
