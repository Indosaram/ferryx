# Ferryx Local Release Hardening: Cryptographic Minisign Verification Manual QA Matrix

Date: 2026-09-08
Component: Wave 2 - Cryptographic Signature Verification (`scripts/lib/minisign-verify.mjs`, `scripts/minisign-verify.test.mjs`)
Goal ID: crypto

## manualQa Matrix

### surfaceEvidence
| Scenario ID | Criterion Reference | Surface | Exact Invocation | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `CRYPTO-SCENARIO-01` | Section 6 / Authentic Tauri Updater Signature Verification | Node ESM Module API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; import fs from "node:fs"; const pubkey = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey; const sig = fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz.sig", "utf8").trim(); const data = fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz"); console.log(verifyMinisign({ data, signature: sig, publicKey: pubkey }));'` | PASS | `crypto-qa-01-authentic-fixture.log` |
| `CRYPTO-SCENARIO-02` | Section 6 / Raw Minisign Text Parsing (Unwrapped 4-Line Sig & 2-Line PubKey) | Node ESM Module API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; import fs from "node:fs"; const pubkey = Buffer.from(JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey, "base64").toString("utf8"); const sig = Buffer.from(fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz.sig", "utf8").trim(), "base64").toString("utf8"); const data = fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz"); console.log(verifyMinisign({ data, signature: sig, publicKey: pubkey }));'` | PASS | `crypto-qa-02-raw-text-fixture.log` |
| `CRYPTO-SCENARIO-03` | Section 6 / Single-Line Base64 Public Key Format (minisign -P 56-character key) | Node ESM Module API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; import fs from "node:fs"; const pubkeyText = Buffer.from(JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey, "base64").toString("utf8"); const pubkey56 = pubkeyText.split(/\r?\n/)[1].trim(); const sig = fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz.sig", "utf8").trim(); const data = fs.readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz"); console.log(verifyMinisign({ data, signature: sig, publicKey: pubkey56 }));'` | PASS | `crypto-qa-03-single-line-pubkey.log` |
| `CRYPTO-SCENARIO-04` | Section 6 / Independent ED Prehashed (Blake2b-512) Signature Verification | Node ESM Module API | `node --input-type=module -e '/* generate independent ED keypair & Blake2b-512 prehash signature */ verifyMinisign({ data, signature, publicKey })'` | PASS | `crypto-qa-04-independent-ed-prehash.log` |
| `CRYPTO-SCENARIO-05` | Section 6 / Independent Ed Raw (Legacy Ed25519) Signature Verification | Node ESM Module API | `node --input-type=module -e '/* generate independent Ed keypair & raw Ed25519 signature */ verifyMinisign({ data, signature, publicKey })'` | PASS | `crypto-qa-05-independent-ed-raw.log` |
| `CRYPTO-SCENARIO-06` | Section 6 / Minisign CLI Oracle Two-Way Cross-Verification | Minisign CLI + Node ESM API | `minisign -V -p <key.pub> -m <data> -x <sig>` and `verifyMinisign({ data, signature: cliSig, publicKey: cliPub })` | PASS | `crypto-qa-06-minisign-oracle-crosscheck.log` |

### adversarialCases
| Scenario ID | Criterion Reference | Adversarial Class | Expected Behavior | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `CRYPTO-ADV-01` | Section 6 / Data Integrity | Tampered Payload Bytes (1 bit flip) | Throws Error: `Minisign payload signature verification failed` | PASS | `crypto-adv-01-tampered-data.log` |
| `CRYPTO-ADV-02` | Section 6 / Signature Integrity | Tampered Payload Signature (1 bit flip in Line 2) | Throws Error: `Minisign payload signature verification failed` | PASS | `crypto-adv-02-tampered-payload-sig.log` |
| `CRYPTO-ADV-03` | Section 6 / Trusted Comment Binding | Tampered Global Signature (1 bit flip in Line 4) | Throws Error: `Minisign trusted comment signature verification failed` | PASS | `crypto-adv-03-tampered-global-sig.log` |
| `CRYPTO-ADV-04` | Section 6 / Strict Key ID Binding | Tampered Key ID in Signature (Mismatch Key ID) | Throws Error: `Signature key ID (...) does not match public key ID (...)` | PASS | `crypto-adv-04-key-id-mismatch.log` |
| `CRYPTO-ADV-05` | Section 6 / Comment Tampering | Modified Trusted Comment Text | Throws Error: `Minisign trusted comment signature verification failed` | PASS | `crypto-adv-05-tampered-comment.log` |
| `CRYPTO-ADV-06` | Section 6 / Input Sanitization | Arbitrary Base64 String Injection ("no arbitrary base64 acceptance") | Throws Error rejecting non-minisign arbitrary base64 strings | PASS | `crypto-adv-06-arbitrary-base64-rejection.log` |
| `CRYPTO-ADV-07` | Section 6 / Strict Algorithm ID | Invalid Algorithm Marker (`XX` instead of `Ed`/`ED`) | Throws Error: `Invalid signature algorithm` / `Invalid publicKey algorithm` | PASS | `crypto-adv-07-invalid-algorithm.log` |
| `CRYPTO-ADV-08` | Section 6 / Strict Field Lengths | Truncated/Overflow Byte Lengths (e.g. 70 bytes sig, 41 bytes pubkey) | Throws Error: `expected 42 bytes` / `expected 74 bytes` / `expected 64 bytes` | PASS | `crypto-adv-08-invalid-byte-lengths.log` |

