# Windows SSH Build Verification — 2026-08-28

## Result

Ferryx was built successfully on the Windows x64 builder reached through the
`maho-win` SSH host. The application executable, both configured installer
bundles, and both Tauri updater signatures were produced.

The existing macOS release environment's `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` were passed directly to the remote build
process without writing them to the Windows filesystem. The final Tauri build
completed with exit code 0.

## Build environment

- Host: `desktop-1lapjmp\sook`
- Target: `x86_64-pc-windows-msvc`
- Cargo: `1.97.0`
- Rustc: `1.97.0`
- Bun: `1.4.0`
- Zig: `0.16.0`
- Tauri CLI: `2.11.4`
- Ghostty source: `6a508fd5e34c7e222c052a6d00bb3891ff3feace`
- Local source base commit: `b7d855c68547d617699350503e2183e4ad17c3fc`
- Source archive SHA-256: `0b2032ea88a12044bbaf08e5f4c0f7db7b221c8fb427686aa7e10d2927f457d0`

The source archive included the current working-tree changes, not only the base
commit. The Ghostty submodule was restored separately from a verified Git
bundle at the pinned commit.

## Artifacts

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `C:\Users\sook\ferryx-win-build\src-tauri\target\release\ferryx.exe` | 35,649,024 bytes | `9BD10298649F6AFE991B0B3CCDBD589F0C8603EB1B2A4D0A2030924D75A36085` |
| `C:\Users\sook\ferryx-win-build\src-tauri\target\release\bundle\nsis\Ferryx_0.1.0_x64-setup.exe` | 11,228,166 bytes | `3E9A09BAA1F85854B666D4FC05FE88AEC8A73A4CD71A7E891E8FCB6391E608BC` |
| `C:\Users\sook\ferryx-win-build\src-tauri\target\release\bundle\nsis\Ferryx_0.1.0_x64-setup.exe.sig` | 416 bytes | Tauri updater signature |
| `C:\Users\sook\ferryx-win-build\src-tauri\target\release\bundle\msi\Ferryx_0.1.0_x64_en-US.msi` | 14,962,688 bytes | `D20AD68ADB95E283C7D03C27D52BEB2EB5726E9291BE775992952FFB723C60E6` |
| `C:\Users\sook\ferryx-win-build\src-tauri\target\release\bundle\msi\Ferryx_0.1.0_x64_en-US.msi.sig` | 416 bytes | Tauri updater signature |

The executable's Windows version metadata reports Ferryx version `0.1.0`.

## Runtime verification

The generated `ferryx.exe` was executed directly on the Windows builder.

1. Invalid CLI input:
   - Command: `ferryx.exe browser snapshot`
   - Exit code: `2`
   - Error: `missing required option --browser-id`
2. Headless daemon:
   - Command: `ferryx.exe --daemon`
   - Observed readiness signal: `FERRYX_DAEMON_READY`
   - The daemon process was then terminated cleanly by the verification script.
3. Updater signature verification:
   - The Tauri updater public key from `src-tauri/tauri.conf.json` was decoded
     to its Minisign public-key form.
   - Both Tauri `.sig` wrappers were decoded and verified against their exact
     NSIS/MSI installer bytes with Minisign.
   - Both reported `Signature and comment signature verified`.

GUI interaction was not automated because desktop input automation is excluded
for this workspace. The user can run either installer on the Windows host for a
manual desktop UI check.

## Build issues resolved during verification

1. Upgraded the remote Bun installation from `1.3.14` to `1.4.0` so it could
   read the repository's lockfile version.
2. Used the installed project-local Tauri CLI (`bun x tauri build`) because the
   root `bun run build` script invokes a missing global `cargo-tauri` command.
3. Installed the required Zig `0.16.0` toolchain from the official Windows
   archive.
4. Restored the pinned Ghostty Git repository because a plain source archive
   does not satisfy the build's Git HEAD verification.
5. Removed macOS AppleDouble `._*` metadata files from the remote source tree;
   Tauri attempted to parse `capabilities/._default.json` as JSON before this
   cleanup.

## Signing outcome

The existing Tauri updater private key used by the macOS release build was
reused for Windows as required. No replacement key was generated. The key and
password existed in the local process environment, were transmitted only as
in-memory environment values for the SSH-launched PowerShell process, and were
removed from that process after the build command.

This signs the Tauri updater artifacts. It is separate from Windows
Authenticode signing of the executable or installers.
