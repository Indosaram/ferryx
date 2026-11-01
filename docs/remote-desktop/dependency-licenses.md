# Declared dependency licenses

Audited on 2026-09-09 using `cargo metadata --locked --format-version 1` on Omarchy.
No compilation was required. This inventory covers the default-feature, all-target
metadata graphs for both committed Cargo workspaces. It includes build and test
dependencies, not just the libraries linked into one platform binary.

## Results

- Main workspace: 576 packages, including 9 first-party packages and 567 third-party package versions.
- ScreenCaptureKit prototype: 17 third-party package versions; 0 versions are additional to the main graph.
- No third-party package in these graphs lacked a declared license expression.
- Main-workspace packages already declared MIT. The prototype manifest was missing
  that field; it now explicitly declares MIT, matching the repository LICENSE.
- Native FFmpeg, codec libraries, SDKs, operating-system libraries and drivers are
  outside Cargo metadata. The existing nonfree Linux FFmpeg build is not
  redistributable; see [third-party notices](../THIRD_PARTY_NOTICES.md).

This is a metadata inventory, not a replacement for package copyright, LICENSE
or NOTICE files. License expressions are preserved exactly as declared upstream.
It does not certify every possible feature combination or binary distribution.

Lockfile SHA-256 values:

- `clients/rust/Cargo.lock`: `6074942d9171f7b7a499c46facd8c8d82c46aedbe37d4e8dc97d9580aeef21e5`
- `Cargo.lock`: `31c0f6ccba2b31a46a032cb281ac89d61cdb845be9f1bf0a32aef609d4d5c401`

## Main workspace

