# Ferryx Local Release Hardening: Minisign Cryptographic Verification Evidence

Date: 2026-09-08
Component: Wave 2 - Cryptographic Signature Verification (`scripts/lib/minisign-verify.mjs`, `scripts/minisign-verify.test.mjs`)
Contract: `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md` (Section 6 & Section 9)

---

## 1. Exported Canonical API

The module `scripts/lib/minisign-verify.mjs` provides pure standard-library ESM Minisign verification compatible with Node.js >= 22.22 and Bun >= 1.4 with zero external npm dependencies and zero shell invocation.

```typescript
export function verifyMinisign(options: {
  data: string | Buffer | Uint8Array;
  signature: string | Buffer | Uint8Array;
  publicKey: string | Buffer | Uint8Array;
}): boolean;

export function parsePublicKey(publicKey: string | Buffer | Uint8Array): {
  keyId: Buffer;
  rawPublicKey: Buffer;
  keyObject: import("node:crypto").KeyObject;
};

export function parseSignature(signature: string | Buffer | Uint8Array): {
  algorithm: "Ed" | "ED";
  keyId: Buffer;
  payloadSig: Buffer;
  trustedComment: string;
  trustedCommentLine: string;
  globalSig: Buffer;
};
```

### Return Value & Error Semantics
- **Success:** Returns `true` strictly when both payload signature and global/trusted comment signature verify.
- **Failure:** Throws a descriptive `Error` immediately upon any format anomaly, algorithm mismatch, key ID mismatch, length violation, or cryptographic signature failure.

---

## 2. Cryptographic Architecture & Format Specifications

### Binary Layouts (Minisign Specification)
1. **Public Key Binary (42 bytes):**
   - Bytes 0..2: Signature algorithm ID (`"Ed"`, ASCII `0x45 0x64`).
   - Bytes 2..10: Key ID (8 bytes little-endian).
   - Bytes 10..42: Ed25519 raw public key (32 bytes).
2. **Payload Signature Binary (74 bytes, Line 2 of signature file):**
   - Bytes 0..2: Signature algorithm ID (`"Ed"` for raw Ed25519, `"ED"` for Blake2b-512 prehash).
   - Bytes 2..10: Key ID (8 bytes little-endian; must match public key ID).
   - Bytes 10..74: Ed25519 signature over payload (64 bytes).
3. **Trusted Comment Signature (64 bytes, Line 4 of signature file):**
   - Bytes 0..64: Raw Ed25519 signature over `payloadSig || Buffer.from(trustedComment, "utf8")`.

### Pure Node Crypto Integration
- **Key Object Construction:** The 32-byte Ed25519 public key is wrapped in standard ASN.1 SubjectPublicKeyInfo (SPKI) DER (`30 2a 30 05 06 03 2b 65 70 03 21 00 || rawPublicKey`) and imported with `crypto.createPublicKey({ key: spkiDer, format: "der", type: "spki" })`.
- **Prehash Blake2b-512:** When algorithm is `ED`, `crypto.createHash("blake2b512").update(data).digest()` produces the 64-byte digest verified by `crypto.verify(null, digest, keyObject, payloadSig)`.
- **Legacy Raw Ed25519:** When algorithm is `Ed`, `crypto.verify(null, data, keyObject, payloadSig)` verifies directly.
- **Global Comment Signature:** Always pure Ed25519 over `Buffer.concat([payloadSig, Buffer.from(trustedComment, "utf8")])`.

### Strict "No Arbitrary Base64 Acceptance"
- Any string or buffer passed as `publicKey` or `signature` must strictly conform to Minisign formats.
- Outer Tauri base64 encoding is unwrapped only if it strictly decodes to text starting with `"untrusted comment:"`.
- Public key lines must decode to exactly 42 bytes starting with `"Ed"`. Single-line public key strings must be exactly 56 base64 characters decoding to 42 bytes.
- Signatures must contain exactly 4 lines starting with `"untrusted comment:"` and `"trusted comment:"`, with line 2 decoding to 74 bytes and line 4 decoding to 64 bytes.
- Arbitrary base64 payloads (e.g. random strings, non-minisign byte streams) are rejected immediately.

### Direct Required Minisign Oracle (No Skips, No `which` Probe)
- Test suite invokes `minisign` CLI directly as a mandatory verification oracle on the coordinator host.
- Zero conditional test skipping (`t.skip` forbidden by contract), zero non-portable shell probes (`which`).
- If `minisign` is missing, the test fails immediately with a descriptive prerequisite error indicating the missing binary in PATH.

---

## 3. TDD Progression (Red-to-Green Evidence)

### Red Phase (Failing Regression Seam)
Initial stub implementation in `scripts/lib/minisign-verify.mjs` exported `verifyMinisign` throwing `"verifyMinisign not implemented"`.

