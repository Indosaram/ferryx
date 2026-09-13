# P32 / BI-01 scoped Claude active ancestry - 2026-09-13

Local same-assertion RED/GREEN is complete. Lead combined verification after the parallel repair batch and native Windows executable acceptance remain pending. This is test-target prototype qualification, not desktop history or Windows runtime acceptance.

## Scope and mechanism

Task st_01a0997b. Read initial git status/diff, root/backend AGENTS (no deeper history instructions found), bounded-infrastructure BI-01, gap-packet-addendum P32 and the parent C002 exact-command registration. Foreign dirty files were left untouched. Only history/mod.rs is modified; hardening_tests.rs remains byte-identical because its existing assertion is sufficient for this registered regression. No renamed test, weakened oracle, fabricated provider identity or new fixture contract.

Read the entire scoped_history integration target and both included test files, full history parser and scoped contracts, session-ID validation/resume argv resolution, Cargo manifest and full build.rs/native_terminal/build_ghostty.rs. The affected path is tests/scoped_history.rs -> included history module -> History::search/read -> parse. Both consumers previously received every text record, including abandoned sibling b. The fixture supplies actual JSONL uuid/parentUuid links a <- b and a <- c, with sessionId 63ba88ec-b3cb-4a70-8a32-f5d1ad07e749. Its latest message c selects a,c.

The 14-line parser addition retains Claude record ancestry independently of extracted text, then walks earlier records backward from the latest message, retaining only active source ordinals. Thus tool-only/textless user/assistant records can connect ancestors without becoming displayed messages. No IDs are invented or normalized. The backward traversal is bounded and cannot loop on malformed cyclic links. Null/missing parents terminate ancestry; unmatched parents cannot pull unrelated siblings into the result. This follows the fixture's append-ordered ancestry contract, not a claim of complete support for all provider history formats. Codex, identity validation, warning collection, paging and source-change checks are unchanged. Source ordinals are retained, never reassigned.

## Execution safety and prerequisites

No branch/worktree/ref/commit/push changes, desktop/daemon/SSH/provider launch, ambient transcript inspection, IPC exposure or Unix symlink-root repair. Entire target inspection established tempfile-owned synchronous file IO; the resume test uses only an in-memory NativeBoundary and argv resolver. Only the exact registered ancestry test executed. The separate paging/corruption and Unix symlink tests were preserved, not executed; no full-target GREEN is claimed.

Local pure-fixture execution used owned /private/tmp/p32-history.eLkoeO on the current checkout, not an authorized isolated Windows runtime checkout. HOME, USERPROFILE, APPDATA, LOCALAPPDATA, TMP/TEMP/TMPDIR, FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR and XDG directories pointed under that owned root. Cargo environment was constructed explicitly rather than inherited. Cargo target artifacts were APFS-cloned into the owned root; the repository target was not used as a writable output directory. CARGO_HOME was owned, with registry linked read-only to the existing package cache. macOS sandbox denied network and all writes beneath the repository, ~/.cargo and ~/.rustup. CARGO_NET_OFFLINE=true, CARGO_BUILD_JOBS=8, RUSTC_WRAPPER empty avoided the ambient sccache configuration. No global environment/config was changed.

Compiler: rustc 1.98.1 (48a229cea 2026-09-01), direct installed 1.98.1-aarch64-apple-darwin toolchain binaries. Zig /opt/homebrew/bin/zig reports 0.16.0; vendored Ghostty HEAD matches required 6a508fd5e34c7e222c052a6d00bb3891ff3feace. Build script runs compiler and read-only git revision checks, directs Ghostty outputs/caches under OUT_DIR and does not launch desktop. Sandbox protection remained enabled throughout builds/tests. The selected binary was /private/tmp/p32-history.eLkoeO/target/debug/deps/scoped_history-cb3d1908a36a8d9c, verified Mach-O arm64; GREEN SHA256 77bd2ee9706a719dde1d5077536e9e82bf40a9bab38e1556c0dce7dac972a05a.

An asyncio subprocess monitor awaited child exit with a bounded 600-second deadline. A kqueue process-exit signal awaited each monitor, without sleeps/polling. Neither deadline fired. RED child PID 50032 exited 101; GREEN child PID 53060 exited 0. Both recorded an empty fixture TMPDIR after process exit. Monitor PIDs 50025/53059 were observed exited; no owned persistent service was started.

## Verification

