# P03 legacy punctuation encoding - 2026-09-13

Status: current-source same-assertion RED/GREEN complete (lead bash57/bash59 respectively). P03 independently inspected both full raw outputs and receipts. Minimal mapping/constants repair applied; diagnostics clean. Cleanup ownership inventory below; raw evidence root retained. Physical Windows input proof remains pending.

## Allocation

Original owned files: `src-tauri/src/native_terminal/key_encoder.rs`, `src-tauri/tests/native_terminal_engine_contract/key_encoding.rs`, and this report.

Lead additionally approved adding only these two ABI constants in `src-tauri/src/native_terminal/sys/constants.rs`, **only after intended behavioral RED**:

```rust
pub const GHOSTTY_KEY_BACKSLASH: c_int = 2;
pub const GHOSTTY_KEY_BRACKET_RIGHT: c_int = 4;
```

Both were missing and were added only after intended RED, with values verified against the pinned Ghostty `include/ghostty/vt/key/event.h` enum. `sys/types.rs` already re-exports `constants::*` and was not edited. No local duplicate constants or vendor changes were made. Production now imports these constants and maps only Character backslash/right bracket; UTF-8 construction, Escape, and left bracket are untouched.

## Diagnosis and staged regression

Browser key DTO -> NativeTerminalInput::encoded -> TerminalEngine::encode_key -> key_encoder sets an unidentified logical key for punctuation and suppresses implicit UTF-8 under Ctrl. Ghostty legacy ctrlSeq uses UTF-8 or logical-key codepoint, not unshifted_codepoint alone, so backslash/right bracket lose their C0 output.

