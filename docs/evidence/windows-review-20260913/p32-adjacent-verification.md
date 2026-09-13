# P32 adjacent verification - five tests executed, one existing failure

2026-09-13; child task st_01a099cd; parent session 01a0983f-c995-753d-afa9-593f6d118788.

## Result

**Full target completed: exit 101; 4 passed, 1 failed, 0 ignored, 0 measured, 0 filtered out. All five existing tests executed.** This supersedes the initial monitor-unavailable blocker receipt. Lead reports tool.monitor run bash54; this child independently read the complete cargo.log, started.json and done.json returned by that run. No rerun or historical RED was performed.

| Existing test | Result |
| --- | --- |
| tests::older_message_search_and_exact_identity | PASS |
| tests::selected_session_exact_typed_resume_and_duplicate_fence | PASS |
| hardening_tests::claude_active_branch_excludes_abandoned_sibling | PASS |
| hardening_tests::paging_append_corrupt_partial_deleted_and_unsupported | PASS |
| hardening_tests::symlink_roots_and_files_are_rejected | FAIL |

The repaired Claude ancestry assertion passes, as do Codex older-message discovery, exact typed resume/duplicate ownership fencing, paging, append source-change detection, corruption/partial-record warnings, deletion and unsupported-provider handling. The unchanged Unix-only test fails at hardening_tests.rs:52:5: configured symlink root must not silently redirect. It expects ROOT_SYMLINK_REJECTED; the inspected search code canonicalizes configured roots without emitting that warning. This existing out-of-scope failure was neither repaired nor weakened. No full-target GREEN or native Windows acceptance is claimed.

Cargo compilation completed in 2m 06s. The existing library emitted 16 warnings, preserved in full below. The executed target was /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/scoped_history-cb3d1908a36a8d9c. Runner PID 57983 awaited child PID 57984; done.json records exit 101 at 2026-09-13T08:13:30.917Z, sourcesUnchanged true and tmpRemaining [].

## Command and isolation

Lead monitor entry command:

```sh
/Users/indo/.bun/bin/bun /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/run.mjs
```

Exact Cargo command, cwd /Users/indo/code/project/orca-lite:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test scoped_history -- --nocapture
```

The child prepared one APFS clone with /bin/cp -cRp src-tauri/target/debug /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug (exit 0). No additional target clone or concurrent build was launched by this child. Despite cloned artifacts, Cargo rebuilt dependencies in this environment, as the full log shows; no cache-hit claim is made. The target is retained for lead-coordinated serial P03/P11 reuse.

The explicit environment pins the installed direct 1.98.1-aarch64-apple-darwin toolchain in PATH, RUSTC and RUSTDOC, bypassing the default toolchain. HOME, USERPROFILE, APPDATA, LOCALAPPDATA, temp, runtime, data, sessions, XDG paths, CARGO_HOME and CARGO_TARGET_DIR are owned. The registry symlink borrows /Users/indo/.cargo/registry read-only under macOS sandbox protection. The sandbox denies network and writes beneath the checkout, /Users/indo/.cargo and /Users/indo/.rustup. Cargo is offline, uses eight jobs and no ambient sccache wrapper. No Python wrapper was used. The Bun runner awaits the exact process-exit promise, with no sleeps/polling; deadline supervision belongs to the lead monitor.

Read the prior p32-history-ancestry.md isolation evidence, full target/parser/both test files, root/backend AGENTS, Cargo.toml, build.rs and native_terminal/build_ghostty.rs. Fixtures use tempfile-owned JSONL; resume uses an in-memory boundary and argv resolver, not provider spawning. Build safety dependencies compile Ghostty and perform read-only Git revision checks, directing outputs/caches under OUT_DIR. An initial incorrect build_ghostty.rs inspection path returned ENOENT and was corrected before execution; it was not a build/test failure.

## Pre-build diagnostics

Fresh diagnostics before handoff:

```text
lsp_diagnostics(src-tauri/tests/scoped_history.rs, all)
No diagnostics found

