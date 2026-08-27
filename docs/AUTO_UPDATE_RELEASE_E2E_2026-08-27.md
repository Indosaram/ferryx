# Ferryx 인앱 자동 업데이트 공개 릴리스 E2E 증적 (2026-08-27)

## 판정

공개 릴리스 파이프라인과 Tauri updater 배포 계약은 검증 완료했다. 남은 유일한 게이트는 **이전 버전으로 설치된 실제 macOS 앱에서 사용자가 `Install and Relaunch`를 실행하는 수동 E2E**다. 이 문서는 그 수동 결과를 아직 성공으로 주장하지 않는다.

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

## 남은 수동 E2E

데스크톱 조작은 자동화하지 않는다. 사용자가 **`2026.826.9`보다 오래된 설치형 Ferryx 앱**에서 아래를 수행해야 한다.

1. **Settings → General → Software Update**를 연다.
2. 현재 설치 version과 실행 중인 agent가 업데이트/relaunch 중에도 백그라운드에서 유지된다는 설명을 확인한다.
3. **Check for Updates**를 눌러 `2026.826.9` 발견을 확인한다.
4. **Install and Relaunch**를 누른다.
5. 재시작 후 업데이트 화면이 `2026.826.9`를 보고하는지, 기존 agent/session이 유지되는지 확인한다.

결과(성공/실패 및 보이는 오류)를 이 문서의 후속 증적으로 기록해야 한다.