- **Command:** `node --test scripts/minisign-verify.test.mjs`
- **Output:**
```text
TAP version 13
# Subtest: verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)
not ok 1 - verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)
  ---
  duration_ms: 1.655542
  type: 'test'
  location: '/Users/indo/code/project/orca-lite/scripts/minisign-verify.test.mjs:101:1'
  failureType: 'testCodeFailure'
  error: 'verifyMinisign not implemented'
  code: 'ERR_TEST_FAILURE'
...
# Subtest: rejects missing headers, truncated lines, or malformed input types
not ok 19 - rejects missing headers, truncated lines, or malformed input types
1..19
# tests 19
# suites 0
# pass 0
# fail 19
# cancelled 0
# skipped 0
# todo 0
# duration_ms 86.379084
```
All 19 test cases failed with testCodeFailure / assertion error, establishing the RED baseline.

### Green Phase (Full Implementation Verification)
Production implementation written in `scripts/lib/minisign-verify.mjs`.

- **Command:** `node --test scripts/minisign-verify.test.mjs`
- **Exit Code:** `0`
- **Output:**
```text
TAP version 13
# Subtest: verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)
ok 1 - verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)
  ---
  duration_ms: 2.64775
  type: 'test'
  ...
# Subtest: verifies authentic repo archive fixture with raw un-wrapped Minisign text format
ok 2 - verifies authentic repo archive fixture with raw un-wrapped Minisign text format
# Subtest: verifies authentic repo archive fixture with single-line base64 public key (56-char minisign -P format)
ok 3 - verifies authentic repo archive fixture with single-line base64 public key (56-char minisign -P format)
# Subtest: supports data passed as Uint8Array or utf8 string
ok 4 - supports data passed as Uint8Array or utf8 string
# Subtest: verifies independent ED (Blake2b-512 prehash) signatures across wrapped, text, and single-line formats
ok 5 - verifies independent ED (Blake2b-512 prehash) signatures across wrapped, text, and single-line formats
# Subtest: verifies independent Ed (raw legacy Ed25519) signatures across wrapped, text, and single-line formats
ok 6 - verifies independent Ed (raw legacy Ed25519) signatures across wrapped, text, and single-line formats
# Subtest: minisign CLI oracle validates independent Node-generated fixtures, and verifyMinisign validates CLI fixtures
ok 7 - minisign CLI oracle validates independent Node-generated fixtures, and verifyMinisign validates CLI fixtures
# Subtest: rejects tampered data (single bit flip, truncation, appending)
ok 8 - rejects tampered data (single bit flip, truncation, appending)
# Subtest: rejects tampered authentic repo archive fixture data
ok 9 - rejects tampered authentic repo archive fixture data
# Subtest: rejects tampered payload signature bytes in line 2
ok 10 - rejects tampered payload signature bytes in line 2
# Subtest: rejects tampered global/trusted comment signature bytes in line 4
ok 11 - rejects tampered global/trusted comment signature bytes in line 4
# Subtest: rejects tampered key ID in signature (mismatched key ID)
ok 12 - rejects tampered key ID in signature (mismatched key ID)
# Subtest: rejects tampered key ID in public key (mismatched key ID)
ok 13 - rejects tampered key ID in public key (mismatched key ID)
# Subtest: rejects tampered trusted comment text in signature
ok 14 - rejects tampered trusted comment text in signature
# Subtest: strictly rejects arbitrary base64 strings (no arbitrary base64 acceptance)
ok 15 - strictly rejects arbitrary base64 strings (no arbitrary base64 acceptance)
# Subtest: strictly rejects invalid or unsupported algorithms in public key and signature
ok 16 - strictly rejects invalid or unsupported algorithms in public key and signature
# Subtest: strictly rejects invalid byte lengths for public key, payload signature, and global signature
ok 17 - strictly rejects invalid byte lengths for public key, payload signature, and global signature
# Subtest: rejects malformed base64 encoding (invalid characters, invalid padding)
ok 18 - rejects malformed base64 encoding (invalid characters, invalid padding)
# Subtest: rejects missing headers, truncated lines, or malformed input types
ok 19 - rejects missing headers, truncated lines, or malformed input types
1..19
# tests 19
# suites 0
# pass 19
# fail 0
# cancelled 0
# skipped 0
# todo 0
# duration_ms 78.008208
```

