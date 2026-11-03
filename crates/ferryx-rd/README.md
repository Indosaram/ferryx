# ferryx-rd

Remote desktop engine absorbed from MahoRD (MIT, Copyright (c) 2026 Indosaram and
MahoRD contributors). Crates: protocol (`ferryx-rd-proto`), transport
(`ferryx-rd-net`), FFmpeg decoding (`ferryx-rd-decode`), audio and cursor
presentation (`ferryx-rd-render`), screen capture, encoding and input host
(`ferryx-rd-host`), client session (`ferryx-rd-app`), the iOS/Android client
bridge (`ferryx-rd-mobile`) and the legacy relay (`ferryx-rd-relay`).

Build against FFmpeg 9.0.2 made by `scripts/build-ffmpeg-lgpl.sh`:

```bash
bash scripts/build-ffmpeg-lgpl.sh "$PWD/.ffmpeg" "$PWD/.ffmpeg-build"
PKG_CONFIG_PATH="$PWD/.ffmpeg/lib/pkgconfig" LD_LIBRARY_PATH="$PWD/.ffmpeg/lib" \
  cargo test --manifest-path crates/ferryx-rd/Cargo.toml
```