Identical command in both runs, unchanged original test source:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test scoped_history hardening_tests::claude_active_branch_excludes_abandoned_sibling -- --exact --nocapture
```

RED: original source, one executed test, assertion at hardening_tests.rs:12:5 fails because abandoned search returns 1 instead of 0. GREEN: repaired source, one passed, zero failed, four filtered out. The unchanged later assertion requires exactly a,c from History::read. Both compile and execute the actual included parser; no mock/parser extraction or stale binary substitution.

LSP before build: mod.rs no diagnostics. Initial hardening_tests.rs diagnostic request timed out; a later fresh request returned no diagnostics. After production edit mod.rs again returned no diagnostics. git diff --check passed. Test compilation succeeded twice; 16 unrelated existing library warnings remain visible in raw logs below (unused imports/mut/unsafe/dead code). No separate full desktop build or unrelated test invocation was performed.

SHA256 bindings:
- Original mod.rs: f8fc70d54815e168e4256ec2b2f302d53131b5a526f49fe8a88983047765cf0e (BI-01 receipt; initial scoped diff was empty).
- Repaired mod.rs: 23e5225b940e4597361d1c4a5edef826b01b5543fa756ef07cd801b0ee57cfc3.
- Unchanged hardening_tests.rs: 4682a962bac2d41bb4bea0f3e391733ff26252a761c2134b1e36008a950d3708.

Uncommitted shared-tree work remains vulnerable to concurrent writers. Lead owns aggregate verification and native Windows execution of this real fixture surface; this receipt closes neither parent acceptance nor a shipped history feature.

## Raw RED output

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
   Compiling memchr v2.8.3
   Compiling parking_lot_core v0.9.12
   Compiling log v0.4.33
   Compiling scopeguard v1.2.0
   Compiling lock_api v0.4.14
   Compiling litemap v0.8.3
   Compiling writeable v0.6.4
   Compiling bytes v1.12.1
   Compiling utf8_iter v1.0.4
   Compiling siphasher v1.0.3
   Compiling icu_properties_data v2.3.0
   Compiling icu_normalizer_data v2.3.0
   Compiling thiserror v2.0.20
   Compiling phf_shared v0.13.1
   Compiling objc2-encode v4.1.0
   Compiling autocfg v1.5.1
   Compiling zmij v1.0.23
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling fastrand v2.5.0
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling http v1.5.0
   Compiling bitflags v2.13.1
   Compiling synstructure v0.13.2
   Compiling objc2 v0.6.4
   Compiling getrandom v0.4.3
   Compiling equivalent v1.0.2
   Compiling serde v1.0.229
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling displaydoc v0.2.7
   Compiling thiserror-impl v2.0.20
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling smallvec v1.15.2
   Compiling time-core v0.1.9
   Compiling num-conv v0.2.2
   Compiling phf_macros v0.13.1
   Compiling zerofrom v0.1.8
   Compiling once_cell v1.21.4
   Compiling winnow v1.0.4
   Compiling powerfmt v0.2.0
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling base64 v0.22.1
   Compiling phf_codegen v0.13.1
   Compiling serde_json v1.0.151
   Compiling typeid v1.0.3
   Compiling ident_case v1.0.1
   Compiling strsim v0.11.1
   Compiling thiserror v1.0.69
   Compiling darling_core v0.23.0
   Compiling block2 v0.6.2
   Compiling thiserror-impl v1.0.69
   Compiling dispatch2 v0.3.1
   Compiling objc2-core-foundation v0.3.2
   Compiling yoke v0.8.3
   Compiling semver v1.0.28
   Compiling byteorder v1.5.0
   Compiling erased-serde v0.4.10
   Compiling darling_macro v0.23.0
   Compiling aho-corasick v1.1.5
   Compiling objc2-foundation v0.3.2
   Compiling new_debug_unreachable v1.0.6
   Compiling unic-char-range v0.9.0
   Compiling fnv v1.0.7
   Compiling regex-syntax v0.8.11
   Compiling unic-common v0.9.0
   Compiling unic-ucd-version v0.9.0
   Compiling unic-char-property v0.9.0
   Compiling darling v0.23.0
   Compiling phf v0.13.1
   Compiling regex-automata v0.4.18
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling string_cache_codegen v0.6.1
   Compiling anyhow v1.0.104
   Compiling alloc-no-stdlib v2.0.4
   Compiling precomputed-hash v0.1.1
   Compiling alloc-stdlib v0.2.4
   Compiling serde_with_macros v3.22.0
   Compiling web_atoms v0.2.6
   Compiling regex v1.13.1
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling same-file v1.0.6
   Compiling walkdir v2.5.0
   Compiling string_cache v0.9.0
   Compiling brotli-decompressor v5.0.3
   Compiling percent-encoding v2.3.2
   Compiling ctor-proc-macro v0.0.7
   Compiling dtoa v1.0.11
   Compiling dunce v1.0.5
   Compiling ctor v0.8.0
   Compiling dtoa-short v0.3.5
   Compiling tinystr v0.8.4
   Compiling potential_utf v0.1.6
   Compiling icu_collections v2.3.0
   Compiling icu_locale_core v2.3.0
   Compiling brotli v8.0.4
   Compiling form_urlencoded v1.2.2
   Compiling uuid v1.26.0
   Compiling icu_provider v2.3.0
   Compiling tendril v0.5.1
   Compiling icu_properties v2.3.0
   Compiling icu_normalizer v2.3.0
   Compiling selectors v0.36.1
   Compiling cssparser-macros v0.6.1
   Compiling derive_more-impl v2.1.1
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling indexmap v1.9.3
   Compiling idna_adapter v1.2.2
   Compiling version_check v0.9.5
   Compiling camino v1.2.5
   Compiling idna v1.1.0
   Compiling glob v0.3.4
   Compiling derive_more v2.1.1
   Compiling url v2.5.8
   Compiling toml v1.1.4+spec-1.1.0
   Compiling markup5ever v0.38.0
   Compiling cssparser v0.36.0
   Compiling swift-rs v1.0.8
   Compiling bytemuck_derive v1.12.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling schemars v0.8.22
   Compiling hashbrown v0.17.1
   Compiling deranged v0.5.8
   Compiling hashbrown v0.12.3
   Compiling bit-vec v0.8.0
   Compiling rustc-hash v2.1.3
   Compiling indexmap v2.14.0
   Compiling schemars_derive v0.8.22
   Compiling bit-set v0.8.0
   Compiling time v0.3.55
   Compiling bytemuck v1.25.2
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling base64 v0.21.7
   Compiling foldhash v0.2.0
   Compiling bitflags v1.3.2
   Compiling dyn-clone v1.0.20
   Compiling pin-project-lite v0.2.17
   Compiling dom_query v0.27.0
   Compiling json-patch v3.0.1
   Compiling serde-untagged v0.1.9
   Compiling cargo_metadata v0.19.2
   Compiling infer v0.19.0
   Compiling plist v1.10.0
   Compiling urlpattern v0.3.0
   Compiling serde_with v3.22.0
   Compiling errno v0.3.14
   Compiling rustc_version v0.4.1
   Compiling libm v0.2.16
   Compiling option-ext v0.2.0
   Compiling generic-array v0.14.7
   Compiling num-traits v0.2.19
   Compiling signal-hook-registry v1.4.8
   Compiling tokio-macros v2.7.2
   Compiling mio v1.2.2
   Compiling socket2 v0.6.5
   Compiling typenum v1.20.1
   Compiling tokio v1.53.1
   Compiling raw-window-handle v0.6.2
   Compiling arrayvec v0.7.8
   Compiling winnow v0.7.15
   Compiling core-foundation-sys v0.8.7
   Compiling tauri-utils v2.9.3
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling zerocopy v0.8.56
   Compiling futures-core v0.3.34
   Compiling crypto-common v0.1.7
   Compiling block-buffer v0.10.4
   Compiling toml v0.9.12+spec-1.1.0
   Compiling dirs-sys v0.5.0
   Compiling embed-resource v3.0.11
   Compiling zerocopy-derive v0.8.56
   Compiling cfg_aliases v0.2.2
   Compiling heck v0.5.0
   Compiling tauri-winres v0.3.6
   Compiling cargo_toml v0.22.3
   Compiling dirs v6.0.0
   Compiling digest v0.10.7
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling zeroize v1.9.0
   Compiling crc32fast v1.5.0
   Compiling simd-adler32 v0.3.10
   Compiling time-macros v0.2.32
   Compiling lazy_static v1.5.0
   Compiling adler2 v2.0.1
   Compiling miniz_oxide v0.8.9
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling fdeflate v0.3.7
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling rustls-pki-types v1.15.1
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling foreign-types-shared v0.3.1
   Compiling subtle v2.6.1
   Compiling untrusted v0.9.0
   Compiling crossbeam-utils v0.8.22
   Compiling cookie v0.18.2
   Compiling foreign-types v0.5.0
   Compiling http-body v1.1.0
   Compiling tracing v0.1.44
   Compiling rustls v0.23.43
   Compiling httparse v1.10.1
   Compiling futures-sink v0.3.34
   Compiling rustls-webpki v0.103.15
   Compiling futures-macro v0.3.34
   Compiling encoding_rs v0.8.35
   Compiling tower-service v0.3.3
   Compiling futures-task v0.3.34
   Compiling slab v0.4.12
   Compiling futures-util v0.3.34
   Compiling crossbeam-channel v0.5.16
   Compiling tauri v2.11.5
   Compiling symphonia-metadata v0.5.5
   Compiling objc2-web-kit v0.3.2
   Compiling core-graphics-types v0.2.0
   Compiling png v0.17.16
   Compiling cpufeatures v0.2.17
   Compiling mime v0.3.17
   Compiling wry v0.55.1
   Compiling tauri-runtime v2.11.3
   Compiling sha2 v0.10.9
   Compiling ico v0.5.0
   Compiling core-graphics v0.25.0
   Compiling ppv-lite86 v0.2.21
   Compiling png v0.18.1
   Compiling try-lock v0.2.5
   Compiling tauri-runtime-wry v2.11.4
   Compiling pxfm v0.1.30
   Compiling tower-layer v0.3.3
   Compiling rustc-hash v1.1.0
   Compiling unicode-segmentation v1.13.3
   Compiling naga-types v30.0.1
   Compiling keyboard-types v0.7.0
   Compiling tao v0.35.3
   Compiling want v0.3.1
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling futures-channel v0.3.34
   Compiling sync_wrapper v1.0.2
   Compiling moxcms v0.8.1
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling objc-sys v0.3.5
   Compiling httpdate v1.0.3
   Compiling unicode-width v0.1.14
   Compiling bit-vec v0.9.1
   Compiling byteorder-lite v0.1.0
   Compiling atomic-waker v1.1.2
   Compiling tauri-macros v2.6.3
   Compiling hyper v1.11.0
   Compiling bit-set v0.10.0
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
   Compiling static_assertions v1.1.0
   Compiling ipnet v2.12.1
   Compiling rustix v1.1.4
   Compiling embed_plist v1.2.2
   Compiling wgpu-types v30.0.1
   Compiling hyper-util v0.1.20
   Compiling objc2-quartz-core v0.3.2
   Compiling rand_chacha v0.9.0
   Compiling tower v0.5.3
   Compiling tauri-plugin-fs v2.5.1
   Compiling http-body-util v0.1.5
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling objc2 v0.5.2
   Compiling objc2-core-audio v0.3.2
   Compiling rand v0.9.5
   Compiling raw-window-metal v1.1.0
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling cfg_aliases v0.1.1
   Compiling profiling v1.0.18
   Compiling security-framework v3.7.0
   Compiling nix v0.28.0
   Compiling block2 v0.5.1
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tauri-plugin-process v2.3.1
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tauri-plugin-updater v2.10.1
   Compiling tauri-plugin-notification v2.3.3
   Compiling tokio-util v0.7.19
   Compiling wgpu-naga-bridge v30.0.1
   Compiling rand_core v0.6.4
   Compiling wgpu-core v30.0.1
   Compiling num-integer v0.1.47
   Compiling core-foundation v0.9.4
   Compiling curve25519-dalek v4.1.3
   Compiling data-encoding v2.11.1
   Compiling unicase v2.9.0
   Compiling extended v0.1.0
   Compiling ryu v1.0.23
   Compiling litrs v1.0.0
   Compiling cpal v0.17.3
   Compiling dispatch v0.2.0
   Compiling rfd v0.16.0
   Compiling document-features v0.2.12
   Compiling objc2-foundation v0.2.2
   Compiling serde_urlencoded v0.7.1
   Compiling mime_guess v2.0.5
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling symphonia-format-riff v0.5.5
   Compiling tungstenite v0.29.0
   Compiling core-graphics-types v0.1.3
   Compiling num-bigint v0.4.8
   Compiling rustls-platform-verifier v0.7.0
   Compiling coreaudio-rs v0.14.2
   Compiling xattr v1.6.1
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling winit v0.30.13
   Compiling wgpu v30.0.1
   Compiling mach2 v0.5.0
   Compiling filetime v0.2.29
   Compiling signature v2.2.0
   Compiling dasp_sample v0.11.0
   Compiling ed25519 v2.2.3
   Compiling tar v0.4.46
   Compiling tempfile v3.27.0
   Compiling osakit v0.3.1
   Compiling symphonia v0.5.5
   Compiling tokio-tungstenite v0.29.0
   Compiling reqwest v0.13.4
   Compiling objc2-app-kit v0.2.2
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
   Compiling fsevent-sys v4.1.0
   Compiling serial2 v0.2.38
   Compiling thread_local v1.1.10
   Compiling nu-ansi-term v0.50.3
   Compiling downcast-rs v1.2.1
   Compiling shell-words v1.1.1
   Compiling smol_str v0.2.2
   Compiling matchit v0.8.4
   Compiling minisign-verify v0.2.5
   Compiling cursor-icon v1.2.0
   Compiling tracing-subscriber v0.3.23
   Compiling portable-pty v0.9.0
   Compiling axum v0.8.9
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
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 52s
     Running tests/scoped_history.rs (/private/tmp/p32-history.eLkoeO/target/debug/deps/scoped_history-cb3d1908a36a8d9c)

running 1 test

thread 'hardening_tests::claude_active_branch_excludes_abandoned_sibling' (2580035) panicked at tests/../src/ferryx_scope/history/hardening_tests.rs:12:5:
assertion `left == right` failed: abandoned branch must not be presented as current conversation
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test hardening_tests::claude_active_branch_excludes_abandoned_sibling ... FAILED

failures:

failures:
    hardening_tests::claude_active_branch_excludes_abandoned_sibling

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test scoped_history`
```

Monitor completion: `{"pid": 50032, "exit": 101, "tmp_remaining": []}`

## Raw GREEN output

```text
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite/src-tauri)
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
    Finished `test` profile [unoptimized + debuginfo] target(s) in 13.30s
     Running tests/scoped_history.rs (/private/tmp/p32-history.eLkoeO/target/debug/deps/scoped_history-cb3d1908a36a8d9c)