### Bun 1.4 Runtime Compatibility
- **Command:** `bun test scripts/minisign-verify.test.mjs`
- **Exit Code:** `0`
- **Output:**
```text
bun test v1.4.0 (34cbb9a40)

scripts/minisign-verify.test.mjs:
(pass) verifies authentic repo archive fixture with configured updater public key (Tauri base64 wrapped)
(pass) verifies authentic repo archive fixture with raw un-wrapped Minisign text format
(pass) verifies authentic repo archive fixture with single-line base64 public key (56-char minisign -P format)
(pass) supports data passed as Uint8Array or utf8 string
(pass) verifies independent ED (Blake2b-512 prehash) signatures across wrapped, text, and single-line formats
(pass) verifies independent Ed (raw legacy Ed25519) signatures across wrapped, text, and single-line formats
(pass) minisign CLI oracle validates independent Node-generated fixtures, and verifyMinisign validates CLI fixtures
(pass) rejects tampered data (single bit flip, truncation, appending)
(pass) rejects tampered authentic repo archive fixture data
(pass) rejects tampered payload signature bytes in line 2
(pass) rejects tampered global/trusted comment signature bytes in line 4
(pass) rejects tampered key ID in signature (mismatched key ID)
(pass) rejects tampered key ID in public key (mismatched key ID)
(pass) rejects tampered trusted comment text in signature
(pass) strictly rejects arbitrary base64 strings (no arbitrary base64 acceptance)
(pass) strictly rejects invalid or unsupported algorithms in public key and signature
(pass) strictly rejects invalid byte lengths for public key, payload signature, and global signature
(pass) rejects malformed base64 encoding (invalid characters, invalid padding)
(pass) rejects missing headers, truncated lines, or malformed input types

 19 pass
 0 fail
Ran 19 tests across 1 file. [253.00ms]
```

---

## 4. Manual QA Matrix & Surface Evidence

The full scenario matrix is documented in `crypto-manual-qa.md` with accompanying log artifacts under `docs/releases/local-release-hardening-evidence/`:

### Surface Scenarios
1. `CRYPTO-SCENARIO-01`: Authentic Repo Archive Fixture (`Ferryx.app.tar.gz` + `Ferryx.app.tar.gz.sig` + `src-tauri/tauri.conf.json:pubkey`). Verdict: **PASS**. Log: `crypto-qa-01-authentic-fixture.log`.
2. `CRYPTO-SCENARIO-02`: Raw Minisign 4-line text signature + 2-line text public key. Verdict: **PASS**. Log: `crypto-qa-02-raw-text-fixture.log`.
3. `CRYPTO-SCENARIO-03`: Single-line 56-character base64 public key (`minisign -P`). Verdict: **PASS**. Log: `crypto-qa-03-single-line-pubkey.log`.
4. `CRYPTO-SCENARIO-04`: Independent ED Prehash (Blake2b-512) signature verification. Verdict: **PASS**. Log: `crypto-qa-04-independent-ed-prehash.log`.
5. `CRYPTO-SCENARIO-05`: Independent Ed Raw legacy signature verification. Verdict: **PASS**. Log: `crypto-qa-05-independent-ed-raw.log`.
6. `CRYPTO-SCENARIO-06`: Minisign CLI Oracle two-way crosscheck. Verdict: **PASS**. Log: `crypto-qa-06-minisign-oracle-crosscheck.log`.

### Adversarial Scenarios
1. `CRYPTO-ADV-01`: Tampered data (1 bit flip). Rejected with `Minisign payload signature verification failed`. Verdict: **PASS**. Log: `crypto-adv-01-tampered-data.log`.
2. `CRYPTO-ADV-02`: Tampered payload signature bytes. Rejected with `Minisign payload signature verification failed`. Verdict: **PASS**. Log: `crypto-adv-02-tampered-payload-sig.log`.
3. `CRYPTO-ADV-03`: Tampered global signature bytes. Rejected with `Minisign trusted comment signature verification failed`. Verdict: **PASS**. Log: `crypto-adv-03-tampered-global-sig.log`.
4. `CRYPTO-ADV-04`: Tampered key ID in signature. Rejected with `Signature key ID (...) does not match public key ID (...)`. Verdict: **PASS**. Log: `crypto-adv-04-key-id-mismatch.log`.
5. `CRYPTO-ADV-05`: Tampered trusted comment. Rejected with `Minisign trusted comment signature verification failed`. Verdict: **PASS**. Log: `crypto-adv-05-tampered-comment.log`.
6. `CRYPTO-ADV-06`: Arbitrary base64 rejection ("no arbitrary base64 acceptance"). Non-minisign base64 rejected with format errors. Verdict: **PASS**. Log: `crypto-adv-06-arbitrary-base64-rejection.log`.
7. `CRYPTO-ADV-07`: Invalid algorithm marker (`XX`). Rejected with `Invalid signature algorithm` / `Invalid publicKey algorithm`. Verdict: **PASS**. Log: `crypto-adv-07-invalid-algorithm.log`.
8. `CRYPTO-ADV-08`: Invalid decoded byte lengths (70 bytes sig, 41 bytes pubkey, 63 bytes global sig). Rejected with strict length errors. Verdict: **PASS**. Log: `crypto-adv-08-invalid-byte-lengths.log`.

---

## 5. Clean Scope Audit

Files modified or created for this deliverable:
- `scripts/lib/minisign-verify.mjs` (production implementation)
- `scripts/minisign-verify.test.mjs` (regression and adversarial test suite, direct oracle)
- `docs/releases/local-release-hardening-evidence/crypto.md` (evidence and audit documentation)
- `docs/releases/local-release-hardening-evidence/crypto-manual-qa.md` (manual QA matrix)
- `docs/releases/local-release-hardening-evidence/crypto-*.log` (verification artifact logs)

No modifications were made to `scripts/build-latest-json.mjs` or foreign application/daemon files.
