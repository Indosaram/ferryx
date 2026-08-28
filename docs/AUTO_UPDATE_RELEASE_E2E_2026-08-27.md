# Ferryx 인앱 자동 업데이트 공개 릴리스 E2E 증적 (2026-08-27)

## 판정

공개 릴리스 파이프라인과 Tauri updater 배포 계약은 검증 완료했다. 이후 실제 `/Applications` 설치본의
`.10 → .11` 인앱 업데이트, 재시작, multi-project catalog 복구, terminal daemon 생존까지 확인했으므로
공개 updater E2E는 완료됐다. 아래 `.9` 기록은 최초 public-release 증적이며, 최신 실제 설치 결과는 문서
마지막의 `.11 실제 설치 E2E`를 따른다.

## 대상 릴리스

| 항목 | 값 |
| --- | --- |
| 공개 릴리스 | [`v2026.08.26.9`](https://github.com/Indosaram/ferryx/releases/tag/v2026.08.26.9) |
| 태그 대상 커밋 | `30cb5e753c27c3ebc505f71fbc51ccfd1468d8fd` (`fix(updater): publish Tauri v2 platform bundles`) |
| GitHub Actions 실행 | [`33037737545`](https://github.com/Indosaram/ferryx/actions/runs/33037737545) |
| 워크플로우 결과 | `success` |
| 공개 시각 | `2026-08-27T04:08:47Z` |
| updater 버전 | `2026.826.9` |

모든 릴리스 잡이 성공했다.

- `Build (macos)` — signed/notarized Tauri bundle 및 updater archive layout 검증 성공
- `Build (linux)` — AppImage updater artifact 생성 성공
- `Build (windows)` — NSIS updater installer 및 signature 생성 성공
- `Build MSIX (Windows Store)` — Store/sideload MSIX 생성 성공
- `Publish GitHub Release` — `latest.json`과 플랫폼 artifact 공개 성공

## 공개 updater manifest 계약

공개 `latest.json`을 내려받아 모든 advertised target을 검사했다. 네 target 모두 공개 artifact URL과 일치하고, manifest에 인라인된 signature는 같은 이름의 공개 `.sig` sidecar와 정확히 일치했다.

| Target | 공개 artifact | Signature sidecar | Signature 길이 |
| --- | --- | --- | ---: |
| `darwin-aarch64` | `Ferryx.app.tar.gz` | `Ferryx.app.tar.gz.sig` | 404 |
| `darwin-x86_64` | `Ferryx.app.tar.gz` | `Ferryx.app.tar.gz.sig` | 404 |
| `windows-x86_64` | `Ferryx_2026.826.9_x64-setup.exe` | `Ferryx_2026.826.9_x64-setup.exe.sig` | 424 |
| `linux-x86_64` | `Ferryx_2026.826.9_amd64.AppImage` | `Ferryx_2026.826.9_amd64.AppImage.sig` | 424 |

검사한 SHA-256:

```text
c5898ba20a798177cda0b93172f2f6ba6f312bf70364c3bbd4380bdd9ee08472  Ferryx.app.tar.gz
7a8e4aef25cd31c213954eac47761d82bf4299bb1a0d8f940e0e8a79343a7be9  Ferryx_2026.826.9_x64-setup.exe
8302777ad4aaf174c5816bd0e352b74c253d0371043254860de8058677cfd17e  Ferryx_2026.826.9_amd64.AppImage
```

`Ferryx_x64-setup.exe` 및 `Ferryx_amd64.AppImage`는 각각 versioned updater artifact와 동일한 공개 alias이며 같은 SHA-256을 가진다.

## macOS updater archive 독립 검증

공개 `Ferryx.app.tar.gz`를 내려받아 추출한 뒤 다음 검사를 실행했다.

| 검사 | 결과 |
| --- | --- |
| `node scripts/assert-updater-archive-layout.mjs` | 성공, archive entry 37개 |
| `codesign --verify --deep --strict Ferryx.app` | 성공 |
| `xcrun stapler validate Ferryx.app` | 성공 |
| `spctl --assess --type execute -vv Ferryx.app` | 성공 — `Notarized Developer ID` |

Gatekeeper origin:

```text
Developer ID Application: Indo Yoon (5DUM8WPB4C)
```

## 수정된 배포 회귀

공개 E2E 중 발견해 수정·검증한 릴리스 회귀는 다음과 같다.

1. Windows에서 Ghostty import library와 static archive가 중복 링크되던 문제를 제거했다.
2. 날짜 기반 version(`2026.826.9`)이 WiX MSI의 major-version 제한을 넘는 문제를 해결하기 위해 updater release의 Windows bundle을 NSIS로 제한했다. 별도 MSIX job은 유지한다.
3. Tauri v2가 실제로 생성하는 `-setup.exe`와 `.AppImage` updater artifact를 `latest.json` 생성기가 인식하지 못하던 문제를 수정했다. 이 때문에 macOS만 광고하던 manifest가 네 updater target을 모두 광고하게 됐다.

## .11 실제 설치 E2E 및 catalog recovery 검증 (2026-08-28)

### 실제 발견과 후속 hotfix

공개 `.9 → .10` 설치 E2E는 다운로드, 설치, 재시작, daemon 생존에는 성공했지만 Finder/`/Applications`
기동에서 WebView localStorage catalog가 하나뿐이고 process startup root가 그 project와 다르면 native
`session_state.json`의 나머지 workspace를 Sidebar에 복구하지 못했다.

`v2026.08.26.11`은 공개 `.10`을 부모로 한 독립 3-file hotfix다.

| 변경 | 내용 |
| --- | --- |
| `ui/src/App.tsx` | 저장된 단일 project가 multi-project native session에 포함되면 startup root와 달라도 catalog를 복구한다. |
| `ui/src/App.test.tsx` | 저장 project와 Finder/`/Applications` startup project가 다른 실제 회귀를 red로 재현한 뒤 green으로 고정했다. |
| `docs/AUTO_UPDATE_RELAUNCH_RECOVERY_2026-08-27.md` | 실제 실패, 원인, 후속 복구 기준을 기록한다. |

격리된 `.10 + hotfix` source의 검증 결과:

```text
bun test src/App.test.tsx src/lib/updater.test.ts src/components/SettingsDialog.update.test.tsx
Test Files 3 passed (3)
Tests 90 passed (90)

bun run build
tsc && vite build
```

### 공개 `.11` artifact 감사

| 항목 | 결과 |
| --- | --- |
| 공개 릴리스 | [`v2026.08.26.11`](https://github.com/Indosaram/ferryx/releases/tag/v2026.08.26.11) |
| 태그 대상 | `5040a5bca034494ec00ba3298d2f6afa5be71478` |
| GitHub Actions | [`33133248892`](https://github.com/Indosaram/ferryx/actions/runs/33133248892) — 전체 성공 |
| stable manifest | `releases/latest/download/latest.json`이 `2026.826.11` 제공 |
| manifest/sidecar | macOS, Windows NSIS, Linux의 manifest signature가 각 공개 `.sig`와 일치 |
| cryptographic verification | 모든 updater payload가 configured updater public key로 Blake2b/Ed25519 검증 통과 |
| checksum | 세 updater payload가 공개 `SHA256SUMS.txt`와 일치 |
| macOS trust | updater archive layout, `codesign --verify --deep --strict`, stapler, Gatekeeper 통과 |

### 승인된 실제 desktop `.10 → .11` E2E

데스크톱 제어 승인 후 `/Applications/Ferryx.app`의 설치된 `2026.826.10`을 실제 Settings 표면으로
`2026.826.11`로 업데이트했다.

1. Settings가 `Version 2026.826.11 is available.`와 활성화된 **Install and Relaunch**를 표시했다.
2. 다운로드 progress가 UI에 표시되고 두 update control이 비활성화됐다.
3. 기존 `.10` GUI PID가 종료됐다.
4. `/Applications/Ferryx.app` bundle이 `2026.826.11`로 교체되고 새 `.11` GUI PID가 기동됐다.
5. 재시작 후 Settings가 `Current version 2026.826.11` 및 `Ferryx is up to date.`를 표시했다.
6. 이전에 하나만 보이던 Sidebar가 native session의 `maho-workspace`, `superwiki-mail-otp`, `orca-lite`를
   원래 project와 함께 복구했다.
7. 실제 terminal pane 세 개가 재시작 뒤에도 표시됐다. 업데이트 전부터 실행 중인 headless daemon PID도
   업데이트 전후 동일하게 생존했고, native session에는 5개 workspace와 20개 terminal session이 유지됐다.

판정: **공개 signed payload의 실제 설치, relaunch, 최신 version 판정, multi-project/worktree catalog
recovery, terminal/agent daemon survival을 모두 확인했다.**