Two independent fresh real NativeTerminal tests require Character backslash/right bracket, Ctrl, utf8=None -> [28]/[29]. A separate test preserves Escape, existing textless legacy Ctrl-[ behavior, Ctrl-[ with explicit text under fixterms, and negotiated Kitty disambiguation for all three punctuation characters. Existing four key_encoding tests remain unchanged.

Selected tests are synchronous, in-process Ghostty allocation/feed/encoding tests. They launch no daemon, GUI, PTY, network listener, or child. NativeTerminal teardown and encoder/event guards release foreign resources. There are no sleeps or timing-based success conditions. Build tools themselves create compiler children.

Pre-build LSP diagnostics on key_encoder.rs and key_encoding.rs: no diagnostics found. Post-fix fresh LSP diagnostics on key_encoder.rs, sys/constants.rs, and key_encoding.rs: no diagnostics found. Scoped git diff --check also passed. Tests are byte-identical to RED (SHA-256 below).

## Executed isolated commands

Runner: `/tmp/ferryx-p03-st_01a099cf-oQzzlk/run.mjs`.

Lead executed these phases serially after releasing the shared target:

```sh
bun /tmp/ferryx-p03-st_01a099cf-oQzzlk/run.mjs red
# Only after intended RED, minimal production repair, and target release:
bun /tmp/ferryx-p03-st_01a099cf-oQzzlk/run.mjs green
```

Both phases execute exactly:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract key_encoding -- --nocapture
```

CWD: `/Users/indo/code/project/orca-lite`. Explicit direct toolchain PATH, RUSTC, and RUSTDOC use `/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin` (installed directory verified), with no rustup dispatch or inherited process environment. Zig is `/opt/homebrew/bin/zig`, version 0.16.0 observed. Ghostty clean vendor HEAD is `6a508fd5e34c7e222c052a6d00bb3891ff3feace`.

Assigned shared target: `/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target`; assigned Cargo dependency cache: `/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo`, consumed offline without cache copying, installation, or cleanup by P03. Cargo may maintain its own cache locks/metadata; the runner does not chmod the shared cache. Default dev/test profiles are retained: the explicit environment contains no CARGO_PROFILE_* or CARGO_INCREMENTAL overrides (no debug0/incremental0). Jobs=8. The runner launches the command through `/usr/bin/sandbox-exec`: deny all network; deny writes to the repository, `/Users/indo/.cargo`, `/Users/indo/.rustup`, `/Users/indo/.cache`, and `/Users/indo/Library/Caches`. Shared Cargo metadata remains writable, while its registry symlink target `/Users/indo/.cargo/registry` is protected by the ambient Cargo write denial. Git global/system configuration is disabled.

All HOME/USERPROFILE/APPDATA/LOCALAPPDATA, Ferryx runtime/data/session paths, XDG config/cache/data/runtime paths, and TMPDIR/TMP/TEMP point into owned root `/tmp/ferryx-p03-st_01a099cf-oQzzlk`. Runner writes phase log, numeric exit, and JSON receipt containing timestamps, PID, command/environment provenance, and SHA-256 of production/test/constants/Cargo.lock. Logs use exclusive creation to prevent accidental receipt overwrite. Lead serialized shared-target use with P11. Both sandboxed runs completed; no further builds were launched by P03.

## Verification boundary

Physical Windows keyboard/PTY byte proof remains lead runtime work; in-process Darwin encoding tests cannot establish that surface. No desktop, physical-key, GUI, daemon, or full application runtime completion is claimed. No further build was performed after lead GREEN. Changes remain uncommitted in the shared source tree.

## RED receipt and post-fix hashes

Lead bash57 ran the registered command once, exit 101, PID 64564, 2026-09-13T08:16:21.581Z to 08:17:21.704Z. Real Ghostty tests: 5 passed, exactly 2 intended failures, 18 filtered. Backslash returned [] instead of [28]; right bracket returned [] instead of [29]. Escape/fixterms/Kitty compatibility passed before repair. Build emitted 16 pre-existing warnings outside the changed files, preserved verbatim below; none were suppressed or fixed by P03.

Raw files remain untouched in the owned root. SHA-256: red.log fa7c3c57ee8fdfcea70f373766dbef7e46ac33d9745f2f97f8c4c700ac91f60b; red.receipt.json c9c53a03c5ec3211d6a9be655ebf637c967da36cd8bd5d6eef7c895cf0e1c530.

Post-fix SHA-256:

- key_encoder.rs: fb8fbda6b38d9f9a00f962d8159fc14e681c93439961b520a11c4e41b379b940
- sys/constants.rs: d40fe684480c1c4e37f93818f9c1307e1a8568067a31a9152f4fdaf5b181495a
- key_encoding.rs (unchanged from RED): 0f998d7c96e261f6fcd90ef9b2e1f8ac162bf8e355c9199b2bdcc23bcc3a4bf3
- Cargo.lock (unchanged): 1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5

### Full RED environment/result receipt

```json
{
  "phase": "red",
  "cwd": "/Users/indo/code/project/orca-lite",
  "command": [
    "cargo",
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--test",
    "native_terminal_engine_contract",
    "key_encoding",
    "--",
    "--nocapture"
  ],
  "toolchain": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin",
  "env": {
    "HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/home",
    "USERPROFILE": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/profile",
    "APPDATA": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/appdata",
    "LOCALAPPDATA": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/localappdata",
    "FERRYX_RUNTIME_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/runtime",
    "FERRYX_DATA_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/data",
    "FERRYX_SESSION_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/sessions",
    "TMPDIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "TMP": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "TEMP": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "XDG_CONFIG_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/config",
    "XDG_CACHE_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/cache",
    "XDG_DATA_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/data",
    "XDG_RUNTIME_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/runtime",
    "CARGO_TARGET_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target",
    "CARGO_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_BUILD_JOBS": "8",
    "CARGO_TERM_COLOR": "never",
    "RUSTC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc",
    "RUSTDOC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc",
    "RUSTC_WRAPPER": "",
    "ZIG": "/opt/homebrew/bin/zig",
    "PATH": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "LANG": "en_US.UTF-8"
  },
  "policy": "(version 1)(allow default)(deny network*)(deny file-write* (subpath \"/Users/indo/code/project/orca-lite\") (subpath \"/Users/indo/.cargo\") (subpath \"/Users/indo/.rustup\") (subpath \"/Users/indo/.cache\") (subpath \"/Users/indo/Library/Caches\"))",
  "hashes": {
    "src-tauri/src/native_terminal/key_encoder.rs": "05f1996a2ed7daf81657c9855f8c4fec4e40b0919a64a060cb59c38aefc19022",
    "src-tauri/src/native_terminal/sys/constants.rs": "4bd7ac12bad8e69a4dccf6157b68bc1b85bf15ef00feba99699ecc5057faae7b",
    "src-tauri/tests/native_terminal_engine_contract/key_encoding.rs": "0f998d7c96e261f6fcd90ef9b2e1f8ac162bf8e355c9199b2bdcc23bcc3a4bf3",
    "src-tauri/Cargo.lock": "1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5"
  },
  "started": "2026-09-13T08:16:21.581Z",
  "pid": 64564,
  "exit": 101,
  "finished": "2026-09-13T08:17:21.704Z"
}
```

### Full RED output

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
   Compiling bitflags v2.13.1
   Compiling phf_shared v0.13.1
   Compiling zmij v1.0.23
   Compiling objc2-encode v4.1.0
   Compiling autocfg v1.5.1
   Compiling fastrand v2.5.0
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling http v1.5.0
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling getrandom v0.4.3
   Compiling objc2 v0.6.4
   Compiling serde v1.0.229
   Compiling smallvec v1.15.2
   Compiling equivalent v1.0.2
   Compiling num-conv v0.2.2
   Compiling time-core v0.1.9
   Compiling once_cell v1.21.4
   Compiling powerfmt v0.2.0
   Compiling winnow v1.0.4
   Compiling synstructure v0.13.2
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling displaydoc v0.2.7
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling thiserror-impl v2.0.20
   Compiling phf_macros v0.13.1
   Compiling zerofrom v0.1.8
   Compiling base64 v0.22.1
   Compiling phf_codegen v0.13.1
   Compiling thiserror v1.0.69
   Compiling ident_case v1.0.1
   Compiling typeid v1.0.3
   Compiling serde_json v1.0.151
   Compiling strsim v0.11.1
   Compiling darling_core v0.23.0
   Compiling yoke v0.8.3
   Compiling block2 v0.6.2
   Compiling dispatch2 v0.3.1
   Compiling thiserror-impl v1.0.69
   Compiling semver v1.0.28
   Compiling erased-serde v0.4.10
   Compiling objc2-core-foundation v0.3.2
   Compiling byteorder v1.5.0
   Compiling darling_macro v0.23.0
   Compiling aho-corasick v1.1.5
   Compiling new_debug_unreachable v1.0.6
   Compiling regex-syntax v0.8.11
   Compiling fnv v1.0.7
   Compiling unic-common v0.9.0
   Compiling unic-char-range v0.9.0
   Compiling unic-char-property v0.9.0
   Compiling unic-ucd-version v0.9.0
   Compiling darling v0.23.0
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling objc2-foundation v0.3.2
   Compiling phf v0.13.1
   Compiling regex-automata v0.4.18
   Compiling string_cache_codegen v0.6.1
   Compiling alloc-no-stdlib v2.0.4
   Compiling anyhow v1.0.104
   Compiling precomputed-hash v0.1.1
   Compiling alloc-stdlib v0.2.4
   Compiling serde_with_macros v3.22.0
   Compiling web_atoms v0.2.6
   Compiling regex v1.13.1
   Compiling tinystr v0.8.4
   Compiling potential_utf v0.1.6
   Compiling icu_locale_core v2.3.0
   Compiling icu_collections v2.3.0
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling same-file v1.0.6
   Compiling string_cache v0.9.0
   Compiling walkdir v2.5.0
   Compiling brotli-decompressor v5.0.3
   Compiling icu_provider v2.3.0
   Compiling percent-encoding v2.3.2
   Compiling dtoa v1.0.11
   Compiling icu_normalizer v2.3.0
   Compiling icu_properties v2.3.0
   Compiling dunce v1.0.5
   Compiling ctor-proc-macro v0.0.7
   Compiling dtoa-short v0.3.5
   Compiling brotli v8.0.4
   Compiling form_urlencoded v1.2.2
   Compiling ctor v0.8.0
   Compiling uuid v1.26.0
   Compiling tendril v0.5.1
   Compiling selectors v0.36.1
   Compiling cssparser-macros v0.6.1
   Compiling derive_more-impl v2.1.1
   Compiling idna_adapter v1.2.2
   Compiling idna v1.1.0
   Compiling indexmap v1.9.3
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling version_check v0.9.5
   Compiling glob v0.3.4
   Compiling camino v1.2.5
   Compiling toml v1.1.4+spec-1.1.0
   Compiling url v2.5.8
   Compiling derive_more v2.1.1
   Compiling cssparser v0.36.0
   Compiling markup5ever v0.38.0
   Compiling swift-rs v1.0.8
   Compiling bytemuck_derive v1.12.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling bit-vec v0.8.0
   Compiling rustc-hash v2.1.3
   Compiling hashbrown v0.12.3
   Compiling deranged v0.5.8
   Compiling hashbrown v0.17.1
   Compiling schemars v0.8.22
   Compiling schemars_derive v0.8.22
   Compiling bytemuck v1.25.2
   Compiling indexmap v2.14.0
   Compiling time v0.3.55
   Compiling bit-set v0.8.0
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling pin-project-lite v0.2.17
   Compiling base64 v0.21.7
   Compiling dyn-clone v1.0.20
   Compiling foldhash v0.2.0
   Compiling bitflags v1.3.2
   Compiling cargo_metadata v0.19.2
   Compiling infer v0.19.0
   Compiling json-patch v3.0.1
   Compiling dom_query v0.27.0
   Compiling plist v1.10.0
   Compiling serde-untagged v0.1.9
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
   Compiling tauri-utils v2.9.3
   Compiling typenum v1.20.1
   Compiling tokio v1.53.1
   Compiling winnow v0.7.15
   Compiling core-foundation-sys v0.8.7
   Compiling raw-window-handle v0.6.2
   Compiling arrayvec v0.7.8
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling futures-core v0.3.34
   Compiling zerocopy v0.8.56
   Compiling toml v0.9.12+spec-1.1.0
   Compiling block-buffer v0.10.4
   Compiling crypto-common v0.1.7
   Compiling embed-resource v3.0.11
   Compiling dirs-sys v0.5.0
   Compiling zerocopy-derive v0.8.56
   Compiling cfg_aliases v0.2.2
   Compiling heck v0.5.0
   Compiling tauri-winres v0.3.6
   Compiling cargo_toml v0.22.3
   Compiling dirs v6.0.0
   Compiling digest v0.10.7
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling simd-adler32 v0.3.10
   Compiling zeroize v1.9.0
   Compiling crc32fast v1.5.0
   Compiling time-macros v0.2.32
   Compiling adler2 v2.0.1
   Compiling lazy_static v1.5.0
   Compiling miniz_oxide v0.8.9
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling rustls-pki-types v1.15.1
   Compiling fdeflate v0.3.7
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling crossbeam-utils v0.8.22
   Compiling untrusted v0.9.0
   Compiling foreign-types-shared v0.3.1
   Compiling subtle v2.6.1
   Compiling foreign-types v0.5.0
   Compiling cookie v0.18.2
   Compiling tracing v0.1.44
   Compiling http-body v1.1.0
   Compiling futures-sink v0.3.34
   Compiling rustls v0.23.43
   Compiling httparse v1.10.1
   Compiling futures-macro v0.3.34
   Compiling encoding_rs v0.8.35
   Compiling slab v0.4.12
   Compiling tauri v2.11.5
   Compiling futures-task v0.3.34
   Compiling tower-service v0.3.3
   Compiling rustls-webpki v0.103.15
   Compiling futures-util v0.3.34
   Compiling symphonia-metadata v0.5.5
   Compiling crossbeam-channel v0.5.16
   Compiling objc2-web-kit v0.3.2
   Compiling png v0.17.16
   Compiling core-graphics-types v0.2.0
   Compiling cpufeatures v0.2.17
   Compiling tauri-runtime v2.11.3
   Compiling wry v0.55.1
   Compiling mime v0.3.17
   Compiling sha2 v0.10.9
   Compiling ico v0.5.0
   Compiling core-graphics v0.25.0
   Compiling png v0.18.1
   Compiling ppv-lite86 v0.2.21
   Compiling tower-layer v0.3.3
   Compiling rustc-hash v1.1.0
   Compiling try-lock v0.2.5
   Compiling tauri-runtime-wry v2.11.4
   Compiling pxfm v0.1.30
   Compiling unicode-segmentation v1.13.3
   Compiling want v0.3.1
   Compiling naga-types v30.0.1
   Compiling keyboard-types v0.7.0
   Compiling tao v0.35.3
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling sync_wrapper v1.0.2
   Compiling moxcms v0.8.1
   Compiling futures-channel v0.3.34
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling objc-sys v0.3.5
   Compiling unicode-width v0.1.14
   Compiling byteorder-lite v0.1.0
   Compiling httpdate v1.0.3
   Compiling bit-vec v0.9.1
   Compiling atomic-waker v1.1.2
   Compiling codespan-reporting v0.13.1
   Compiling hyper v1.11.0
   Compiling bit-set v0.10.0
   Compiling serialize-to-javascript v0.1.2
   Compiling tauri-macros v2.6.3
   Compiling muda v0.19.3
   Compiling tokio-rustls v0.26.4
   Compiling symphonia-utils-xiph v0.5.5
   Compiling window-vibrancy v0.6.0
   Compiling rand_core v0.9.5
   Compiling half v2.7.1
   Compiling objc2-metal v0.3.2
   Compiling objc2-core-graphics v0.3.2
   Compiling serde_repr v0.1.21
   Compiling static_assertions v1.1.0
   Compiling embed_plist v1.2.2
   Compiling rustix v1.1.4
   Compiling image v0.25.10
   Compiling ipnet v2.12.1
   Compiling wgpu-types v30.0.1
   Compiling hyper-util v0.1.20
   Compiling rand_chacha v0.9.0
   Compiling tower v0.5.3
   Compiling objc2-quartz-core v0.3.2
   Compiling tauri-plugin-fs v2.5.1
   Compiling http-body-util v0.1.5
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling objc2-core-audio v0.3.2
   Compiling raw-window-metal v1.1.0
   Compiling objc2 v0.5.2
   Compiling rand v0.9.5
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling cfg_aliases v0.1.1
   Compiling profiling v1.0.18
   Compiling nix v0.28.0
   Compiling block2 v0.5.1
   Compiling security-framework v3.7.0
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tauri-plugin-updater v2.10.1
   Compiling tauri-plugin-notification v2.3.3
   Compiling tauri-plugin-process v2.3.1
   Compiling tokio-util v0.7.19
   Compiling rand_core v0.6.4
   Compiling wgpu-core v30.0.1
   Compiling num-integer v0.1.47
   Compiling core-foundation v0.9.4
   Compiling curve25519-dalek v4.1.3
   Compiling unicase v2.9.0
   Compiling rfd v0.16.0
   Compiling extended v0.1.0
   Compiling wgpu-naga-bridge v30.0.1
   Compiling ryu v1.0.23
   Compiling dispatch v0.2.0
   Compiling data-encoding v2.11.1
   Compiling litrs v1.0.0
   Compiling cpal v0.17.3
   Compiling objc2-foundation v0.2.2
   Compiling serde_urlencoded v0.7.1
   Compiling tungstenite v0.29.0
   Compiling document-features v0.2.12
   Compiling mime_guess v2.0.5
   Compiling symphonia-format-riff v0.5.5
   Compiling core-graphics-types v0.1.3
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling num-bigint v0.4.8
   Compiling rustls-platform-verifier v0.7.0
   Compiling coreaudio-rs v0.14.2
   Compiling xattr v1.6.1
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling winit v0.30.13
   Compiling wgpu v30.0.1
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
   Compiling reqwest v0.13.4
   Compiling num-rational v0.4.2
   Compiling objc2-app-kit v0.2.2
   Compiling core-graphics v0.23.2
   Compiling notify-rust v4.18.0
   Compiling rand_chacha v0.3.1
   Compiling axum-core v0.5.6
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite/src-tauri)
   Compiling tracing-log v0.2.0
   Compiling sharded-slab v0.1.7
   Compiling filedescriptor v0.8.3
   Compiling serial2 v0.2.38
   Compiling fsevent-sys v4.1.0
   Compiling notify-types v2.1.0
   Compiling serde_path_to_error v0.1.20
   Compiling thread_local v1.1.10
   Compiling shell-words v1.1.1
   Compiling cursor-icon v1.2.0
   Compiling nu-ansi-term v0.50.3
   Compiling downcast-rs v1.2.1
   Compiling matchit v0.8.4
   Compiling minisign-verify v0.2.5
   Compiling smol_str v0.2.2
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
    Finished `test` profile [unoptimized + debuginfo] target(s) in 59.32s
     Running tests/native_terminal_engine_contract.rs (/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/native_terminal_engine_contract-fb414f1faeea60e4)

running 7 tests

thread 'key_encoding::test_key_encode_legacy_ctrl_backslash_without_utf8' (3252885) panicked at tests/native_terminal_engine_contract/key_encoding.rs:91:5:
assertion `left == right` failed
  left: []
 right: [28]
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'key_encoding::test_key_encode_legacy_ctrl_right_bracket_without_utf8' (3252886) panicked at tests/native_terminal_engine_contract/key_encoding.rs:103:5:
assertion `left == right` failed
  left: []
 right: [29]
test key_encoding::test_key_encode_rejects_c0_control_utf8_payload ... ok
test key_encoding::test_key_encode_rejects_pua_utf8_payload ... ok
test key_encoding::test_key_encode_rejects_del_utf8_payload ... ok
test key_encoding::test_key_encode_punctuation_preserves_escape_fixterms_and_kitty ... ok
test key_encoding::test_key_encode_plain_and_mode_sensitive_arrow_keys ... ok
test key_encoding::test_key_encode_legacy_ctrl_backslash_without_utf8 ... FAILED
test key_encoding::test_key_encode_legacy_ctrl_right_bracket_without_utf8 ... FAILED

failures:

failures:
    key_encoding::test_key_encode_legacy_ctrl_backslash_without_utf8
    key_encoding::test_key_encode_legacy_ctrl_right_bracket_without_utf8

test result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 18 filtered out; finished in 0.00s

error: test failed, to rerun pass `--test native_terminal_engine_contract`
```

## GREEN receipt, source binding, and cleanup ownership

Lead bash59: exit 0, PID 70687, 2026-09-13T08:22:37.900Z to 2026-09-13T08:23:54.800Z. P03 independently read full raw log: 7 passed, 0 failed, 18 filtered; both formerly failing C0 assertions and all compatibility checks passed. The same 16 pre-existing unrelated warnings remain visible.

Machine-checked equality: RED and GREEN command, complete explicit environment, sandbox policy, toolchain path, and cwd are identical. RED test hash == GREEN test hash == current test hash: 0f998d7c96e261f6fcd90ef9b2e1f8ac162bf8e355c9199b2bdcc23bcc3a4bf3. Every current file hash in GREEN receipt (encoder, constants, tests, Cargo.lock) matches its pre-execution receipt. This binds the assertions and focused repair, not every concurrently edited foreign file in the repository.

GREEN log SHA-256: 114587b07416d8ad03222b54273c413ceddbf39e1e1018b56939208c173b16da. GREEN receipt SHA-256: 2229fec1d82d23b561f512cd885d1321ff70e0213960fc3587bc5d0a59127fa4. Executed binary path from both logs: /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/native_terminal_engine_contract-fb414f1faeea60e4. Post-GREEN observed binary SHA-256: 91c1dcb2575d138f939870c3dd8044d7508708e4787e594d765375eb42afb55e; shared artifact may be replaced by other lead runs, so this is an observation at evidence capture, not an immutable retained executable.

### Cleanup ownership disposition

P03 owns only /tmp/ferryx-p03-st_01a099cf-oQzzlk; inventory below shows all 11 profile/runtime/cache/temp/local-target directories empty. No session files, sockets, or test-created child resources remain there. Runner receipts record Cargo child exits (RED 101, GREEN 0); selected tests never spawn daemon/GUI/PTY processes. No kill, restart, or process manipulation was performed. Raw logs, receipts, exit files, and runner are intentionally retained for lead evidence collection; no claim that the owned root was deleted. Final removal of this evidence-only root is assigned to lead after collection.

Borrowed resources are NOT P03 cleanup scope: shared target /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target, shared Cargo metadata /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo, ambient registry symlink target /Users/indo/.cargo/registry, installed Rust/Zig, and repository/vendor. P03 deleted or modified none of these during evidence capture. No further build ran. Physical Windows input remains pending, not runtime-complete.

Inventory captured 2026-09-13T08:26:34.561Z:

```json
[
  {
    "name": "red.exit",
    "type": "file",
    "bytes": 4,
    "sha256": "39b8dc3fc8b44765c8e6f1adee04c5b465e555ab791cc42d0d9e810d5b64297c"
  },
  {
    "name": "home",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "run.mjs",
    "type": "file",
    "bytes": 2982,
    "sha256": "e20105376330397a21f2fa9a85f3f3dfd067bd5312767038deabcee81ed59d41"
  },
  {
    "name": "cache",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "red.log",
    "type": "file",
    "bytes": 19403,
    "sha256": "fa7c3c57ee8fdfcea70f373766dbef7e46ac33d9745f2f97f8c4c700ac91f60b"
  },
  {
    "name": "config",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "green.exit",
    "type": "file",
    "bytes": 2,
    "sha256": "9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa"
  },
  {
    "name": "target",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "green.log",
    "type": "file",
    "bytes": 18641,
    "sha256": "114587b07416d8ad03222b54273c413ceddbf39e1e1018b56939208c173b16da"
  },
  {
    "name": "runtime",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "localappdata",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "green.receipt.json",
    "type": "file",
    "bytes": 2831,
    "sha256": "2229fec1d82d23b561f512cd885d1321ff70e0213960fc3587bc5d0a59127fa4"
  },
  {
    "name": "sessions",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "profile",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "appdata",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "data",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "tmp",
    "type": "directory",
    "bytes": 64,
    "entries": []
  },
  {
    "name": "red.receipt.json",
    "type": "file",
    "bytes": 2831,
    "sha256": "c9c53a03c5ec3211d6a9be655ebf637c967da36cd8bd5d6eef7c895cf0e1c530"
  }
]
```

### Full GREEN environment/result receipt

```json
{
  "phase": "green",
  "cwd": "/Users/indo/code/project/orca-lite",
  "command": [
    "cargo",
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--test",
    "native_terminal_engine_contract",
    "key_encoding",
    "--",
    "--nocapture"
  ],
  "toolchain": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin",
  "env": {
    "HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/home",
    "USERPROFILE": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/profile",
    "APPDATA": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/appdata",
    "LOCALAPPDATA": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/localappdata",
    "FERRYX_RUNTIME_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/runtime",
    "FERRYX_DATA_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/data",
    "FERRYX_SESSION_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/sessions",
    "TMPDIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "TMP": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "TEMP": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/tmp",
    "XDG_CONFIG_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/config",
    "XDG_CACHE_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/cache",
    "XDG_DATA_HOME": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/data",
    "XDG_RUNTIME_DIR": "/tmp/ferryx-p03-st_01a099cf-oQzzlk/runtime",
    "CARGO_TARGET_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target",
    "CARGO_HOME": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_BUILD_JOBS": "8",
    "CARGO_TERM_COLOR": "never",
    "RUSTC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc",
    "RUSTDOC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc",
    "RUSTC_WRAPPER": "",
    "ZIG": "/opt/homebrew/bin/zig",
    "PATH": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "LANG": "en_US.UTF-8"
  },
  "policy": "(version 1)(allow default)(deny network*)(deny file-write* (subpath \"/Users/indo/code/project/orca-lite\") (subpath \"/Users/indo/.cargo\") (subpath \"/Users/indo/.rustup\") (subpath \"/Users/indo/.cache\") (subpath \"/Users/indo/Library/Caches\"))",
  "hashes": {
    "src-tauri/src/native_terminal/key_encoder.rs": "fb8fbda6b38d9f9a00f962d8159fc14e681c93439961b520a11c4e41b379b940",
    "src-tauri/src/native_terminal/sys/constants.rs": "d40fe684480c1c4e37f93818f9c1307e1a8568067a31a9152f4fdaf5b181495a",
    "src-tauri/tests/native_terminal_engine_contract/key_encoding.rs": "0f998d7c96e261f6fcd90ef9b2e1f8ac162bf8e355c9199b2bdcc23bcc3a4bf3",
    "src-tauri/Cargo.lock": "1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5"
  },
  "started": "2026-09-13T08:22:37.900Z",
  "pid": 70687,
  "exit": 0,
  "finished": "2026-09-13T08:23:54.800Z"
}
```

### Full GREEN output

```text
   Compiling proc-macro2 v1.0.107
   Compiling quote v1.0.47
   Compiling unicode-ident v1.0.24
   Compiling cfg-if v1.0.4
   Compiling libc v0.2.189
   Compiling serde_core v1.0.229
   Compiling find-msvc-tools v0.1.11
   Compiling shlex v2.0.1
   Compiling cc v1.4.3
   Compiling itoa v1.0.18
   Compiling memchr v2.8.3
   Compiling parking_lot_core v0.9.12
   Compiling scopeguard v1.2.0
   Compiling log v0.4.33
   Compiling lock_api v0.4.14
   Compiling litemap v0.8.3
   Compiling writeable v0.6.4
   Compiling bytes v1.12.1
   Compiling utf8_iter v1.0.4
   Compiling siphasher v1.0.3
   Compiling thiserror v2.0.20
   Compiling icu_normalizer_data v2.3.0
   Compiling icu_properties_data v2.3.0
   Compiling phf_shared v0.13.1
   Compiling zmij v1.0.23
   Compiling bitflags v2.13.1
   Compiling autocfg v1.5.1
   Compiling objc2-encode v4.1.0
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling fastrand v2.5.0
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling http v1.5.0
   Compiling objc2 v0.6.4
   Compiling getrandom v0.4.3
   Compiling smallvec v1.15.2
   Compiling equivalent v1.0.2
   Compiling serde v1.0.229
   Compiling num-conv v0.2.2
   Compiling time-core v0.1.9
   Compiling once_cell v1.21.4
   Compiling powerfmt v0.2.0
   Compiling winnow v1.0.4
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling base64 v0.22.1
   Compiling phf_codegen v0.13.1
   Compiling strsim v0.11.1
   Compiling thiserror v1.0.69
   Compiling ident_case v1.0.1
   Compiling serde_json v1.0.151
   Compiling typeid v1.0.3
   Compiling semver v1.0.28
   Compiling erased-serde v0.4.10
   Compiling byteorder v1.5.0
   Compiling aho-corasick v1.1.5
   Compiling synstructure v0.13.2
   Compiling block2 v0.6.2
   Compiling darling_core v0.23.0
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling displaydoc v0.2.7
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling thiserror-impl v2.0.20
   Compiling phf_macros v0.13.1
   Compiling dispatch2 v0.3.1
   Compiling thiserror-impl v1.0.69
   Compiling objc2-core-foundation v0.3.2
   Compiling zerofrom v0.1.8
   Compiling unic-char-range v0.9.0
   Compiling new_debug_unreachable v1.0.6
   Compiling unic-common v0.9.0
   Compiling fnv v1.0.7
   Compiling regex-syntax v0.8.11
   Compiling unic-ucd-version v0.9.0
   Compiling darling_macro v0.23.0
   Compiling unic-char-property v0.9.0
   Compiling phf v0.13.1
   Compiling darling v0.23.0
   Compiling objc2-foundation v0.3.2
   Compiling yoke v0.8.3
   Compiling regex-automata v0.4.18
   Compiling string_cache_codegen v0.6.1
   Compiling anyhow v1.0.104
   Compiling precomputed-hash v0.1.1
   Compiling alloc-no-stdlib v2.0.4
   Compiling alloc-stdlib v0.2.4
   Compiling web_atoms v0.2.6
   Compiling regex v1.13.1
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling same-file v1.0.6
   Compiling string_cache v0.9.0
   Compiling walkdir v2.5.0
   Compiling brotli-decompressor v5.0.3
   Compiling dtoa v1.0.11
   Compiling dunce v1.0.5
   Compiling ctor-proc-macro v0.0.7
   Compiling percent-encoding v2.3.2
   Compiling ctor v0.8.0
   Compiling form_urlencoded v1.2.2
   Compiling dtoa-short v0.3.5
   Compiling serde_with_macros v3.22.0
   Compiling brotli v8.0.4
   Compiling uuid v1.26.0
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling tendril v0.5.1
   Compiling derive_more-impl v2.1.1
   Compiling cssparser-macros v0.6.1
   Compiling selectors v0.36.1
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling indexmap v1.9.3
   Compiling version_check v0.9.5
   Compiling glob v0.3.4
   Compiling camino v1.2.5
   Compiling markup5ever v0.38.0
   Compiling toml v1.1.4+spec-1.1.0
   Compiling derive_more v2.1.1
   Compiling cssparser v0.36.0
   Compiling tinystr v0.8.4
   Compiling icu_locale_core v2.3.0
   Compiling potential_utf v0.1.6
   Compiling icu_collections v2.3.0
   Compiling swift-rs v1.0.8
   Compiling icu_provider v2.3.0
   Compiling bytemuck_derive v1.12.0
   Compiling icu_normalizer v2.3.0
   Compiling icu_properties v2.3.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling schemars v0.8.22
   Compiling bit-vec v0.8.0
   Compiling hashbrown v0.12.3
   Compiling rustc-hash v2.1.3
   Compiling deranged v0.5.8
   Compiling idna_adapter v1.2.2
   Compiling idna v1.1.0
   Compiling hashbrown v0.17.1
   Compiling bit-set v0.8.0
   Compiling url v2.5.8
   Compiling time v0.3.55
   Compiling indexmap v2.14.0
   Compiling schemars_derive v0.8.22
   Compiling bytemuck v1.25.2
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling bitflags v1.3.2
   Compiling foldhash v0.2.0
   Compiling pin-project-lite v0.2.17
   Compiling dyn-clone v1.0.20
   Compiling base64 v0.21.7
   Compiling cargo_metadata v0.19.2
   Compiling plist v1.10.0
   Compiling serde-untagged v0.1.9
   Compiling infer v0.19.0
   Compiling json-patch v3.0.1
   Compiling urlpattern v0.3.0
   Compiling serde_with v3.22.0
   Compiling errno v0.3.14
   Compiling dom_query v0.27.0
   Compiling rustc_version v0.4.1
   Compiling option-ext v0.2.0
   Compiling libm v0.2.16
   Compiling generic-array v0.14.7
   Compiling num-traits v0.2.19
   Compiling signal-hook-registry v1.4.8
   Compiling tokio-macros v2.7.2
   Compiling socket2 v0.6.5
   Compiling mio v1.2.2
   Compiling typenum v1.20.1
   Compiling tokio v1.53.1
   Compiling arrayvec v0.7.8
   Compiling core-foundation-sys v0.8.7
   Compiling winnow v0.7.15
   Compiling raw-window-handle v0.6.2
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling zerocopy v0.8.56
   Compiling futures-core v0.3.34
   Compiling toml v0.9.12+spec-1.1.0
   Compiling dirs-sys v0.5.0
   Compiling embed-resource v3.0.11
   Compiling zerocopy-derive v0.8.56
   Compiling heck v0.5.0
   Compiling cfg_aliases v0.2.2
   Compiling tauri-utils v2.9.3
   Compiling tauri-winres v0.3.6
   Compiling dirs v6.0.0
   Compiling cargo_toml v0.22.3
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling crc32fast v1.5.0
   Compiling simd-adler32 v0.3.10
   Compiling zeroize v1.9.0
   Compiling time-macros v0.2.32
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling block-buffer v0.10.4
   Compiling crypto-common v0.1.7
   Compiling digest v0.10.7
   Compiling lazy_static v1.5.0
   Compiling adler2 v2.0.1
   Compiling miniz_oxide v0.8.9
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling tauri v2.11.5
   Compiling rustls-pki-types v1.15.1
   Compiling fdeflate v0.3.7
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling subtle v2.6.1
   Compiling untrusted v0.9.0
   Compiling crossbeam-utils v0.8.22
   Compiling foreign-types-shared v0.3.1
   Compiling tracing v0.1.44
   Compiling cookie v0.18.2
   Compiling foreign-types v0.5.0
   Compiling http-body v1.1.0
   Compiling rustls v0.23.43
   Compiling httparse v1.10.1
   Compiling futures-sink v0.3.34
   Compiling futures-macro v0.3.34
   Compiling encoding_rs v0.8.35
   Compiling slab v0.4.12
   Compiling tower-service v0.3.3
   Compiling futures-task v0.3.34
   Compiling futures-util v0.3.34
   Compiling rustls-webpki v0.103.15
   Compiling symphonia-metadata v0.5.5
   Compiling crossbeam-channel v0.5.16
   Compiling core-graphics-types v0.2.0
   Compiling png v0.17.16
   Compiling objc2-web-kit v0.3.2
   Compiling cpufeatures v0.2.17
   Compiling tauri-runtime v2.11.3
   Compiling wry v0.55.1
   Compiling mime v0.3.17
   Compiling ico v0.5.0
   Compiling sha2 v0.10.9
   Compiling core-graphics v0.25.0
   Compiling ppv-lite86 v0.2.21
   Compiling png v0.18.1
   Compiling tauri-runtime-wry v2.11.4
   Compiling pxfm v0.1.30
   Compiling try-lock v0.2.5
   Compiling rustc-hash v1.1.0
   Compiling unicode-segmentation v1.13.3
   Compiling tower-layer v0.3.3
   Compiling naga-types v30.0.1
   Compiling keyboard-types v0.7.0
   Compiling want v0.3.1
   Compiling tao v0.35.3
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling moxcms v0.8.1
   Compiling sync_wrapper v1.0.2
   Compiling futures-channel v0.3.34
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling objc-sys v0.3.5
   Compiling byteorder-lite v0.1.0
   Compiling bit-vec v0.9.1
   Compiling unicode-width v0.1.14
   Compiling httpdate v1.0.3
   Compiling atomic-waker v1.1.2
   Compiling bit-set v0.10.0
   Compiling codespan-reporting v0.13.1
   Compiling hyper v1.11.0
   Compiling tauri-macros v2.6.3
   Compiling serialize-to-javascript v0.1.2
   Compiling muda v0.19.3
   Compiling tokio-rustls v0.26.4
   Compiling symphonia-utils-xiph v0.5.5
   Compiling half v2.7.1
   Compiling rand_core v0.9.5
   Compiling window-vibrancy v0.6.0
   Compiling objc2-metal v0.3.2
   Compiling objc2-core-graphics v0.3.2
   Compiling serde_repr v0.1.21
   Compiling image v0.25.10
   Compiling ipnet v2.12.1
   Compiling static_assertions v1.1.0
   Compiling embed_plist v1.2.2
   Compiling rustix v1.1.4
   Compiling hyper-util v0.1.20
   Compiling wgpu-types v30.0.1
   Compiling rand_chacha v0.9.0
   Compiling objc2-quartz-core v0.3.2
   Compiling tower v0.5.3
   Compiling http-body-util v0.1.5
   Compiling tauri-plugin-fs v2.5.1
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling raw-window-metal v1.1.0
   Compiling objc2 v0.5.2
   Compiling rand v0.9.5
   Compiling objc2-core-audio v0.3.2
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling profiling v1.0.18
   Compiling cfg_aliases v0.1.1
   Compiling security-framework v3.7.0
   Compiling nix v0.28.0
   Compiling block2 v0.5.1
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tokio-util v0.7.19
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tauri-plugin-updater v2.10.1
   Compiling wgpu-naga-bridge v30.0.1
   Compiling tauri-plugin-notification v2.3.3
   Compiling tauri-plugin-process v2.3.1
   Compiling rand_core v0.6.4
   Compiling num-integer v0.1.47
   Compiling wgpu-core v30.0.1
   Compiling core-foundation v0.9.4
   Compiling curve25519-dalek v4.1.3
   Compiling data-encoding v2.11.1
   Compiling rfd v0.16.0
   Compiling cpal v0.17.3
   Compiling extended v0.1.0
   Compiling ryu v1.0.23
   Compiling litrs v1.0.0
   Compiling dispatch v0.2.0
   Compiling unicase v2.9.0
   Compiling objc2-foundation v0.2.2
   Compiling document-features v0.2.12
   Compiling mime_guess v2.0.5
   Compiling serde_urlencoded v0.7.1
   Compiling symphonia-format-riff v0.5.5
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling tungstenite v0.29.0
   Compiling core-graphics-types v0.1.3
   Compiling num-bigint v0.4.8
   Compiling coreaudio-rs v0.14.2
   Compiling rustls-platform-verifier v0.7.0
   Compiling xattr v1.6.1
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling winit v0.30.13
   Compiling wgpu v30.0.1
   Compiling filetime v0.2.29
   Compiling mach2 v0.5.0
   Compiling dasp_sample v0.11.0
   Compiling signature v2.2.0
   Compiling ed25519 v2.2.3
   Compiling tempfile v3.27.0
   Compiling tar v0.4.46
   Compiling osakit v0.3.1
   Compiling symphonia v0.5.5
   Compiling tokio-tungstenite v0.29.0
   Compiling reqwest v0.13.4
   Compiling num-rational v0.4.2
   Compiling notify-rust v4.18.0
   Compiling core-graphics v0.23.2
   Compiling objc2-app-kit v0.2.2
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
   Compiling shell-words v1.1.1
   Compiling cursor-icon v1.2.0
   Compiling minisign-verify v0.2.5
   Compiling smol_str v0.2.2
   Compiling matchit v0.8.4
   Compiling downcast-rs v1.2.1
   Compiling nu-ansi-term v0.50.3
   Compiling portable-pty v0.9.0
   Compiling tracing-subscriber v0.3.23
   Compiling axum v0.8.9
   Compiling notify v8.2.0
   Compiling ed25519-dalek v2.2.0
   Compiling rand v0.8.7
   Compiling rodio v0.22.2
   Compiling reqwest v0.12.28
   Compiling tower-http v0.7.1
   Compiling objc2-user-notifications v0.3.2
   Compiling objc2-core-text v0.3.2
   Compiling base64 v0.23.1
   Compiling pollster v1.0.1
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
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 16s
     Running tests/native_terminal_engine_contract.rs (/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/native_terminal_engine_contract-fb414f1faeea60e4)

running 7 tests
test key_encoding::test_key_encode_rejects_pua_utf8_payload ... ok
test key_encoding::test_key_encode_legacy_ctrl_right_bracket_without_utf8 ... ok
test key_encoding::test_key_encode_rejects_del_utf8_payload ... ok
test key_encoding::test_key_encode_legacy_ctrl_backslash_without_utf8 ... ok
test key_encoding::test_key_encode_rejects_c0_control_utf8_payload ... ok
test key_encoding::test_key_encode_punctuation_preserves_escape_fixterms_and_kitty ... ok
test key_encoding::test_key_encode_plain_and_mode_sensitive_arrow_keys ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out; finished in 0.00s

```