### artifactRefs
| ID | Kind | Description | Path |
| :--- | :--- | :--- | :--- |
| `crypto-qa-01-authentic-fixture.log` | cli-log | Verification of authentic repo archive fixture against configured Tauri updater public key | `docs/releases/local-release-hardening-evidence/crypto-qa-01-authentic-fixture.log` |
| `crypto-qa-02-raw-text-fixture.log` | cli-log | Verification of authentic fixture using raw unwrapped 4-line signature and 2-line public key text | `docs/releases/local-release-hardening-evidence/crypto-qa-02-raw-text-fixture.log` |
| `crypto-qa-03-single-line-pubkey.log` | cli-log | Verification using single-line 56-character base64 public key (`minisign -P` format) | `docs/releases/local-release-hardening-evidence/crypto-qa-03-single-line-pubkey.log` |
| `crypto-qa-04-independent-ed-prehash.log` | cli-log | Verification of independently signed ED Blake2b-512 prehash signature | `docs/releases/local-release-hardening-evidence/crypto-qa-04-independent-ed-prehash.log` |
| `crypto-qa-05-independent-ed-raw.log` | cli-log | Verification of independently signed Ed raw legacy signature | `docs/releases/local-release-hardening-evidence/crypto-qa-05-independent-ed-raw.log` |
| `crypto-qa-06-minisign-oracle-crosscheck.log` | cli-log | Two-way cross-verification between minisign CLI oracle and verifyMinisign implementation | `docs/releases/local-release-hardening-evidence/crypto-qa-06-minisign-oracle-crosscheck.log` |
| `crypto-adv-01-tampered-data.log` | cli-log | Rejection error output when flipping 1 byte in payload data | `docs/releases/local-release-hardening-evidence/crypto-adv-01-tampered-data.log` |
| `crypto-adv-02-tampered-payload-sig.log` | cli-log | Rejection error output when corrupting payload signature bytes | `docs/releases/local-release-hardening-evidence/crypto-adv-02-tampered-payload-sig.log` |
| `crypto-adv-03-tampered-global-sig.log` | cli-log | Rejection error output when corrupting global/trusted comment signature bytes | `docs/releases/local-release-hardening-evidence/crypto-adv-03-tampered-global-sig.log` |
| `crypto-adv-04-key-id-mismatch.log` | cli-log | Rejection error output when signature key ID does not match public key key ID | `docs/releases/local-release-hardening-evidence/crypto-adv-04-key-id-mismatch.log` |
| `crypto-adv-05-tampered-comment.log` | cli-log | Rejection error output when trusted comment text is modified | `docs/releases/local-release-hardening-evidence/crypto-adv-05-tampered-comment.log` |
| `crypto-adv-06-arbitrary-base64-rejection.log` | cli-log | Rejection error output when arbitrary base64 strings are supplied as key or signature | `docs/releases/local-release-hardening-evidence/crypto-adv-06-arbitrary-base64-rejection.log` |
| `crypto-adv-07-invalid-algorithm.log` | cli-log | Rejection error output when algorithm marker is invalid (`XX`) | `docs/releases/local-release-hardening-evidence/crypto-adv-07-invalid-algorithm.log` |
| `crypto-adv-08-invalid-byte-lengths.log` | cli-log | Rejection error output when decoded fields have invalid byte lengths | `docs/releases/local-release-hardening-evidence/crypto-adv-08-invalid-byte-lengths.log` |