| Package | Version | Declared license |
| --- | --- | --- |
| `adler2` | `2.0.1` | 0BSD OR MIT OR Apache-2.0 |
| `aead` | `0.5.2` | MIT OR Apache-2.0 |
| `aes` | `0.8.4` | MIT OR Apache-2.0 |
| `aes-gcm` | `0.10.3` | Apache-2.0 OR MIT |
| `aho-corasick` | `1.1.5` | Unlicense OR MIT |
| `alsa` | `0.9.1` | Apache-2.0/MIT |
| `alsa-sys` | `0.3.1` | MIT |
| `android_system_properties` | `0.1.6` | MIT OR Apache-2.0 |
| `anstream` | `1.0.0` | MIT OR Apache-2.0 |
| `anstyle` | `1.0.14` | MIT OR Apache-2.0 |
| `anstyle-parse` | `1.0.0` | MIT OR Apache-2.0 |
| `anstyle-query` | `1.1.5` | MIT OR Apache-2.0 |
| `anstyle-wincon` | `3.0.11` | MIT OR Apache-2.0 |
| `anyhow` | `1.0.104` | MIT OR Apache-2.0 |
| `atk` | `0.18.2` | MIT |
| `atk-sys` | `0.18.2` | MIT |
| `atomic-waker` | `1.1.2` | Apache-2.0 OR MIT |
| `autocfg` | `1.5.1` | Apache-2.0 OR MIT |
| `base64` | `0.21.7` | MIT OR Apache-2.0 |
| `base64` | `0.22.1` | MIT OR Apache-2.0 |
| `bindgen` | `0.72.1` | BSD-3-Clause |
| `bit-set` | `0.8.0` | Apache-2.0 OR MIT |
| `bit-vec` | `0.8.0` | Apache-2.0 OR MIT |
| `bitfields` | `1.0.3` | MIT |
| `bitfields-impl` | `1.0.3` | MIT |
| `bitflags` | `1.3.2` | MIT/Apache-2.0 |
| `bitflags` | `2.13.1` | MIT OR Apache-2.0 |
| `bitvec` | `1.1.1` | MIT |
| `block-buffer` | `0.10.4` | MIT OR Apache-2.0 |
| `block2` | `0.6.2` | MIT |
| `bs58` | `0.5.1` | MIT/Apache-2.0 |
| `bumpalo` | `3.20.3` | MIT OR Apache-2.0 |
| `bytemuck` | `1.25.2` | Zlib OR Apache-2.0 OR MIT |
| `byteorder` | `1.5.0` | Unlicense OR MIT |
| `byteorder-lite` | `0.1.0` | Unlicense OR MIT |
| `bytes` | `1.12.1` | MIT |
| `cairo-rs` | `0.18.5` | MIT |
| `cairo-sys-rs` | `0.18.2` | MIT |
| `camino` | `1.2.5` | MIT OR Apache-2.0 |
| `cargo_metadata` | `0.19.2` | MIT |
| `cargo_toml` | `0.22.3` | Apache-2.0 OR MIT |
| `cargo-platform` | `0.1.9` | MIT OR Apache-2.0 |
| `cc` | `1.4.4` | MIT OR Apache-2.0 |
| `cesu8` | `1.1.0` | Apache-2.0/MIT |
| `cexpr` | `0.6.0` | Apache-2.0/MIT |
| `cfb` | `0.7.3` | MIT |
| `cfg_aliases` | `0.2.2` | MIT |
| `cfg-expr` | `0.15.8` | MIT OR Apache-2.0 |
| `cfg-if` | `1.0.4` | MIT OR Apache-2.0 |
| `chacha20` | `0.10.2` | MIT OR Apache-2.0 |
| `chrono` | `0.4.45` | MIT OR Apache-2.0 |
| `cipher` | `0.4.4` | MIT OR Apache-2.0 |
| `clang-sys` | `1.9.1` | Apache-2.0 |
| `clap` | `4.6.6` | MIT OR Apache-2.0 |
| `clap_builder` | `4.6.6` | MIT OR Apache-2.0 |
| `clap_derive` | `4.6.4` | MIT OR Apache-2.0 |
| `clap_lex` | `1.1.0` | MIT OR Apache-2.0 |
| `clipboard-win` | `5.4.1` | BSL-1.0 |
| `colorchoice` | `1.0.5` | MIT OR Apache-2.0 |
| `combine` | `4.6.8` | MIT |
| `cookie` | `0.18.2` | MIT OR Apache-2.0 |
| `core-foundation` | `0.10.1` | MIT OR Apache-2.0 |
| `core-foundation-sys` | `0.8.7` | MIT OR Apache-2.0 |
| `core-graphics` | `0.25.0` | MIT OR Apache-2.0 |
| `core-graphics-types` | `0.2.0` | MIT OR Apache-2.0 |
| `coreaudio-rs` | `0.13.0` | MIT/Apache-2.0 |
| `cpal` | `0.16.0` | Apache-2.0 |
| `cpufeatures` | `0.2.17` | MIT OR Apache-2.0 |
| `cpufeatures` | `0.3.1` | MIT OR Apache-2.0 |
| `crc32fast` | `1.5.1` | MIT OR Apache-2.0 |
| `crossbeam-channel` | `0.5.16` | MIT OR Apache-2.0 |
| `crossbeam-utils` | `0.8.22` | MIT OR Apache-2.0 |
| `crypto-common` | `0.1.7` | MIT OR Apache-2.0 |
| `cssparser` | `0.36.0` | MPL-2.0 |
| `cssparser-macros` | `0.6.1` | MPL-2.0 |
| `ctor` | `0.8.0` | Apache-2.0 OR MIT |
| `ctor-proc-macro` | `0.0.7` | Apache-2.0 OR MIT |
| `ctr` | `0.9.2` | MIT OR Apache-2.0 |
| `darling` | `0.23.0` | MIT |
| `darling_core` | `0.23.0` | MIT |
| `darling_macro` | `0.23.0` | MIT |
| `dasp_sample` | `0.11.0` | MIT OR Apache-2.0 |
| `defmt` | `1.1.1` | MIT OR Apache-2.0 |
| `defmt-macros` | `1.1.1` | MIT OR Apache-2.0 |
| `defmt-parser` | `1.0.0` | MIT OR Apache-2.0 |
| `deranged` | `0.5.8` | MIT OR Apache-2.0 |
| `derive_more` | `2.1.1` | MIT |
| `derive_more-impl` | `2.1.1` | MIT |
| `digest` | `0.10.7` | MIT OR Apache-2.0 |
| `dirs` | `6.0.0` | MIT OR Apache-2.0 |
| `dirs-sys` | `0.5.0` | MIT OR Apache-2.0 |
| `dispatch2` | `0.3.1` | Zlib OR Apache-2.0 OR MIT |
| `displaydoc` | `0.2.7` | MIT OR Apache-2.0 |
| `dlopen2` | `0.8.2` | MIT |
| `dlopen2_derive` | `0.4.3` | MIT |
| `dom_query` | `0.27.0` | MIT |
| `downcast-rs` | `1.2.1` | MIT/Apache-2.0 |
| `dpi` | `0.1.2` | Apache-2.0 AND MIT |
| `dtoa` | `1.0.11` | MIT OR Apache-2.0 |
| `dtoa-short` | `0.3.5` | MPL-2.0 |
| `dtor` | `0.3.0` | Apache-2.0 OR MIT |
| `dtor-proc-macro` | `0.0.6` | Apache-2.0 OR MIT |
| `dunce` | `1.0.5` | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| `dxgi-capture-rs` | `1.2.2` | MIT |
| `dyn-clone` | `1.0.20` | MIT OR Apache-2.0 |
| `either` | `1.18.0` | MIT OR Apache-2.0 |
| `embed_plist` | `1.2.2` | MIT OR Apache-2.0 |
| `embed-resource` | `3.0.11` | MIT |
| `equivalent` | `1.0.2` | Apache-2.0 OR MIT |
| `erased-serde` | `0.4.10` | MIT OR Apache-2.0 |
| `errno` | `0.3.14` | MIT OR Apache-2.0 |
| `error-code` | `3.4.0` | BSL-1.0 |
| `evdev` | `0.13.2` | Apache-2.0 OR MIT |
| `fastrand` | `2.5.0` | Apache-2.0 OR MIT |
| `fdeflate` | `0.3.7` | MIT OR Apache-2.0 |
| `ffmpeg-next` | `8.1.0` | WTFPL |
| `ffmpeg-sys-next` | `8.1.0` | WTFPL |
| `field-offset` | `0.3.6` | MIT OR Apache-2.0 |
| `find-msvc-tools` | `0.1.11` | MIT OR Apache-2.0 |
| `flate2` | `1.1.10` | MIT OR Apache-2.0 |
| `flume` | `0.11.1` | Apache-2.0/MIT |
| `fnv` | `1.0.7` | Apache-2.0 / MIT |
| `foldhash` | `0.2.0` | Zlib |
| `foreign-types` | `0.3.2` | MIT/Apache-2.0 |
| `foreign-types` | `0.5.0` | MIT/Apache-2.0 |
| `foreign-types-macros` | `0.2.4` | MIT/Apache-2.0 |
| `foreign-types-shared` | `0.1.1` | MIT/Apache-2.0 |
| `foreign-types-shared` | `0.3.1` | MIT/Apache-2.0 |
| `form_urlencoded` | `1.2.2` | MIT OR Apache-2.0 |
| `funty` | `2.0.0` | MIT |
| `futures-channel` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-core` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-executor` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-io` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-macro` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-sink` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-task` | `0.3.34` | MIT OR Apache-2.0 |
| `futures-util` | `0.3.34` | MIT OR Apache-2.0 |
| `gdk` | `0.18.2` | MIT |
| `gdk-pixbuf` | `0.18.5` | MIT |
| `gdk-pixbuf-sys` | `0.18.0` | MIT |
| `gdk-sys` | `0.18.2` | MIT |
| `gdkwayland-sys` | `0.18.2` | MIT |
| `generic-array` | `0.14.7` | MIT |
| `getrandom` | `0.2.17` | MIT OR Apache-2.0 |
| `getrandom` | `0.3.4` | MIT OR Apache-2.0 |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 |
| `ghash` | `0.5.1` | Apache-2.0 OR MIT |
| `gio` | `0.18.4` | MIT |
| `gio-sys` | `0.18.1` | MIT |
| `glib` | `0.18.5` | MIT |
| `glib-macros` | `0.18.5` | MIT |
| `glib-sys` | `0.18.1` | MIT |
| `glob` | `0.3.4` | MIT OR Apache-2.0 |
| `gobject-sys` | `0.18.0` | MIT |
| `gtk` | `0.18.2` | MIT |
| `gtk-sys` | `0.18.2` | MIT |
| `gtk3-macros` | `0.18.2` | MIT |
| `hashbrown` | `0.12.3` | MIT OR Apache-2.0 |
| `hashbrown` | `0.17.1` | MIT OR Apache-2.0 |
| `heck` | `0.4.1` | MIT OR Apache-2.0 |
| `heck` | `0.5.0` | MIT OR Apache-2.0 |
| `hermit-abi` | `0.5.3` | MIT OR Apache-2.0 |
| `hex` | `0.4.3` | MIT OR Apache-2.0 |
| `hkdf` | `0.12.4` | MIT OR Apache-2.0 |
| `hmac` | `0.12.1` | MIT OR Apache-2.0 |
| `hostname` | `0.4.2` | MIT |
| `html5ever` | `0.38.0` | MIT OR Apache-2.0 |
| `http` | `1.5.0` | MIT OR Apache-2.0 |
| `http-body` | `1.1.0` | MIT |
| `http-body-util` | `0.1.5` | MIT |
| `httparse` | `1.10.1` | MIT OR Apache-2.0 |
| `hyper` | `1.11.1` | MIT |
| `hyper-rustls` | `0.27.9` | Apache-2.0 OR ISC OR MIT |
| `hyper-util` | `0.1.20` | MIT |
| `iana-time-zone` | `0.1.65` | MIT OR Apache-2.0 |
| `iana-time-zone-haiku` | `0.1.2` | MIT OR Apache-2.0 |
| `ico` | `0.5.0` | MIT |
| `icu_collections` | `2.3.0` | Unicode-3.0 |
| `icu_locale_core` | `2.3.0` | Unicode-3.0 |
| `icu_normalizer` | `2.3.0` | Unicode-3.0 |
| `icu_normalizer_data` | `2.3.0` | Unicode-3.0 |
| `icu_properties` | `2.3.0` | Unicode-3.0 |
| `icu_properties_data` | `2.3.0` | Unicode-3.0 |
| `icu_provider` | `2.3.1` | Unicode-3.0 |
| `ident_case` | `1.0.1` | MIT/Apache-2.0 |
| `idna` | `1.1.0` | MIT OR Apache-2.0 |
| `idna_adapter` | `1.2.2` | Apache-2.0 OR MIT |
| `if-addrs` | `0.13.4` | MIT OR BSD-3-Clause |
| `image` | `0.25.10` | MIT OR Apache-2.0 |
| `indexmap` | `1.9.3` | Apache-2.0 OR MIT |
| `indexmap` | `2.14.1` | Apache-2.0 OR MIT |
| `infer` | `0.19.0` | MIT |
| `inout` | `0.1.4` | MIT OR Apache-2.0 |
| `ipnet` | `2.12.1` | MIT OR Apache-2.0 |
| `is_terminal_polyfill` | `1.70.2` | MIT OR Apache-2.0 |
| `itertools` | `0.13.0` | MIT OR Apache-2.0 |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 |
| `javascriptcore-rs` | `1.1.2` | MIT |
| `javascriptcore-rs-sys` | `1.1.1` | MIT |
| `jiff` | `0.2.35` | Unlicense OR MIT |
| `jiff-core` | `0.1.0` | Unlicense OR MIT |
| `jiff-static` | `0.2.35` | Unlicense OR MIT |
| `jiff-tzdb` | `0.1.8` | Unlicense OR MIT |
| `jiff-tzdb-platform` | `0.1.3` | Unlicense OR MIT |
| `jni` | `0.21.1` | MIT/Apache-2.0 |
| `jni-sys` | `0.3.1` | MIT OR Apache-2.0 |
| `jni-sys` | `0.4.1` | MIT OR Apache-2.0 |
| `jni-sys-macros` | `0.4.1` | MIT OR Apache-2.0 |
| `js-sys` | `0.3.104` | MIT OR Apache-2.0 |
| `json-patch` | `3.0.1` | MIT/Apache-2.0 |
| `jsonptr` | `0.6.3` | MIT OR Apache-2.0 |
| `keyboard-types` | `0.7.0` | MIT OR Apache-2.0 |
| `lazy_static` | `1.5.0` | MIT OR Apache-2.0 |
| `libc` | `0.2.189` | MIT OR Apache-2.0 |
| `libloading` | `0.8.9` | ISC |
| `libredox` | `0.1.21` | MIT |
| `linux-raw-sys` | `0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `litemap` | `0.8.3` | Unicode-3.0 |
| `lock_api` | `0.4.14` | MIT OR Apache-2.0 |
| `log` | `0.4.34` | MIT OR Apache-2.0 |
| `lru-slab` | `0.1.2` | MIT OR Apache-2.0 OR Zlib |
| `mach2` | `0.4.3` | BSD-2-Clause OR MIT OR Apache-2.0 |
| `markup5ever` | `0.38.0` | MIT OR Apache-2.0 |
| `matchers` | `0.2.0` | MIT |
| `mdns-sd` | `0.13.11` | Apache-2.0 OR MIT |
| `memchr` | `2.8.3` | Unlicense OR MIT |
| `memoffset` | `0.9.1` | MIT |
| `mime` | `0.3.17` | MIT OR Apache-2.0 |
| `minimal-lexical` | `0.2.1` | MIT/Apache-2.0 |
| `miniz_oxide` | `0.8.9` | MIT OR Zlib OR Apache-2.0 |
| `miniz_oxide` | `0.9.1` | MIT OR Zlib OR Apache-2.0 |
| `mio` | `1.2.2` | MIT |
| `moxcms` | `0.8.1` | BSD-3-Clause OR Apache-2.0 |
| `muda` | `0.19.3` | Apache-2.0 OR MIT |
| `ndk` | `0.9.0` | MIT OR Apache-2.0 |
| `ndk-context` | `0.1.1` | MIT OR Apache-2.0 |
| `ndk-sys` | `0.6.0+11769913` | MIT OR Apache-2.0 |
| `new_debug_unreachable` | `1.0.6` | MIT |
| `nix` | `0.29.0` | MIT |
| `nom` | `7.1.3` | MIT |
| `nu-ansi-term` | `0.50.3` | MIT |
| `num_cpus` | `1.17.0` | MIT OR Apache-2.0 |
| `num_enum` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 |
| `num_enum_derive` | `0.7.6` | BSD-3-Clause OR MIT OR Apache-2.0 |
| `num-conv` | `0.2.2` | MIT OR Apache-2.0 |
| `num-derive` | `0.4.2` | MIT OR Apache-2.0 |
| `num-traits` | `0.2.19` | MIT OR Apache-2.0 |
| `nvenc` | `0.1.0` | MIT |
| `objc2` | `0.6.4` | MIT |
| `objc2-app-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-audio-toolbox` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-av-foundation` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-cloud-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-audio` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-audio-types` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-data` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-foundation` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-graphics` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-image` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-location` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-media` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-text` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-core-video` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-encode` | `4.1.0` | MIT |
| `objc2-exception-helper` | `0.1.1` | Zlib OR Apache-2.0 OR MIT |
| `objc2-foundation` | `0.3.2` | MIT |
| `objc2-io-surface` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-metal` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-quartz-core` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-screen-capture-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-ui-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-uniform-type-identifiers` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-user-notifications` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `objc2-web-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT |
| `once_cell` | `1.21.4` | MIT OR Apache-2.0 |
| `once_cell_polyfill` | `1.70.2` | MIT OR Apache-2.0 |
| `opaque-debug` | `0.3.1` | MIT OR Apache-2.0 |
| `openssl` | `0.10.81` | Apache-2.0 |
| `openssl-macros` | `0.1.1` | MIT/Apache-2.0 |
| `openssl-src` | `300.6.1+3.6.3` | MIT/Apache-2.0 |
| `openssl-sys` | `0.9.117` | MIT |
| `option-ext` | `0.2.0` | MPL-2.0 |
| `pango` | `0.18.3` | MIT |
| `pango-sys` | `0.18.0` | MIT |
| `parking_lot` | `0.12.5` | MIT OR Apache-2.0 |
| `parking_lot_core` | `0.9.12` | MIT OR Apache-2.0 |
| `percent-encoding` | `2.3.2` | MIT OR Apache-2.0 |
| `phf` | `0.13.1` | MIT |
| `phf_codegen` | `0.13.1` | MIT |
| `phf_generator` | `0.13.1` | MIT |
| `phf_macros` | `0.13.1` | MIT |
| `phf_shared` | `0.13.1` | MIT |
| `pin-project-lite` | `0.2.17` | Apache-2.0 OR MIT |
| `pkg-config` | `0.3.34` | MIT OR Apache-2.0 |
| `plist` | `1.10.0` | MIT |
| `png` | `0.17.16` | MIT OR Apache-2.0 |
| `png` | `0.18.1` | MIT OR Apache-2.0 |
| `polyval` | `0.6.2` | Apache-2.0 OR MIT |
| `portable-atomic` | `1.15.0` | Apache-2.0 OR MIT |
| `portable-atomic-util` | `0.2.7` | Apache-2.0 OR MIT |
| `potential_utf` | `0.1.6` | Unicode-3.0 |
| `powerfmt` | `0.2.0` | MIT OR Apache-2.0 |
| `ppv-lite86` | `0.2.21` | MIT OR Apache-2.0 |
| `precomputed-hash` | `0.1.1` | MIT |
| `proc-macro-crate` | `1.3.1` | MIT OR Apache-2.0 |
| `proc-macro-crate` | `2.0.2` | MIT OR Apache-2.0 |
| `proc-macro-crate` | `3.5.0` | MIT OR Apache-2.0 |
| `proc-macro-error` | `1.0.4` | MIT OR Apache-2.0 |
| `proc-macro-error-attr` | `1.0.4` | MIT OR Apache-2.0 |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 |
| `pxfm` | `0.1.30` | BSD-3-Clause OR Apache-2.0 |
| `quick-xml` | `0.41.0` | MIT |
| `quinn` | `0.11.11` | MIT OR Apache-2.0 |
| `quinn-proto` | `0.11.17` | MIT OR Apache-2.0 |
| `quinn-udp` | `0.5.15` | MIT OR Apache-2.0 |
| `quote` | `1.0.47` | MIT OR Apache-2.0 |
| `r-efi` | `5.3.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `r-efi` | `6.0.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `radium` | `0.7.0` | MIT |
| `rand` | `0.10.2` | MIT OR Apache-2.0 |
| `rand` | `0.8.8` | MIT OR Apache-2.0 |
| `rand` | `0.9.5` | MIT OR Apache-2.0 |
| `rand_chacha` | `0.3.1` | MIT OR Apache-2.0 |
| `rand_chacha` | `0.9.0` | MIT OR Apache-2.0 |
| `rand_core` | `0.10.1` | MIT OR Apache-2.0 |
| `rand_core` | `0.6.4` | MIT OR Apache-2.0 |
| `rand_core` | `0.9.5` | MIT OR Apache-2.0 |
| `rand_pcg` | `0.10.2` | MIT OR Apache-2.0 |
| `raw-window-handle` | `0.6.2` | MIT OR Apache-2.0 OR Zlib |
| `redox_syscall` | `0.5.18` | MIT |
| `redox_users` | `0.5.2` | MIT |
| `ref-cast` | `1.0.27` | MIT OR Apache-2.0 |
| `ref-cast-impl` | `1.0.27` | MIT OR Apache-2.0 |
| `regex` | `1.13.1` | MIT OR Apache-2.0 |
| `regex-automata` | `0.4.18` | MIT OR Apache-2.0 |
| `regex-syntax` | `0.8.11` | MIT OR Apache-2.0 |
| `reqwest` | `0.12.28` | MIT OR Apache-2.0 |
| `reqwest` | `0.13.4` | MIT OR Apache-2.0 |
| `ring` | `0.17.14` | Apache-2.0 AND ISC |
| `rustc_version` | `0.4.1` | MIT OR Apache-2.0 |
| `rustc-hash` | `2.1.3` | Apache-2.0 OR MIT |
| `rustix` | `1.1.4` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustls` | `0.23.43` | Apache-2.0 OR ISC OR MIT |
| `rustls-pki-types` | `1.15.1` | MIT OR Apache-2.0 |
| `rustls-webpki` | `0.103.15` | ISC |
| `rustversion` | `1.0.23` | MIT OR Apache-2.0 |
| `ryu` | `1.0.23` | Apache-2.0 OR BSL-1.0 |
| `same-file` | `1.0.6` | Unlicense/MIT |
| `schemars` | `0.8.22` | MIT |
| `schemars` | `0.9.0` | MIT |
| `schemars` | `1.2.2` | MIT |
| `schemars_derive` | `0.8.22` | MIT |
| `scopeguard` | `1.2.0` | MIT OR Apache-2.0 |
| `selectors` | `0.36.1` | MPL-2.0 |
| `semver` | `1.0.28` | MIT OR Apache-2.0 |
| `serde` | `1.0.229` | MIT OR Apache-2.0 |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 |
| `serde_derive_internals` | `0.29.1` | MIT OR Apache-2.0 |
| `serde_json` | `1.0.151` | MIT OR Apache-2.0 |
| `serde_repr` | `0.1.21` | MIT OR Apache-2.0 |
| `serde_spanned` | `0.6.9` | MIT OR Apache-2.0 |
| `serde_spanned` | `1.1.1` | MIT OR Apache-2.0 |
| `serde_urlencoded` | `0.7.1` | MIT/Apache-2.0 |
| `serde_with` | `3.22.0` | MIT OR Apache-2.0 |
| `serde_with_macros` | `3.22.0` | MIT OR Apache-2.0 |
| `serde-untagged` | `0.1.9` | MIT OR Apache-2.0 |
| `serialize-to-javascript` | `0.1.2` | MIT OR Apache-2.0 |
| `serialize-to-javascript-impl` | `0.1.2` | MIT OR Apache-2.0 |
| `servo_arc` | `0.4.3` | MIT OR Apache-2.0 |
| `sha2` | `0.10.9` | MIT OR Apache-2.0 |
| `sharded-slab` | `0.1.7` | MIT |
| `shlex` | `1.3.0` | MIT OR Apache-2.0 |
| `shlex` | `2.0.1` | MIT OR Apache-2.0 |
| `signal-hook-registry` | `1.4.8` | MIT OR Apache-2.0 |
| `simd-adler32` | `0.3.10` | MIT |
| `siphasher` | `1.0.3` | MIT/Apache-2.0 |
| `slab` | `0.4.12` | MIT |
| `smallvec` | `1.15.2` | MIT OR Apache-2.0 |
| `socket2` | `0.5.10` | MIT OR Apache-2.0 |
| `socket2` | `0.6.5` | MIT OR Apache-2.0 |
| `softbuffer` | `0.4.8` | MIT OR Apache-2.0 |
| `soup3` | `0.5.0` | MIT |
| `soup3-sys` | `0.5.0` | MIT |
| `spin` | `0.9.9` | MIT |
| `stable_deref_trait` | `1.2.1` | MIT OR Apache-2.0 |
| `string_cache` | `0.9.0` | MIT OR Apache-2.0 |
| `string_cache_codegen` | `0.6.1` | MIT OR Apache-2.0 |
| `strsim` | `0.11.1` | MIT |
| `subtle` | `2.6.1` | BSD-3-Clause |
| `swift-rs` | `1.0.8` | MIT OR Apache-2.0 |
| `syn` | `1.0.109` | MIT OR Apache-2.0 |
| `syn` | `2.0.119` | MIT OR Apache-2.0 |
| `syn` | `3.0.4` | MIT OR Apache-2.0 |
| `sync_wrapper` | `1.0.2` | Apache-2.0 |
| `synstructure` | `0.13.2` | MIT |
| `system-deps` | `6.2.2` | MIT OR Apache-2.0 |
| `tao` | `0.35.3` | Apache-2.0 |
| `tao-macros` | `0.1.4` | MIT OR Apache-2.0 |
| `tap` | `1.0.1` | MIT |
| `target-lexicon` | `0.12.16` | Apache-2.0 WITH LLVM-exception |
| `tauri` | `2.11.5` | Apache-2.0 OR MIT |
| `tauri-build` | `2.6.3` | Apache-2.0 OR MIT |
| `tauri-codegen` | `2.6.3` | Apache-2.0 OR MIT |
| `tauri-macros` | `2.6.3` | Apache-2.0 OR MIT |
| `tauri-runtime` | `2.11.3` | Apache-2.0 OR MIT |
| `tauri-runtime-wry` | `2.11.4` | Apache-2.0 OR MIT |
| `tauri-utils` | `2.9.3` | Apache-2.0 OR MIT |
| `tauri-winres` | `0.3.6` | MIT |
| `tempfile` | `3.27.0` | MIT OR Apache-2.0 |
| `tendril` | `0.5.1` | MIT OR Apache-2.0 |
| `thiserror` | `1.0.69` | MIT OR Apache-2.0 |
| `thiserror` | `2.0.20` | MIT OR Apache-2.0 |
| `thiserror-impl` | `1.0.69` | MIT OR Apache-2.0 |
| `thiserror-impl` | `2.0.20` | MIT OR Apache-2.0 |
| `thread_local` | `1.1.10` | MIT OR Apache-2.0 |
| `time` | `0.3.55` | MIT OR Apache-2.0 |
| `time-core` | `0.1.9` | MIT OR Apache-2.0 |
| `time-macros` | `0.2.32` | MIT OR Apache-2.0 |
| `tinystr` | `0.8.4` | Unicode-3.0 |
| `tinyvec` | `1.12.0` | Zlib OR Apache-2.0 OR MIT |
| `tinyvec_macros` | `0.1.1` | MIT OR Apache-2.0 OR Zlib |
| `tokio` | `1.53.1` | MIT |
| `tokio-macros` | `2.7.2` | MIT |
| `tokio-rustls` | `0.26.4` | MIT OR Apache-2.0 |
| `tokio-util` | `0.7.19` | MIT |
| `toml` | `0.8.2` | MIT OR Apache-2.0 |
| `toml` | `0.9.12+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml` | `1.1.4+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml_datetime` | `0.6.3` | MIT OR Apache-2.0 |
| `toml_datetime` | `0.7.5+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml_datetime` | `1.1.1+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml_edit` | `0.19.15` | MIT OR Apache-2.0 |
| `toml_edit` | `0.20.2` | MIT OR Apache-2.0 |
| `toml_edit` | `0.25.13+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml_parser` | `1.1.3+spec-1.1.0` | MIT OR Apache-2.0 |
| `toml_writer` | `1.1.2+spec-1.1.0` | MIT OR Apache-2.0 |
| `tower` | `0.5.3` | MIT |
| `tower-http` | `0.6.11` | MIT |
| `tower-layer` | `0.3.3` | MIT |
| `tower-service` | `0.3.3` | MIT |
| `tracing` | `0.1.44` | MIT |
| `tracing-attributes` | `0.1.31` | MIT |
| `tracing-core` | `0.1.36` | MIT |
| `tracing-log` | `0.2.0` | MIT |
| `tracing-subscriber` | `0.3.23` | MIT |
| `try-lock` | `0.2.5` | MIT |
| `typeid` | `1.0.3` | MIT OR Apache-2.0 |
| `typenum` | `1.20.1` | MIT OR Apache-2.0 |
| `unic-char-property` | `0.9.0` | MIT/Apache-2.0 |
| `unic-char-range` | `0.9.0` | MIT/Apache-2.0 |
| `unic-common` | `0.9.0` | MIT/Apache-2.0 |
| `unic-ucd-ident` | `0.9.0` | MIT/Apache-2.0 |
| `unic-ucd-version` | `0.9.0` | MIT/Apache-2.0 |
| `unicode-ident` | `1.0.24` | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `unicode-segmentation` | `1.13.3` | MIT OR Apache-2.0 |
| `universal-hash` | `0.5.1` | MIT OR Apache-2.0 |
| `untrusted` | `0.9.0` | ISC |
| `url` | `2.5.8` | MIT OR Apache-2.0 |
| `urlpattern` | `0.3.0` | MIT |
| `utf8_iter` | `1.0.4` | Apache-2.0 OR MIT |
| `utf8parse` | `0.2.2` | Apache-2.0 OR MIT |
| `uuid` | `1.26.0` | Apache-2.0 OR MIT |
| `valuable` | `0.1.1` | MIT |
| `vcpkg` | `0.2.15` | MIT/Apache-2.0 |
| `version_check` | `0.9.5` | MIT/Apache-2.0 |
| `version-compare` | `0.2.1` | MIT |
| `vswhom` | `0.1.0` | MIT |
| `vswhom-sys` | `0.1.3` | MIT |
| `walkdir` | `2.5.0` | Unlicense/MIT |
| `want` | `0.3.1` | MIT |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `wasip2` | `1.0.4+wasi-0.2.12` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `wasm-bindgen` | `0.2.127` | MIT OR Apache-2.0 |
| `wasm-bindgen-futures` | `0.4.77` | MIT OR Apache-2.0 |
| `wasm-bindgen-macro` | `0.2.127` | MIT OR Apache-2.0 |
| `wasm-bindgen-macro-support` | `0.2.127` | MIT OR Apache-2.0 |
| `wasm-bindgen-shared` | `0.2.127` | MIT OR Apache-2.0 |
| `wasm-streams` | `0.5.0` | MIT OR Apache-2.0 |
| `wayland-backend` | `0.3.17` | MIT |
| `wayland-client` | `0.31.15` | MIT |
| `wayland-protocols` | `0.32.13` | MIT |
| `wayland-protocols-wlr` | `0.3.12` | MIT |
| `wayland-scanner` | `0.31.11` | MIT |
| `wayland-sys` | `0.31.11` | MIT |
| `web_atoms` | `0.2.6` | MIT OR Apache-2.0 |
| `web-sys` | `0.3.104` | MIT OR Apache-2.0 |
| `web-time` | `1.1.0` | MIT OR Apache-2.0 |
| `webkit2gtk` | `2.0.2` | MIT |
| `webkit2gtk-sys` | `2.0.2` | MIT |
| `webpki-roots` | `1.0.9` | CDLA-Permissive-2.0 |
| `webview2-com` | `0.38.2` | MIT |
| `webview2-com-macros` | `0.8.1` | MIT |
| `webview2-com-sys` | `0.38.2` | MIT |
| `winapi` | `0.3.9` | MIT/Apache-2.0 |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | MIT/Apache-2.0 |
| `winapi-util` | `0.1.11` | Unlicense OR MIT |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | MIT/Apache-2.0 |
| `window-vibrancy` | `0.6.0` | Apache-2.0 OR MIT |
| `windows` | `0.54.0` | MIT OR Apache-2.0 |
| `windows` | `0.61.3` | MIT OR Apache-2.0 |
| `windows` | `0.62.2` | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_i686_gnu` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_i686_gnu` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_i686_msvc` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_i686_msvc` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | `0.52.6` | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | `0.42.2` | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | `0.52.6` | MIT OR Apache-2.0 |
| `windows-collections` | `0.2.0` | MIT OR Apache-2.0 |
| `windows-collections` | `0.3.2` | MIT OR Apache-2.0 |
| `windows-core` | `0.54.0` | MIT OR Apache-2.0 |
| `windows-core` | `0.61.2` | MIT OR Apache-2.0 |
| `windows-core` | `0.62.2` | MIT OR Apache-2.0 |
| `windows-future` | `0.2.1` | MIT OR Apache-2.0 |
| `windows-future` | `0.3.2` | MIT OR Apache-2.0 |
| `windows-implement` | `0.60.2` | MIT OR Apache-2.0 |
| `windows-interface` | `0.59.3` | MIT OR Apache-2.0 |
| `windows-link` | `0.1.3` | MIT OR Apache-2.0 |
| `windows-link` | `0.2.1` | MIT OR Apache-2.0 |
| `windows-numerics` | `0.2.0` | MIT OR Apache-2.0 |
| `windows-numerics` | `0.3.1` | MIT OR Apache-2.0 |
| `windows-result` | `0.1.2` | MIT OR Apache-2.0 |
| `windows-result` | `0.3.4` | MIT OR Apache-2.0 |
| `windows-result` | `0.4.1` | MIT OR Apache-2.0 |
| `windows-strings` | `0.4.2` | MIT OR Apache-2.0 |
| `windows-strings` | `0.5.1` | MIT OR Apache-2.0 |
| `windows-sys` | `0.45.0` | MIT OR Apache-2.0 |
| `windows-sys` | `0.52.0` | MIT OR Apache-2.0 |
| `windows-sys` | `0.59.0` | MIT OR Apache-2.0 |
| `windows-sys` | `0.61.2` | MIT OR Apache-2.0 |
| `windows-targets` | `0.42.2` | MIT OR Apache-2.0 |
| `windows-targets` | `0.52.6` | MIT OR Apache-2.0 |
| `windows-threading` | `0.1.0` | MIT OR Apache-2.0 |
| `windows-threading` | `0.2.1` | MIT OR Apache-2.0 |
| `windows-version` | `0.1.7` | MIT OR Apache-2.0 |
| `winnow` | `0.5.40` | MIT |
| `winnow` | `0.7.15` | MIT |
| `winnow` | `1.0.4` | MIT |
| `winreg` | `0.55.0` | MIT |
| `wit-bindgen` | `0.57.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `writeable` | `0.6.4` | Unicode-3.0 |
| `wry` | `0.55.1` | Apache-2.0 OR MIT |
| `wyz` | `0.5.1` | MIT |
| `yoke` | `0.8.3` | Unicode-3.0 |
| `yoke-derive` | `0.8.2` | Unicode-3.0 |
| `zerocopy` | `0.8.56` | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerocopy-derive` | `0.8.56` | BSD-2-Clause OR Apache-2.0 OR MIT |
| `zerofrom` | `0.1.8` | Unicode-3.0 |
| `zerofrom-derive` | `0.1.7` | Unicode-3.0 |
| `zeroize` | `1.9.0` | Apache-2.0 OR MIT |
| `zerotrie` | `0.2.5` | Unicode-3.0 |
| `zerovec` | `0.11.8` | Unicode-3.0 |
| `zerovec-derive` | `0.11.6` | Unicode-3.0 |
| `zlib-rs` | `0.6.7` | Zlib |
| `zmij` | `1.0.23` | MIT |
| `zune-core` | `0.5.3` | MIT OR Apache-2.0 OR Zlib |
| `zune-jpeg` | `0.5.15` | MIT OR Apache-2.0 OR Zlib |

## Additional prototype dependency versions

| Package | Version | Declared license |
| --- | --- | --- |
