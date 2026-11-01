#!/usr/bin/env bash
# Build a redistributable (LGPL-2.1-or-later) FFmpeg 9.0.2 for the Ferryx remote desktop
# engine (crates/ferryx-rd).
# No --enable-gpl, --enable-nonfree or libx264. Linux gets VAAPI, NVENC and the
# OpenH264 software fallback; macOS gets VideoToolbox; Windows gets NVENC, AMF, D3D11VA and
# Media Foundation (h264_mf / hevc_mf) and never OpenH264. Shared libraries only, so
# the LGPL relinking requirement is met by dynamic linking.
#
# Windows: run from an MSYS2 shell (packages: make nasm pkgconf diffutils) whose PATH inherits a
# Visual Studio x64 developer environment (vcvars64.bat, MSYS2_PATH_TYPE=inherit); the build
# uses --toolchain=msvc, so the DLLs and import libraries match Rust's x86_64-pc-windows-msvc.
#
# Environment:
#   FERRYX_FFMPEG_JOBS      make -j value (default 4)
#   FERRYX_FFMPEG_ARCH      macOS only: arm64 or x86_64; cross-compiles the other slice of a
#                           universal app (default: the build machine's arch)
#   FERRYX_FFMPEG_OPENH264  Linux only: 1 (default) links libopenh264, 0 leaves it out. Release
#                           payloads (scripts/stage-ffmpeg.mjs) use 0: libopenh264 would become a
#                           hard load-time dependency, and Cisco's patent licence only covers the
#                           binary Cisco itself distributes. stage-ffmpeg.mjs refuses any
#                           receipt with --enable-libopenh264.
set -euo pipefail

if [[ $# != 2 || $1 != /* || $2 != /* ]]; then
  printf 'Usage: bash %s NEW_ABSOLUTE_PREFIX NEW_ABSOLUTE_BUILD_DIRECTORY\n' "$0" >&2
  exit 2
fi
prefix=$1
work=$2
if [[ -e $prefix || -e $work || $prefix == "$work" ]]; then
  printf 'Prefix and build directory must be new, separate paths.\n' >&2
  exit 2
fi
mkdir -p -- "$prefix" "$work"
cd "$work"

ffmpeg_version=9.0.2
ffmpeg_sha256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
nv_headers=n12.1.14.0
nv_sha256=2fefaa227d2a3b4170797796425a59d1dd2ed5fd231db9b4244468ba327acd0b
amf_headers=v1.5.3
amf_sha256=0f521ad98041f8001147cded4a660127f18b1c27a801a5cd691bda1fa48381be

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
fetch() {
  curl --fail --location --max-time 300 "$1" -o "$2"
  if [[ $(sha256 "$2") != "$3" ]]; then
    printf '%s: checksum mismatch\n' "$2" >&2
    exit 1
  fi
}

fetch "https://ffmpeg.org/releases/ffmpeg-$ffmpeg_version.tar.xz" ffmpeg.tar.xz "$ffmpeg_sha256"
tar -xf ffmpeg.tar.xz

flags=(
  --prefix="$prefix" --disable-doc --disable-debug --disable-ffplay
  --disable-network --disable-autodetect --enable-shared --disable-static
)
target_arch=$(uname -m)
case "$(uname -s)" in
  Linux)
    fetch "https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/refs/tags/$nv_headers" nv.tar.gz "$nv_sha256"
    tar -xf nv.tar.gz
    make -C "nv-codec-headers-${nv_headers}" PREFIX="$work/codec-headers" install
    export PKG_CONFIG_PATH="$work/codec-headers/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    flags+=(--enable-vaapi --enable-ffnvcodec --enable-nvenc)
    case "${FERRYX_FFMPEG_OPENH264:-1}" in
      1) flags+=(--enable-libopenh264) ;;
      0) ;;
      *) printf 'FERRYX_FFMPEG_OPENH264 must be 0 or 1\n' >&2; exit 2 ;;
    esac
    ;;
  Darwin)
    # Matches bundle.macOS.minimumSystemVersion in src-tauri/tauri.conf.json.
    mac_flags="-mmacosx-version-min=10.15"
    case "${FERRYX_FFMPEG_ARCH:-$target_arch}" in
      arm64 | aarch64) mac_arch=arm64 ;;
      x86_64) mac_arch=x86_64 ;;
      *) printf 'FERRYX_FFMPEG_ARCH must be arm64 or x86_64\n' >&2; exit 2 ;;
    esac
    if [[ $mac_arch != "$target_arch" ]]; then
      flags+=(--enable-cross-compile --target-os=darwin --arch="$mac_arch")
    fi
    target_arch=$mac_arch
    flags+=(
      --enable-videotoolbox --enable-audiotoolbox --cc=clang --install-name-dir=@rpath
      --extra-cflags="-arch $mac_arch $mac_flags" --extra-ldflags="-arch $mac_arch $mac_flags"
    )
    ;;
  MINGW* | MSYS*)
    command -v cl.exe >/dev/null || { printf 'cl.exe not on PATH; start from a vcvars64 environment with MSYS2_PATH_TYPE=inherit\n' >&2; exit 2; }
    # cl.exe needs Windows paths, so the headers go under mixed (C:/...) prefixes.
    fetch "https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/refs/tags/$nv_headers" nv.tar.gz "$nv_sha256"
    tar -xf nv.tar.gz
    make -C "nv-codec-headers-${nv_headers}" PREFIX="$(cygpath -m "$work/codec-headers")" install
    export PKG_CONFIG_PATH="$work/codec-headers/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    fetch "https://github.com/GPUOpen-LibrariesAndSDKs/AMF/releases/download/$amf_headers/AMF-headers-$amf_headers.tar.gz" amf.tar.gz "$amf_sha256"
    tar -xf amf.tar.gz
    target_arch=x86_64
    flags+=(
      --toolchain=msvc --target-os=win64 --arch=x86_64 --enable-w32threads
      --enable-ffnvcodec --enable-nvenc --enable-amf --enable-d3d11va --enable-mediafoundation
      --extra-cflags="-I$(cygpath -m "$work/amf-headers-$amf_headers")"
    )
    ;;
  *)
    printf 'Unsupported build OS: %s\n' "$(uname -s)" >&2
    exit 2
    ;;