lsp_diagnostics(src-tauri/src/ferryx_scope/history, all)
Directory: /Users/indo/code/project/orca-lite/src-tauri/src/ferryx_scope/history
Extension: .rs
Files scanned: 3
Files with errors: 0
Total diagnostics: 0
```

## SHA256 current-source bindings

The runner hashed these files before and after execution; both maps are identical. The child rehashed each current file when generating this report and confirmed equality with done.json. These bindings cover the direct history inputs and listed library/build contracts, not an assertion that every repository file was frozen against other sessions.

```text
2e7a06206e5ab4a6070bc5494f09cf7eb63622586133abca66542274686ae974  src-tauri/tests/scoped_history.rs
23e5225b940e4597361d1c4a5edef826b01b5543fa756ef07cd801b0ee57cfc3  src-tauri/src/ferryx_scope/history/mod.rs
8d19889a88822b4a207be1cab8d4b651d667af88ed4da285cf94c79b70caa9f9  src-tauri/src/ferryx_scope/history/tests.rs
4682a962bac2d41bb4bea0f3e391733ff26252a761c2134b1e36008a950d3708  src-tauri/src/ferryx_scope/history/hardening_tests.rs
e2e8004912098458c2a045539f233471b3514d0fba66ba1cf261ff829845cbc9  src-tauri/Cargo.toml
1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5  src-tauri/Cargo.lock
c0f32563f319186bced8cf47d167a3e08d1af5424b729f3545b596893c9dc636  src-tauri/build.rs
d39a6a596f3f2333010281bbb299b24fe6f2a469ab290cebd9ef718aac78a265  src-tauri/native_terminal/build_ghostty.rs
ad7aea08691635fa25cb8d0681e23773bfef9fc1a7df16b1616b2e47736446d9  src-tauri/src/scoped_contracts.rs
10111cd8a781e75bac0d0ea646eb423f51695cc267868b9f681e21a6db7667c5  src-tauri/src/terminal/shell.rs
1ba548a842a101dda579bf74a7ce6f4ffa11c449e86f49508faf1e15b2199337  src-tauri/src/daemon/protocol.rs
```

## Full start receipt

```json
{
  "root": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW",
  "command": [
    "cargo",
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--test",
    "scoped_history",
    "--",
    "--nocapture"
  ],
  "env": {
    "PATH": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    "HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/home",
    "USERPROFILE": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/home",
    "APPDATA": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/appdata",
    "LOCALAPPDATA": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/localappdata",
    "TMPDIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/tmp",
    "TMP": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/tmp",
    "TEMP": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/tmp",
    "FERRYX_RUNTIME_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/runtime",
    "FERRYX_DATA_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/data",
    "FERRYX_SESSION_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/sessions",
    "XDG_CONFIG_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/home/config",
    "XDG_CACHE_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/home/cache",
    "XDG_DATA_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/data",
    "XDG_RUNTIME_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/runtime",
    "CARGO_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo",
    "CARGO_TARGET_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_BUILD_JOBS": "8",
    "CARGO_TERM_COLOR": "never",
    "RUSTC_WRAPPER": "",
    "RUSTC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc",
    "RUSTDOC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc"
  },
  "policy": "(version 1)(allow default)(deny network*)(deny file-write* (subpath \"/Users/indo/code/project/orca-lite\") (subpath \"/Users/indo/.cargo\") (subpath \"/Users/indo/.rustup\"))",
  "before": {
    "src-tauri/tests/scoped_history.rs": "2e7a06206e5ab4a6070bc5494f09cf7eb63622586133abca66542274686ae974",
    "src-tauri/src/ferryx_scope/history/mod.rs": "23e5225b940e4597361d1c4a5edef826b01b5543fa756ef07cd801b0ee57cfc3",
    "src-tauri/src/ferryx_scope/history/tests.rs": "8d19889a88822b4a207be1cab8d4b651d667af88ed4da285cf94c79b70caa9f9",
    "src-tauri/src/ferryx_scope/history/hardening_tests.rs": "4682a962bac2d41bb4bea0f3e391733ff26252a761c2134b1e36008a950d3708",
    "src-tauri/Cargo.toml": "e2e8004912098458c2a045539f233471b3514d0fba66ba1cf261ff829845cbc9",
    "src-tauri/Cargo.lock": "1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5",
    "src-tauri/build.rs": "c0f32563f319186bced8cf47d167a3e08d1af5424b729f3545b596893c9dc636",
    "src-tauri/native_terminal/build_ghostty.rs": "d39a6a596f3f2333010281bbb299b24fe6f2a469ab290cebd9ef718aac78a265",
    "src-tauri/src/scoped_contracts.rs": "ad7aea08691635fa25cb8d0681e23773bfef9fc1a7df16b1616b2e47736446d9",
    "src-tauri/src/terminal/shell.rs": "10111cd8a781e75bac0d0ea646eb423f51695cc267868b9f681e21a6db7667c5",
    "src-tauri/src/daemon/protocol.rs": "1ba548a842a101dda579bf74a7ce6f4ffa11c449e86f49508faf1e15b2199337"
  },
  "started": "2026-09-13T08:11:23.028Z",
  "runnerPid": 57983
}
```

## Full Cargo output

The following block preserves the complete cargo.log verbatim, including dependency compilation, warnings and the unchanged assertion failure. cargo.log SHA256: 16b479bda678029f842d70f312d73fd28a2c9b7dbc8d47c951fec711bf83eafb.

```text
   Compiling proc-macro2 v1.0.107
   Compiling unicode-ident v1.0.24
   Compiling quote v1.0.47
   Compiling cfg-if v1.0.4
   Compiling libc v0.2.189
   Compiling serde_core v1.0.229
   Compiling find-msvc-tools v0.1.11
   Compiling shlex v2.0.1
   Compiling cc v1.4.3
   Compiling itoa v1.0.18
   Compiling parking_lot_core v0.9.12
   Compiling memchr v2.8.3
   Compiling log v0.4.33
   Compiling scopeguard v1.2.0
   Compiling lock_api v0.4.14
   Compiling writeable v0.6.4
   Compiling litemap v0.8.3
   Compiling bytes v1.12.1
   Compiling utf8_iter v1.0.4
   Compiling siphasher v1.0.3
   Compiling icu_properties_data v2.3.0
   Compiling icu_normalizer_data v2.3.0
   Compiling thiserror v2.0.20
   Compiling phf_shared v0.13.1
   Compiling autocfg v1.5.1
   Compiling objc2-encode v4.1.0
   Compiling zmij v1.0.23
   Compiling fastrand v2.5.0
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling synstructure v0.13.2
   Compiling bitflags v2.13.1
   Compiling http v1.5.0
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling displaydoc v0.2.7
   Compiling thiserror-impl v2.0.20
   Compiling getrandom v0.4.3
   Compiling zerofrom v0.1.8
   Compiling objc2 v0.6.4
   Compiling smallvec v1.15.2
   Compiling serde v1.0.229
   Compiling equivalent v1.0.2
   Compiling num-conv v0.2.2
   Compiling time-core v0.1.9
   Compiling phf_macros v0.13.1
   Compiling winnow v1.0.4
   Compiling once_cell v1.21.4
   Compiling powerfmt v0.2.0
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling base64 v0.22.1
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling phf_codegen v0.13.1
   Compiling thiserror v1.0.69
   Compiling strsim v0.11.1
   Compiling typeid v1.0.3
   Compiling serde_json v1.0.151
   Compiling ident_case v1.0.1
   Compiling darling_core v0.23.0
   Compiling yoke v0.8.3
   Compiling thiserror-impl v1.0.69
   Compiling semver v1.0.28
   Compiling erased-serde v0.4.10
   Compiling byteorder v1.5.0
   Compiling darling_macro v0.23.0
   Compiling aho-corasick v1.1.5
   Compiling unic-char-range v0.9.0
   Compiling block2 v0.6.2
   Compiling fnv v1.0.7
   Compiling dispatch2 v0.3.1
   Compiling new_debug_unreachable v1.0.6
   Compiling unic-common v0.9.0
   Compiling regex-syntax v0.8.11
   Compiling objc2-core-foundation v0.3.2
   Compiling unic-ucd-version v0.9.0
   Compiling unic-char-property v0.9.0
   Compiling darling v0.23.0
   Compiling phf v0.13.1
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling objc2-foundation v0.3.2
   Compiling regex-automata v0.4.18
   Compiling string_cache_codegen v0.6.1
   Compiling anyhow v1.0.104
   Compiling alloc-no-stdlib v2.0.4
   Compiling precomputed-hash v0.1.1
   Compiling alloc-stdlib v0.2.4
   Compiling serde_with_macros v3.22.0
   Compiling regex v1.13.1
   Compiling web_atoms v0.2.6
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling same-file v1.0.6
   Compiling tinystr v0.8.4
   Compiling potential_utf v0.1.6
   Compiling walkdir v2.5.0
   Compiling icu_collections v2.3.0
   Compiling string_cache v0.9.0
   Compiling icu_locale_core v2.3.0
   Compiling brotli-decompressor v5.0.3
   Compiling dtoa v1.0.11
   Compiling dunce v1.0.5
   Compiling ctor-proc-macro v0.0.7
   Compiling percent-encoding v2.3.2
   Compiling ctor v0.8.0
   Compiling icu_provider v2.3.0
   Compiling icu_normalizer v2.3.0
   Compiling icu_properties v2.3.0
   Compiling form_urlencoded v1.2.2
   Compiling brotli v8.0.4
   Compiling dtoa-short v0.3.5
   Compiling uuid v1.26.0
   Compiling tendril v0.5.1
   Compiling idna_adapter v1.2.2
   Compiling selectors v0.36.1
   Compiling idna v1.1.0
   Compiling cssparser-macros v0.6.1
   Compiling derive_more-impl v2.1.1
   Compiling indexmap v1.9.3
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling glob v0.3.4
   Compiling camino v1.2.5
   Compiling version_check v0.9.5
   Compiling derive_more v2.1.1
   Compiling toml v1.1.4+spec-1.1.0
   Compiling markup5ever v0.38.0
   Compiling cssparser v0.36.0
   Compiling url v2.5.8
   Compiling swift-rs v1.0.8
   Compiling bytemuck_derive v1.12.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling schemars v0.8.22
   Compiling rustc-hash v2.1.3
   Compiling deranged v0.5.8
   Compiling hashbrown v0.17.1
   Compiling bit-vec v0.8.0
   Compiling hashbrown v0.12.3
   Compiling bit-set v0.8.0
   Compiling bytemuck v1.25.2
   Compiling schemars_derive v0.8.22
   Compiling indexmap v2.14.0
   Compiling time v0.3.55
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling base64 v0.21.7
   Compiling bitflags v1.3.2
   Compiling foldhash v0.2.0
   Compiling dyn-clone v1.0.20
   Compiling pin-project-lite v0.2.17
   Compiling serde-untagged v0.1.9
   Compiling cargo_metadata v0.19.2
   Compiling json-patch v3.0.1
   Compiling dom_query v0.27.0
   Compiling plist v1.10.0
   Compiling infer v0.19.0
   Compiling urlpattern v0.3.0
   Compiling serde_with v3.22.0
   Compiling errno v0.3.14
   Compiling rustc_version v0.4.1
   Compiling option-ext v0.2.0
   Compiling libm v0.2.16
   Compiling generic-array v0.14.7
   Compiling num-traits v0.2.19
   Compiling signal-hook-registry v1.4.8
   Compiling tokio-macros v2.7.2
   Compiling mio v1.2.2
   Compiling socket2 v0.6.5
   Compiling typenum v1.20.1
   Compiling tokio v1.53.1
   Compiling winnow v0.7.15
   Compiling arrayvec v0.7.8
   Compiling raw-window-handle v0.6.2
   Compiling core-foundation-sys v0.8.7
   Compiling tauri-utils v2.9.3
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling futures-core v0.3.34
   Compiling zerocopy v0.8.56
   Compiling toml v0.9.12+spec-1.1.0
   Compiling block-buffer v0.10.4
   Compiling crypto-common v0.1.7
   Compiling dirs-sys v0.5.0
   Compiling embed-resource v3.0.11
   Compiling zerocopy-derive v0.8.56
   Compiling heck v0.5.0
   Compiling cfg_aliases v0.2.2
   Compiling tauri-winres v0.3.6
   Compiling dirs v6.0.0
   Compiling cargo_toml v0.22.3
   Compiling digest v0.10.7
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling zeroize v1.9.0
   Compiling simd-adler32 v0.3.10
   Compiling crc32fast v1.5.0
   Compiling time-macros v0.2.32
   Compiling lazy_static v1.5.0
   Compiling adler2 v2.0.1
   Compiling miniz_oxide v0.8.9
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling fdeflate v0.3.7
   Compiling rustls-pki-types v1.15.1
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling crossbeam-utils v0.8.22
   Compiling untrusted v0.9.0
   Compiling subtle v2.6.1
   Compiling foreign-types-shared v0.3.1
   Compiling foreign-types v0.5.0
   Compiling tracing v0.1.44
   Compiling cookie v0.18.2
   Compiling http-body v1.1.0
   Compiling rustls v0.23.43
   Compiling futures-sink v0.3.34
   Compiling httparse v1.10.1
   Compiling rustls-webpki v0.103.15
   Compiling futures-macro v0.3.34
   Compiling encoding_rs v0.8.35
   Compiling tower-service v0.3.3
   Compiling futures-task v0.3.34
   Compiling tauri v2.11.5
   Compiling slab v0.4.12
   Compiling futures-util v0.3.34
   Compiling symphonia-metadata v0.5.5
   Compiling crossbeam-channel v0.5.16
   Compiling objc2-web-kit v0.3.2
   Compiling core-graphics-types v0.2.0
   Compiling png v0.17.16
   Compiling cpufeatures v0.2.17
   Compiling mime v0.3.17
   Compiling wry v0.55.1
   Compiling tauri-runtime v2.11.3
   Compiling ico v0.5.0
   Compiling sha2 v0.10.9
   Compiling core-graphics v0.25.0
   Compiling png v0.18.1
   Compiling ppv-lite86 v0.2.21
   Compiling pxfm v0.1.30
   Compiling tauri-runtime-wry v2.11.4
   Compiling try-lock v0.2.5
   Compiling tower-layer v0.3.3
   Compiling unicode-segmentation v1.13.3
   Compiling rustc-hash v1.1.0
   Compiling naga-types v30.0.1
   Compiling want v0.3.1
   Compiling keyboard-types v0.7.0
   Compiling tao v0.35.3
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling futures-channel v0.3.34
   Compiling sync_wrapper v1.0.2
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling moxcms v0.8.1
   Compiling unicode-width v0.1.14
   Compiling httpdate v1.0.3
   Compiling bit-vec v0.9.1
   Compiling atomic-waker v1.1.2
   Compiling byteorder-lite v0.1.0
   Compiling objc-sys v0.3.5
   Compiling bit-set v0.10.0
   Compiling hyper v1.11.0
   Compiling tauri-macros v2.6.3
   Compiling codespan-reporting v0.13.1
   Compiling serialize-to-javascript v0.1.2
   Compiling muda v0.19.3
   Compiling tokio-rustls v0.26.4
   Compiling symphonia-utils-xiph v0.5.5
   Compiling window-vibrancy v0.6.0
   Compiling rand_core v0.9.5
   Compiling half v2.7.1
   Compiling objc2-metal v0.3.2
   Compiling objc2-core-graphics v0.3.2
   Compiling serde_repr v0.1.21
   Compiling image v0.25.10
   Compiling rustix v1.1.4
   Compiling embed_plist v1.2.2
   Compiling ipnet v2.12.1
   Compiling static_assertions v1.1.0
   Compiling wgpu-types v30.0.1
   Compiling hyper-util v0.1.20
   Compiling rand_chacha v0.9.0
   Compiling objc2-quartz-core v0.3.2
   Compiling tower v0.5.3
   Compiling tauri-plugin-fs v2.5.1
   Compiling http-body-util v0.1.5
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling objc2 v0.5.2
   Compiling objc2-core-audio v0.3.2
   Compiling raw-window-metal v1.1.0
   Compiling rand v0.9.5
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling cfg_aliases v0.1.1
   Compiling profiling v1.0.18
   Compiling nix v0.28.0
   Compiling security-framework v3.7.0
   Compiling block2 v0.5.1
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tauri-plugin-process v2.3.1
   Compiling tauri-plugin-updater v2.10.1
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tauri-plugin-notification v2.3.3
   Compiling tokio-util v0.7.19
   Compiling rand_core v0.6.4
   Compiling wgpu-naga-bridge v30.0.1
   Compiling wgpu-core v30.0.1
   Compiling num-integer v0.1.47
   Compiling core-foundation v0.9.4
   Compiling curve25519-dalek v4.1.3
   Compiling litrs v1.0.0
   Compiling cpal v0.17.3
   Compiling rfd v0.16.0
   Compiling dispatch v0.2.0
   Compiling ryu v1.0.23
   Compiling unicase v2.9.0
   Compiling data-encoding v2.11.1
   Compiling extended v0.1.0
   Compiling document-features v0.2.12
   Compiling mime_guess v2.0.5
   Compiling symphonia-format-riff v0.5.5
   Compiling tungstenite v0.29.0
   Compiling serde_urlencoded v0.7.1
   Compiling objc2-foundation v0.2.2
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling core-graphics-types v0.1.3
   Compiling num-bigint v0.4.8
   Compiling coreaudio-rs v0.14.2
   Compiling rustls-platform-verifier v0.7.0
   Compiling xattr v1.6.1
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling wgpu v30.0.1
   Compiling winit v0.30.13
   Compiling mach2 v0.5.0
   Compiling filetime v0.2.29
   Compiling signature v2.2.0
   Compiling dasp_sample v0.11.0
   Compiling tar v0.4.46
   Compiling ed25519 v2.2.3
   Compiling tempfile v3.27.0
   Compiling symphonia v0.5.5
   Compiling osakit v0.3.1
   Compiling tokio-tungstenite v0.29.0
   Compiling objc2-app-kit v0.2.2
   Compiling reqwest v0.13.4
   Compiling num-rational v0.4.2
   Compiling core-graphics v0.23.2
   Compiling notify-rust v4.18.0
   Compiling rand_chacha v0.3.1
   Compiling axum-core v0.5.6
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite/src-tauri)
   Compiling tracing-log v0.2.0
   Compiling sharded-slab v0.1.7
   Compiling filedescriptor v0.8.3
   Compiling notify-types v2.1.0
   Compiling serde_path_to_error v0.1.20
   Compiling serial2 v0.2.38
   Compiling fsevent-sys v4.1.0
   Compiling thread_local v1.1.10
   Compiling shell-words v1.1.1
   Compiling downcast-rs v1.2.1
   Compiling nu-ansi-term v0.50.3
   Compiling smol_str v0.2.2
   Compiling cursor-icon v1.2.0
   Compiling minisign-verify v0.2.5
   Compiling matchit v0.8.4
   Compiling tracing-subscriber v0.3.23
   Compiling axum v0.8.9
   Compiling portable-pty v0.9.0
   Compiling notify v8.2.0
   Compiling ed25519-dalek v2.2.0
   Compiling rand v0.8.7
   Compiling rodio v0.22.2
   Compiling reqwest v0.12.28
   Compiling tower-http v0.7.1
   Compiling objc2-user-notifications v0.3.2
   Compiling objc2-core-text v0.3.2
   Compiling pollster v1.0.1
   Compiling base64 v0.23.1
   Compiling tokio-stream v0.1.19
   Compiling tokio-test v0.4.5
