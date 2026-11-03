# Third-party notices

This file covers third-party components that Ferryx ships as separate files or absorbed source
trees. Rust and JavaScript dependencies compiled into the app are listed in the CycloneDX SBOMs
attached to every release (see `docs/releases/LOCAL_RELEASE_RUNBOOK.md`, "Release provenance").

## FFmpeg (remote desktop builds only)

Releases built with the `remote-desktop` cargo feature include the FFmpeg 9 libraries
(libavutil, libavcodec, libavformat, libavfilter, libavdevice, libswscale, libswresample) as
unmodified **shared libraries**. Releases built without the feature do not contain FFmpeg.

- License: GNU Lesser General Public License 2.1 or later on every platform. All three are built
  by `scripts/build-ffmpeg-lgpl.sh` from the same source tarball. No build enables
  `--enable-gpl`, `--enable-nonfree` or `--enable-libopenh264`; `scripts/stage-ffmpeg.mjs` refuses
  any that does.
- Where they are: `Ferryx.app/Contents/Frameworks/` (macOS), `usr/lib/Ferryx/ffmpeg/` (deb and
  AppImage), and next to `ferryx.exe` (Windows NSIS and MSIX). The license texts, the exact
  `configure` line (`build-receipt.txt`) and the source location (`SOURCE.txt`) are installed
  next to them in `ffmpeg-legal/`.
- Corresponding source, also attached to each remote desktop release:
  `https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz`, sha256
  `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e` (pinned in
  `src-tauri/resources/ffmpeg/pin.json`), built by `scripts/build-ffmpeg-lgpl.sh` in the Ferryx
  source tree. The Windows build also compiles against two header-only packages, both pinned by
  sha256 in that script and not linked as libraries: nv-codec-headers `n12.1.14.0` (MIT) and
  AMD AMF headers `v1.5.3` (MIT).
- Relinking: Ferryx loads FFmpeg dynamically. You may replace the bundled libraries with your
  own build of the same major versions: put them in the same directory under the same file
  names. On macOS, replacing them breaks the app's code signature; re-sign the app ad hoc
  (`codesign --force --deep --sign - Ferryx.app`) to run it. Ferryx's own source is available at
  https://github.com/Indosaram/ferryx, so you can also rebuild the app against a modified FFmpeg.
- Written offer: for at least three years after each release, the Ferryx maintainers will
  provide the corresponding FFmpeg source on request, for no more than the cost of
  distribution. Ask through https://github.com/Indosaram/ferryx/issues.

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.

## OpenH264

No Ferryx release contains OpenH264 on any platform. Release payloads build FFmpeg without
`--enable-libopenh264` (Linux sets `FERRYX_FFMPEG_OPENH264=0`, Windows never enables it, macOS
uses VideoToolbox), and `scripts/stage-ffmpeg.mjs` refuses any receipt that enables it.
`scripts/build-ffmpeg-lgpl.sh` still enables it by default for local Linux developer builds, which
are not redistributed; there the remote desktop host can use it as a software fallback.

OpenH264 is BSD-2-Clause licensed, but Cisco pays the MPEG LA H.264 patent royalties only for the
binary that Cisco itself distributes (http://www.openh264.org/BINARY_LICENSE.txt), which is why
Ferryx does not redistribute it.

## ferryx-rd (MahoRD)

`crates/ferryx-rd` is absorbed from MahoRD under the MIT license:

```text
MIT License

Copyright (c) 2026 Indosaram and MahoRD contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The same text is in `crates/ferryx-rd/LICENSE`.