running 1 test
test hardening_tests::claude_active_branch_excludes_abandoned_sibling ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s

```

Monitor completion: `{"pid": 53060, "exit": 0, "tmp_remaining": []}`

## Exact monitor and isolation wrapper

```python
import asyncio, os, pathlib, json, sys
root = pathlib.Path('/private/tmp/p32-history.eLkoeO')
repo = '/Users/indo/code/project/orca-lite'
label = sys.argv[1]
async def main():
    env = {'PATH':'/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin', 'HOME':str(root/'home'), 'USERPROFILE':str(root/'home'), 'APPDATA':str(root/'appdata'), 'LOCALAPPDATA':str(root/'localappdata'), 'TMPDIR':str(root/'tmp'), 'TMP':str(root/'tmp'), 'TEMP':str(root/'tmp'), 'FERRYX_RUNTIME_DIR':str(root/'runtime'), 'FERRYX_DATA_DIR':str(root/'data'), 'FERRYX_SESSION_DIR':str(root/'sessions'), 'CARGO_HOME':str(root/'cargo'), 'CARGO_TARGET_DIR':str(root/'target'), 'CARGO_NET_OFFLINE':'true', 'CARGO_BUILD_JOBS':'8', 'RUSTC_WRAPPER':'', 'RUSTC':'/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc', 'RUSTDOC':'/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc', 'XDG_CONFIG_HOME':str(root/'home/config'), 'XDG_CACHE_HOME':str(root/'home/cache'), 'XDG_DATA_HOME':str(root/'data'), 'XDG_RUNTIME_DIR':str(root/'runtime')}
    policy = '(version 1)(allow default)(deny network*)(deny file-write* (subpath "/Users/indo/.cargo") (subpath "/Users/indo/.rustup") (subpath "'+repo+'"))'
    cmd = ['cargo','test','--manifest-path','src-tauri/Cargo.toml','--test','scoped_history','hardening_tests::claude_active_branch_excludes_abandoned_sibling','--','--exact','--nocapture']
    with (root/(label+'.log')).open('wb') as out:
        proc = await asyncio.create_subprocess_exec('/usr/bin/sandbox-exec','-p',policy,*cmd,cwd=repo,env=env,stdout=out,stderr=asyncio.subprocess.STDOUT)
        (root/(label+'.started.json')).write_text(json.dumps({'pid':proc.pid,'command':cmd,'env':env}))
        try:
            code = await asyncio.wait_for(proc.wait(),timeout=600)
        except asyncio.TimeoutError:
            proc.terminate()
            code = await proc.wait()
            (root/(label+'.timeout')).write_text('600-second build deadline exceeded')
        (root/(label+'.done.json')).write_text(json.dumps({'pid':proc.pid,'exit':code,'tmp_remaining':list(map(str,(root/'tmp').iterdir()))}))
asyncio.run(main())
```

## Cleanup receipt

After both observed exits and raw-output preservation, the owned profile/runtime/data/session/temp directories, APFS-cloned build artifacts, monitor and registry symlink were removed together. /private/tmp/p32-history.eLkoeO and its owned locator /private/tmp/p32-st_01a0997b-root are absent. No cache target was traversed by cleanup. Final ps query found none of the four owned monitor/cargo PIDs. Original user caches and checkout were not cleanup targets.