warning: unused import: `Manager`
  --> src/ipc/notifications.rs:21:24
   |
21 | use tauri::{AppHandle, Manager, Runtime, State};
   |                        ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: method `register_pairing_capability` is never used
   --> src/remote/auth.rs:396:19
    |
317 | impl AuthManager {
    | ---------------- method in this implementation
...
396 |     pub(crate) fn register_pairing_capability(&self, token: &str) {
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:270:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
270 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib) generated 16 warnings (run `cargo fix --lib -p ferryx` to apply 2 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2m 06s
     Running tests/scoped_history.rs (/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/scoped_history-cb3d1908a36a8d9c)

running 5 tests

thread 'hardening_tests::symlink_roots_and_files_are_rejected' (3215019) panicked at tests/../src/ferryx_scope/history/hardening_tests.rs:52:5:
configured symlink root must not silently redirect
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test tests::older_message_search_and_exact_identity ... ok
test hardening_tests::claude_active_branch_excludes_abandoned_sibling ... ok
test tests::selected_session_exact_typed_resume_and_duplicate_fence ... ok
test hardening_tests::symlink_roots_and_files_are_rejected ... FAILED
test hardening_tests::paging_append_corrupt_partial_deleted_and_unsupported ... ok

failures:

failures:
    hardening_tests::symlink_roots_and_files_are_rejected

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test scoped_history`
```

## Full completion receipt

```json
{
  "pid": 57984,
  "runnerPid": 57983,
  "exit": 101,
  "finished": "2026-09-13T08:13:30.917Z",
  "after": {
    "src-tauri/tests/scoped_history.rs": "2e7a06206e5ab4a6070bc5494f09cf7eb63622586133abca66542274686ae974",
    "src-tauri/src/ferryx_scope/history/mod.rs": "23e5225b940e4597361d1c4a5edef826b01b5543fa756ef07cd801b0ee57cfc3",
    "src-tauri/src/ferryx_scope/history/tests.rs": "8d19889a88822b4a207be1cab8d4b651d667af88ed4da285cf94c79b70caa9f9",
    "src-tauri/src/ferryx_scope/history/hardening_tests.rs": "4682a962bac2d41bb4bea0f3e391733ff26252a761c2134b1e36008a950d3708",
    "src-tauri/Cargo.toml": "e2e8004912098458c2a045539f233471b3514d0fba66ba1cf261ff829845cbc9",
    "src-tauri/Cargo.lock": "1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5",
    "src-tauri/build.rs": "c0f32563f319186bced8cf47d167a3e08d1af5424b729f3545b596893c9dc636",
    "src-tauri/native_terminal/build_ghostty.rs": "d39a6a596f3f2333010281bbb299b24fe6f2a469ab290cebd9ef718aac78a265",
    "src-tauri/src/scoped_contracts.rs": "ad7aea08691635fa25cb8d0681e23773bfef9fc1a7df16b1616b2e47736446d9",
    "src-tauri/src/terminal/shell.rs": "10111cd8a781e75bac0d0ea646eb423f51695cc267868b9f681e21a6db7667c5",
    "src-tauri/src/daemon/protocol.rs": "1ba548a842a101dda579bf74a7ce6f4ffa11c449e86f49508faf1e15b2199337"
  },
  "sourcesUnchanged": true,
  "tmpRemaining": []
}
```

## Exact Bun runner

```javascript
import { readFileSync, writeFileSync, openSync, closeSync, readdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
const root = '/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW';
const repo = '/Users/indo/code/project/orca-lite';
const toolchain = '/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin';
const files = ['src-tauri/tests/scoped_history.rs','src-tauri/src/ferryx_scope/history/mod.rs','src-tauri/src/ferryx_scope/history/tests.rs','src-tauri/src/ferryx_scope/history/hardening_tests.rs','src-tauri/Cargo.toml','src-tauri/Cargo.lock','src-tauri/build.rs','src-tauri/native_terminal/build_ghostty.rs','src-tauri/src/scoped_contracts.rs','src-tauri/src/terminal/shell.rs','src-tauri/src/daemon/protocol.rs'];
const hashes = () => Object.fromEntries(files.map(f => [f,createHash('sha256').update(readFileSync(`${repo}/${f}`)).digest('hex')]));
const env = {PATH:`${toolchain}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`,HOME:`${root}/home`,USERPROFILE:`${root}/home`,APPDATA:`${root}/appdata`,LOCALAPPDATA:`${root}/localappdata`,TMPDIR:`${root}/tmp`,TMP:`${root}/tmp`,TEMP:`${root}/tmp`,FERRYX_RUNTIME_DIR:`${root}/runtime`,FERRYX_DATA_DIR:`${root}/data`,FERRYX_SESSION_DIR:`${root}/sessions`,XDG_CONFIG_HOME:`${root}/home/config`,XDG_CACHE_HOME:`${root}/home/cache`,XDG_DATA_HOME:`${root}/data`,XDG_RUNTIME_DIR:`${root}/runtime`,CARGO_HOME:`${root}/cargo`,CARGO_TARGET_DIR:`${root}/target`,CARGO_NET_OFFLINE:'true',CARGO_BUILD_JOBS:'8',CARGO_TERM_COLOR:'never',RUSTC_WRAPPER:'',RUSTC:`${toolchain}/rustc`,RUSTDOC:`${toolchain}/rustdoc`};
const policy = `(version 1)(allow default)(deny network*)(deny file-write* (subpath "${repo}") (subpath "/Users/indo/.cargo") (subpath "/Users/indo/.rustup"))`;
const command = ['cargo','test','--manifest-path','src-tauri/Cargo.toml','--test','scoped_history','--','--nocapture'];
const before = hashes();
const fd = openSync(`${root}/cargo.log`,'wx');
const started = {root,command,env,policy,before,started:new Date().toISOString(),runnerPid:process.pid};
writeFileSync(`${root}/started.json`,JSON.stringify(started,null,2));
const child = Bun.spawn(['/usr/bin/sandbox-exec','-p',policy,...command],{cwd:repo,env,stdout:fd,stderr:fd});
console.log(JSON.stringify({event:'started',pid:child.pid,root,command}));
const exit = await child.exited;
closeSync(fd);
const after = hashes();
const result = {pid:child.pid,runnerPid:process.pid,exit,finished:new Date().toISOString(),after,sourcesUnchanged:JSON.stringify(before)===JSON.stringify(after),tmpRemaining:readdirSync(`${root}/tmp`)};
writeFileSync(`${root}/done.json`,JSON.stringify(result,null,2));
process.stdout.write(readFileSync(`${root}/cargo.log`));
console.log(JSON.stringify(result));
process.exitCode = exit;
```

## Cleanup and preservation receipt

**Cleanup deferred to lead: shared root intentionally preserved for active P03/P11 borrowers.** Do not remove /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW or its target while borrowers are active. The completed P32 run recorded an empty fixture TMPDIR (tmpRemaining []); this is fixture cleanup evidence, not a claim that the owned build/profile root has been removed. The clone, registry symlink, profile/runtime directories, runner and raw receipts remain owned resources for lead cleanup after all borrowers complete. No cleanup command was issued in this reporting turn, and no foreign cache or file was deleted.

Production and tests remain unchanged by this child; only this evidence document and owned temporary preparation resources were written. The shared-tree parser retains its pre-existing 14-line repair. No branch/worktree/ref/commit changes, desktop/daemon/SSH launch, real user history inspection, global configuration edits or historical RED execution occurred. Branch/worktree approval remains unanswered. This evidence is uncommitted and subject to concurrent shared-tree changes.
