const TOTAL_BYTES = 10 * 1024 * 1024;
const CHUNK_BYTES = 32 * 1024;
const RUNS = 3;
const BINARY_STRING_SLICE = 8 * 1024;

function syntheticChunks(): Uint8Array[] {
  const chunks: Uint8Array[] = [];
  let remaining = TOTAL_BYTES;
  let state = 0x9e3779b9 >>> 0;

  while (remaining > 0) {
    const size = Math.min(CHUNK_BYTES, remaining);
    const chunk = new Uint8Array(size);
    for (let index = 0; index < size; index += 1) {
      // xorshift32, mapped to printable ASCII to mimic terminal text without a repetitive fixture.
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      state >>>= 0;
      chunk[index] = 0x20 + (state % 95);
    }
    chunks.push(chunk);
    remaining -= size;
  }

  return chunks;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.byteLength; offset += BINARY_STRING_SLICE) {
    const slice = bytes.subarray(offset, Math.min(offset + BINARY_STRING_SLICE, bytes.byteLength));
    binary += String.fromCharCode(...slice);
  }
  return globalThis.btoa(binary);
}

function atobToUint8Array(base64: string): Uint8Array {
  const binary = globalThis.atob(base64);
  const decoded = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    decoded[index] = binary.charCodeAt(index);
  }
  return decoded;
}

function consumeDirect(chunks: Uint8Array[]): number {
  let checksum = 0;
  for (const chunk of chunks) {
    checksum = (checksum + chunk.byteLength + (chunk[0] ?? 0) + (chunk[chunk.byteLength - 1] ?? 0)) >>> 0;
  }
  return checksum;
}

function consumeLegacy(chunks: Uint8Array[]): number {
  let checksum = 0;
  for (const chunk of chunks) {
    const base64 = bytesToBase64(chunk);
    const decoded = atobToUint8Array(base64);
    checksum = (checksum + decoded.byteLength + (decoded[0] ?? 0) + (decoded[decoded.byteLength - 1] ?? 0)) >>> 0;
  }
  return checksum;
}

function timed(run: () => number): { ms: number; checksum: number } {
  const startedAt = performance.now();
  const checksum = run();
  return { ms: performance.now() - startedAt, checksum };
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

function mibPerSecond(ms: number): number {
  return (TOTAL_BYTES / (1024 * 1024)) / (ms / 1000);
}

const chunks = syntheticChunks();
const expectedChecksum = consumeDirect(chunks);

// Warm both paths before taking measurements.
consumeDirect(chunks);
consumeLegacy(chunks.slice(0, 8));

const directRuns: number[] = [];
const legacyRuns: number[] = [];
for (let run = 0; run < RUNS; run += 1) {
  const direct = timed(() => consumeDirect(chunks));
  const legacy = timed(() => consumeLegacy(chunks));
  if (direct.checksum !== expectedChecksum || legacy.checksum !== expectedChecksum) {
    throw new Error(
      `benchmark checksum mismatch: expected=${expectedChecksum} direct=${direct.checksum} legacy=${legacy.checksum}`,
    );
  }
  directRuns.push(direct.ms);
  legacyRuns.push(legacy.ms);
}

const directMs = median(directRuns);
const legacyMs = median(legacyRuns);

console.log("Terminal frontend throughput microbenchmark");
console.log(`Payload: ${(TOTAL_BYTES / (1024 * 1024)).toFixed(0)} MiB in ${chunks.length} x ${CHUNK_BYTES / 1024} KiB chunks`);
console.log(`Runs: ${RUNS}; table reports median total time`);
console.log("");
console.log("path                         total ms      MiB/s");
console.log(`${"Uint8Array pass-through".padEnd(28)} ${directMs.toFixed(3).padStart(9)} ${mibPerSecond(directMs).toFixed(1).padStart(11)}`);
console.log(`${"base64 -> atob -> Uint8Array".padEnd(28)} ${legacyMs.toFixed(3).padStart(9)} ${mibPerSecond(legacyMs).toFixed(1).padStart(11)}`);
console.log("");
console.log(`direct runs ms: ${directRuns.map((value) => value.toFixed(3)).join(", ")}`);
console.log(`legacy runs ms: ${legacyRuns.map((value) => value.toFixed(3)).join(", ")}`);
