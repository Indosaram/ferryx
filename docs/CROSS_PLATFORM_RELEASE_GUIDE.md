# Ferryx Cross-Platform Release and Distribution Guide

Ferryx releases are produced only by the operator-controlled local multi-host workflow. Pushing
a tag or manually dispatching GitHub Actions must not build, sign, or publish release artifacts.
Use the [canonical local release runbook](releases/LOCAL_RELEASE_RUNBOOK.md) for exact commands,
approval gates, recovery, and the current implementation-validation hold. Pull-request checks and
Pages deployment remain in GitHub Actions.

## Platforms and channels

| Platform | Build host | Release artifact | Distribution |
| --- | --- | --- | --- |
| macOS arm64 + x86_64 | `macbook` | `Ferryx_universal.app.tar.gz` updater and `Ferryx_universal.dmg` | GitHub Releases/direct updater |
| Linux x86_64 | `omaki` | `Ferryx_amd64.AppImage` updater and `Ferryx_amd64.deb` | GitHub Releases/direct download |
| Windows x64 | `maho-win` | `Ferryx_x64.msix` | Microsoft Store package; Store submission is manual |
| Windows x64 migration | `maho-win` | `Ferryx_x64-setup.exe` NSIS updater | GitHub Releases only when `--nsis-migration` is selected |

Windows Store is the primary Windows channel. During the current legacy transition, the runbook
recommends explicitly preparing with `--nsis-migration`; a Store-only `latest.json` has no
`windows-x86_64` updater entry and cannot serve installed NSIS clients. MSI is not part of the
local release contract.

All artifacts in one release must come from the same recorded source SHA and version mapping:

```text
v2026.09.08.1 -> app/updater 2026.908.1 -> MSIX 2026.908.1.0
```

The coordinator verifies signed updater payloads against the public key committed in
`src-tauri/tauri.conf.json`, emits stable aliases, `latest.json`, and `SHA256SUMS.txt`, and uses
an explicitly approved draft/publish flow. Preserve that updater key and the existing macOS
Developer ID identity across the local-only migration.

## Microsoft Store identity

Keep these Partner Center values aligned with `src-tauri/windows/msix/AppxManifest.xml` and the
validated MSIX package:

- Package Identity Name: `ProjectMaho.Ferryx`
- Package Family Name: `ProjectMaho.Ferryx_s4dtschhe0d3e`
- Publisher: `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`
- Publisher Display Name: `Project Maho`

The Windows builder packages the exact release executable into an unsigned Store-ingestion MSIX
using the canonical quad version. Upload and submission in Microsoft Partner Center are manual,
separately approved operations; a GitHub asset is not a Store submission.

## User-facing release assets

The canonical stable aliases are:

- `Ferryx_universal.dmg`
- `Ferryx_amd64.AppImage`
- `Ferryx_amd64.deb`
- `Ferryx_x64.msix`
- `Ferryx_x64-setup.exe` only for an explicit NSIS migration release
- `latest.json` and `SHA256SUMS.txt`

The website download behavior is implemented in `site/src/lib/downloads.ts`. Validate website
links separately from artifact production. Do not restore an Actions tag-trigger recipe to this
guide; source policy forbids hosted release producers.
