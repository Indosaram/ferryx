# INVARIANTS-rust.md

> Ferryx note: this crate is a port of Maho's `maho-import`. The invariants below still hold
> for the parsers, decrypt paths, workers and orchestrator. The integration sections describe
> Maho (its `maho-ffi` panic boundary and C++ bridge); in Ferryx the caller is
> `src-tauri/src/browser/import/`, which runs imports on a blocking thread through
> `ipc::run_blocking`. Line references are to the Maho source. SQLite databases are now read
> from a temporary copy (`src/sqlite_snapshot.rs`) instead of `immutable=1`.

Safety contract for the Rust-side Maho Importer (`maho-import` crate).

This document supersedes `maho-chromium/browser/importer/INVARIANTS.md`, which
described the pre-migration C++ subsystem. The orchestrator, workers, parsers,
and decrypt paths now live in pure Rust; only a thin progress/lifecycle bridge
remains on the C++ side (`MahoImportSessionBridge`).

Read this file before modifying any code under `maho/crates/maho-import/` or
`maho-chromium/browser/importer/`.

## Invariants

- **INV-1** (preserved): No parser returns success with zero items.
- **INV-2** (REMOVED): Cross-thread parser/UI dispatch is no longer a parser
  concern — see "Removed" below.
- **INV-3** (preserved, generalized): Parsers MUST NOT panic; failures are
  returned as `Result<_, ImportError>`.
- **INV-4** (preserved): Cookie/password parsers MUST NOT bypass the 3-byte
  decrypt prefix check (`v10` / `v20`).
- **INV-5** (NEW): Progress callbacks fire from the Rust runtime thread; the C++
  consumer MUST `PostTask` to the UI sequence before touching browser state.
- **INV-6** (preserved): Plaintext key material is zeroed via
  `zeroize::Zeroize` / `Zeroizing<T>` (replaces the C++ RAII `SecureBuffer`).

---

### INV-1 — Empty parser results are not success

**Statement.** A parser that finds no candidate file or extracts zero records
MUST return `Err(ImportError::FileNotFound(_))` or `Err(ImportError::Parse(_))`,
not `Ok(vec![])`. The orchestrator and workers may legitimately produce a zero
count for a *worker* (e.g. Safari has no cookies — see
`workers/cookie.rs:31-33`), but the underlying *parser* never claims success
without data.

**Rationale.** Silent "success" hides corrupt profiles, schema changes, and
missing files behind a passing import. Forcing parsers to fail loudly keeps
diagnostics actionable.

**Enforcement.**
- `src/parsers/chromium/cookies.rs:77-81` — missing `Cookies` file →
  `ImportError::FileNotFound`.
- `src/parsers/chromium/cookies.rs:85-87` — missing `cookies` table →
  `ImportError::Parse`.
- `src/decrypt/firefox_nss.rs:206-207`, `:222-223` — empty salt/key fields →
  `NssError::Database`.
- Tests: `workers::cookie::tests::test_cookie_worker_nonexistent_firefox`
  (`src/workers/cookie.rs:147-162`) asserts the error path.

**Failure mode.** A user reports "import said success but nothing arrived";
absent INV-1, every parser failure becomes silent.

---

### INV-3 — Parsers MUST NOT panic

**Statement.** Every public function under `src/parsers/`, `src/decrypt/`,
`src/workers/`, and `src/orchestrator.rs` returns `Result<_, ImportError>` (or
a more specific `NssError` / `KeychainError` / `DecryptError`). No `unwrap()`,
`expect()`, slice indexing, or arithmetic panic is permitted on
attacker-controlled input (untrusted SQLite blobs, JSON, ASN.1).

**Rationale.** Panicking unwinds across the FFI boundary into Chromium C++,
which is undefined behavior. A malformed `Cookies` DB or a tampered
`logins.json` must not crash the browser process.

**Enforcement.**
- `lib.rs:100-114` — `ImportError` is the canonical parser error type.
- `src/decrypt/firefox_nss.rs:27-42` — `NssError` for all NSS paths.
- `src/decrypt/chromium_keychain.rs:18-41` — `KeychainError` / `DecryptError`.
- `src/decrypt/chromium_keychain.rs:114-123` — the only `expect()` is on
  PBKDF2-HMAC initialization, which is infallible for any key length (documented
  in the call comment).