esac
case "$target_arch" in
  x86_64 | amd64) flags+=(--enable-x86asm) ;;
esac

cd "ffmpeg-$ffmpeg_version"
./configure "${flags[@]}"
make -j"${FERRYX_FFMPEG_JOBS:-4}"
make install
case "$(uname -s)" in
  MINGW* | MSYS*)
    # MSVC installs the import libraries next to the DLLs; FFMPEG_DIR consumers look in lib/.
    cp "$prefix"/bin/*.lib "$prefix/lib/"
    ;;
esac

# Read from config.h rather than running ffmpeg, so a cross-compiled slice is checked too.
buildconf=$(sed -n 's/^#define FFMPEG_CONFIGURATION "\(.*\)"$/\1/p' config.h)
license=$(sed -n 's/^#define FFMPEG_LICENSE "\(.*\)"$/\1/p' config.h)
if [[ -z $buildconf || $license != LGPL* ]]; then
  printf 'Refusing: could not read an LGPL configuration from config.h (license "%s").\n' "$license" >&2
  exit 1
fi
if [[ $buildconf == *--enable-gpl* || $buildconf == *--enable-nonfree* || $buildconf == *--enable-libx264* || $buildconf == *--enable-libx265* ]]; then
  printf 'Refusing: build is not LGPL-only.\n' >&2
  exit 1
fi
if [[ $(uname -s) == MINGW* || $(uname -s) == MSYS* ]] && [[ $buildconf == *--enable-libopenh264* ]]; then
  printf 'Refusing: Windows builds must not include OpenH264.\n' >&2
  exit 1
fi
# License texts travel with the libraries (scripts/stage-ffmpeg.mjs bundles them).
mkdir -p "$prefix/share/ferryx-legal"
cp COPYING.LGPLv2.1 COPYING.LGPLv3 LICENSE.md "$prefix/share/ferryx-legal/"
{
  printf 'FFmpeg %s (sha256 %s)\n' "$ffmpeg_version" "$ffmpeg_sha256"
  printf '%s\n' "$buildconf"
} > "$prefix/build-receipt.txt"
printf 'FFMPEG_LGPL_BUILD_PASS %s\n' "$prefix"