- The FFI layer (`maho-ffi`) is the lone panic boundary — wraps Rust calls in
  `catch_unwind` before crossing back to C++.

**Failure mode.** Browser-wide crash on import; no diagnostic, no graceful
"that profile is corrupt" path.

---

### INV-4 — Decrypt prefix checks are mandatory

**Statement.** Before attempting AES-128-CBC decryption of a Chromium cookie or
password blob, the code MUST inspect the first 3 bytes and dispatch on
`v10` / `v20` / unknown. Unknown or short blobs MUST be skipped, not fed
through the decryptor.

**Rationale.** Feeding non-`v10` bytes into AES-CBC produces garbage that may
*decrypt cleanly* (valid PKCS#7 padding by chance) and corrupt the destination
store. The 3-byte prefix is the only authentic signal that a value was
produced by Chromium's encryption pipeline. `v20` (App-Bound, Windows-only) is
recognized and explicitly skipped on macOS until Phase 3+ support lands.

**Enforcement.**
- `src/parsers/chromium/cookies.rs:54-66` — `detect_encryption_version` is the
  only entry point that classifies the prefix.
- `src/parsers/chromium/cookies.rs:133-138` — parser checks the prefix before
  emitting the row; encrypted values are stored as `encrypted_value` (raw bytes)
  with the detected `EncryptionVersion`, never decrypted in-place at parse time.
- `src/decrypt/chromium_keychain.rs:139-151` — `decrypt_v10` re-checks the
  prefix and returns `DecryptError::MissingPrefix` on mismatch (defense in
  depth).
- `src/parsers/chromium/passwords.rs:87-89` — `MissingPrefix` and
  `InvalidLength` are continued, not propagated, so one bad row never poisons
  the batch.
- Tests: `decrypt_v10_rejects_wrong_prefix` and `decrypt_v10_rejects_no_prefix`
  in `src/decrypt/chromium_keychain.rs:241-256`; `detects_v20_prefix` in
  `src/parsers/chromium/cookies.rs:301`.

**Failure mode.** Garbage cookie/password rows written to `maho-core` storage;
corrupted user profile; silent data loss on the source side if the user
re-syncs.

---

### INV-5 — Progress callbacks fire from the Rust runtime thread

**Statement.** The progress callback registered via
`maho_import_orchestrator_start` is invoked on whichever thread the Rust
orchestrator happens to be running on (today: the caller's thread; future: a
dedicated worker pool). The C++ consumer MUST `PostTask` to its UI
`SequencedTaskRunner` before touching any browser state, WebUI handler, or
`MahoCore`.

**Rationale.** This replaces the old INV-2 / INV-8 pair. The Rust side no
longer knows or cares about the UI thread; threading is purely a C++
correctness concern at the FFI boundary.

**Enforcement.**
- `maho-chromium/browser/importer/maho_import_session_bridge.h:33-88` — bridge
  owns a `scoped_refptr<base::SequencedTaskRunner>` captured at construction.
- `maho-chromium/browser/importer/maho_import_session_bridge.cc:66-79` — the
  static `OnProgress` trampoline copies the message into a `std::string` and
  `PostTask`s to `HandleProgressOnUI`. No browser-state access happens on the
  Rust thread.
- `src/orchestrator.rs:6-11` — orchestrator doc-comment pins the design:
  "Workers run synchronously on the calling thread (no spawn) — the C++ side
  already posts to a background thread before calling into Rust."
- INV-7 from the old C++ doc (single `OnImportProgress(complete=true)` caller)
  is now structurally enforced: `ImportProgress::AllComplete` is sent from
  exactly one site in `src/orchestrator.rs`; workers cannot construct it (see
  the doc on `ImportWorker::run` at `src/workers/mod.rs:24-36`).

**Failure mode.** UAF / data races inside Chromium when the Rust thread mutates
WebUI handler state directly; `DCHECK_CALLED_ON_VALID_SEQUENCE` failures in
debug builds; subtle corruption in release.

---

### INV-6 — Plaintext key material is zeroed

**Statement.** Every byte of decrypted key material — NSS master keys, derived
AES-128 keys, Keychain passwords, intermediate buffers holding cleartext during
decrypt — is wrapped in `zeroize::Zeroizing<T>` or a struct with
`#[derive(Zeroize)] #[zeroize(drop)]`. No plaintext key, password, or master
secret is logged, returned by `Debug`, or copied into a `Vec<u8>` that lacks a
zeroizing drop.

**Rationale.** Replaces the C++ RAII `SecureBuffer` and `memset_s` pattern with
a stronger Rust-native equivalent: the compiler tracks ownership, and `Drop`
guarantees the wipe runs even on panic.

**Enforcement.**
- `src/decrypt/chromium_keychain.rs:96-105` — `ChromiumKey` derives
  `Zeroize` with `#[zeroize(drop)]`.
- `src/decrypt/chromium_keychain.rs:60-61`, `:87-90` — `KeychainProvider`
  returns `Zeroizing<Vec<u8>>` for the raw Keychain password.
- `src/decrypt/chromium_keychain.rs:153-154` — decrypt scratch buffer is
  `Zeroizing<Vec<u8>>`.
- `src/decrypt/firefox_nss.rs:55-57` — `NssMasterKey` holds key bytes in
  `Zeroizing<Vec<u8>>` and overrides `Debug` to never print contents
  (`:59-62`).
- The `Debug` impls for `PasswordEntry` (`lib.rs:140-146`) use
  `#[serde(skip_serializing)]` on the password field; do not relax this.

**Failure mode.** Plaintext master passwords / AES keys lingering on the heap;
visible in core dumps, `/proc/self/mem`, or post-mortem disk images;
catastrophic if the device is later compromised.

---

## Removed

### INV-2 (old) — "Importer parsers run on `ImporterThread`; bridge/orchestrator dispatch posts to UI"

**Removed.** The C++ `ImporterThread` and its DCHECK-based sequence enforcement
are gone. The Rust orchestrator owns its own scheduling: parsers and workers
run on whichever thread the C++ caller picks (today, a `base::ThreadPool` task
posted by `MahoWelcomePageHandler` / `MahoMigrationDialogView`). The Rust side
is thread-agnostic and has no UI-thread concept.

The relevant safety property — "do not touch browser state off the UI
sequence" — survives as **INV-5**, but its enforcement point has moved from
"inside every parser" to a single chokepoint: the `PostTask` call in
`maho_import_session_bridge.cc:74-78`.

### INV-7 / INV-8 (old) — "ImportOrchestrator is the only caller of
`OnImportProgress(complete=true)`" / "FFI calls to `maho_core_handle_event`
MUST be on UI thread"

**Subsumed by INV-5.** INV-7 is now a type-system guarantee: only the
orchestrator constructs `ImportProgress::AllComplete`
(`src/orchestrator.rs:79-80`), and workers receive a `Sender<ImportProgress>`
with no API surface to send completion. INV-8 collapses into the generic
"PostTask before touching browser state" rule of INV-5.

### INV-9 (old) — "Plaintext key material zeroed via RAII SecureBuffer or
`memset_s`"

**Renamed to INV-6 and re-implemented in Rust.** The mechanism changed
(`zeroize` crate replaces hand-rolled C++ RAII), the contract is unchanged.

---

## See also

- `src/orchestrator.rs` — top-level coordinator; sole owner of `AllComplete`.
- `src/workers/mod.rs` — worker trait + mock destination for tests.
- `src/parsers/{arc,chromium,firefox,safari,zen}/` — source-specific parsers.
- `src/decrypt/{chromium_keychain,firefox_nss,asn1}.rs` — Phase 3 decrypt paths.
- `maho/crates/maho-ffi/src/import_destination.rs` — `MahoCoreDestination`, the
  production `ImportDestination` impl that calls into `maho-core`.
- `maho-chromium/browser/importer/maho_import_session_bridge.{h,cc}` — the
  remaining C++ shim; the only place that needs to know about UI threads.
