# Ferryx 자동 업데이트 구현 (2026-08-26)

`docs/AUTO_UPDATE_VERIFICATION_2026-08-26.md`가 기록한 "구현되어 있지 않음" 상태를 실제 동작하는
인앱 자동 업데이트로 대체했다. 사용자는 Settings > General의 Software Update 항목에서 현재 버전을
확인하고, 업데이트를 조회하고, 진행률을 보며 내려받고, 설치 후 재시작할 수 있다. 배포 측에서는
날짜 버전 태그를 푸시하면 릴리스 파이프라인이 번들을 서명하고 `latest.json`과 함께 GitHub Release에
업로드한다.

## 구성 요소

| 요소 | 파일 |
| --- | --- |
| Rust 플러그인 등록 | `src-tauri/src/lib.rs` (`tauri_plugin_updater::Builder`, `tauri_plugin_process`) |
| 플러그인 의존성 | `src-tauri/Cargo.toml` (데스크톱 타깃 전용 블록) |
| 업데이터 설정 | `src-tauri/tauri.conf.json` (`plugins.updater.endpoints`, `pubkey`, `windows.installMode`) |
| 업데이터 아티팩트 생성 | `src-tauri/tauri.conf.json` (`bundle.createUpdaterArtifacts: true`) |
| 웹뷰 권한 | `src-tauri/capabilities/default.json` (`updater:default`) |
| 태그 기반 버전 동기화 | `scripts/sync-version.mjs` |
| TS 브리지 | `ui/src/lib/updater.ts` |
| 설정 UI | `ui/src/components/SettingsDialog.tsx` (`SoftwareUpdateCard`) |
| 서명 환경 계약 | `.github/workflows/release.yml` (`TAURI_SIGNING_PRIVATE_KEY`, `..._PASSWORD`) |
| 매니페스트 생성 | `scripts/build-latest-json.mjs` |
| 계약 테스트 | `src-tauri/tests/updater_config_contract.rs`, `ui/src/lib/updater.test.ts`, `ui/src/components/SettingsDialog.update.test.tsx`, `scripts/*.test.mjs` |

## 동작 흐름

1. 사용자가 Settings > General에서 Check for Updates를 누르면 `checkForUpdate()`가
   `plugins.updater.endpoints`에 등록된
   `https://github.com/Indosaram/ferryx/releases/latest/download/latest.json`을 조회한다.
2. 매니페스트의 `version`이 설치된 앱 버전보다 새로우면 상태가 `available`로 바뀌고 버전과 릴리스
   노트가 표시된다.
3. 사용자가 강조된 Install and Relaunch를 누르면 플러그인이 해당 플랫폼 아티팩트를 내려받으며
   진행률을 0~1로 보고한다. 내려받은 바이트는 `tauri.conf.json`에 박힌 공개키로 minisign 서명
   검증을 통과해야 한다.
4. 검증과 설치에 성공하면 `tauri-plugin-process`의 `relaunch()`가 앱을 자동으로 재시작한다.
   Software Update에는 Check for Updates와 Install and Relaunch 두 동작만 표시한다.
5. 서명 검증 자체가 실제로 동작함은 `docs/evidence/auto-update/c6-signature-verification.md`가
   증명한다. 정상 서명은 `VERIFY=OK`, 위조 아티팩트는 `VERIFY=FAIL`이며, 로컬 HTTP 엔드포인트에서
   매니페스트를 받아 아티팩트를 내려받고 검증하는 전체 경로도 `ENDPOINT_CONTRACT=PASS`로 통과했다.

## 검증

모든 항목은 `docs/evidence/auto-update/`의 캡처에 근거한다.

| 명령 | 결과 |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --test updater_config_contract` | `test result: ok. 5 passed; 0 failed` (c1-rust-config.md) |
| `cargo test --manifest-path src-tauri/Cargo.toml --test updater_endpoint_contract` | 실제 `tauri-plugin-updater`의 check/download/서명 검증 3/3 통과 (c6-signature-verification.md) |
| `node --test scripts/sync-version.test.mjs` | `# pass 13 / # fail 0` (c2-version-sync.md) |
| `bunx vitest run src/lib/updater.test.ts` | `Tests 13 passed (13)` (c3-ts-ui.md) |
| `bunx vitest run src/components/SettingsDialog.update.test.tsx` | `Tests 5 passed (5)` (c3-ts-ui.md) |
| `node --test scripts/build-latest-json.test.mjs` | `# pass 6 / # fail 0` (c4-ci-release.md) |
| `node --test scripts/release-workflow.test.mjs` | `# pass 6 / # fail 0` (c4-ci-release.md) |
| `python3 script/qa/ci-step-order-gate.py` | `PASS: every building job has zig + submodule init before its build step` (c4-ci-release.md) |
| `bun run --cwd ui test` | `Test Files 87 passed (87)`, `Tests 753 passed (753)`, exit 0 (c5-verification.md) |
| `bun run --cwd ui build` | `✓ built in 4.58s`, exit 0 (c5-verification.md) |
| `cargo build --manifest-path src-tauri/Cargo.toml` | `Finished dev profile`, exit 0 (c5-verification.md) |
| `bunx @tauri-apps/cli build --target aarch64-apple-darwin` | 실제 `Ferryx.app.tar.gz`와 404-byte `.sig` 생성, exit 0 (c6-signature-verification.md) |
| `cargo test --manifest-path src-tauri/Cargo.toml` | `test result: ok. 254 passed; 0 failed` for library tests and updater contracts; full command exit 101 only from the 3 baseline `daemon_persistence_contract` failures (c5-verification.md) |

변경 전 기준선은 `docs/evidence/auto-update/baseline-ui-test.txt`(80 파일 / 657 테스트, exit 0)와
`baseline-cargo-test.txt`에 있다. UI 테스트는 87 파일 / 753 테스트로 늘었고 실패는 없다.

`cargo test` 전체 실행은 exit 101이다. 하지만 최종 현재 작업트리 실행에서 library 254개와 updater
계약 테스트는 전부 통과했다. 남은 실패는 기준선에도 있던 `daemon_persistence_contract` 3건뿐이다.
중간 실행에서 보였던 uncommitted `remote::tests::test_grid_render_*` 2건은 최종 실행에서는 통과하므로
남은 릴리스 게이트가 아니다(c5-verification.md).

## 릴리스 담당자가 해야 할 일

서명 키가 로컬 빌드 환경에 없으면 번들은 서명되지 않고 업데이터는 그 업데이트를 거부한다. 로컬
`.env`에는 다음 두 환경 변수가 있어야 한다.

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/ferryx-updater.key)"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="…"
```

개인키(`~/.tauri/ferryx-updater.key`)와 암호는 반드시 백업해야 한다. 분실하면 이미 배포된 앱이
신뢰하는 공개키에 대응하는 서명을 더 이상 만들 수 없어, 이후 모든 자동 업데이트가 불가능해진다.
저장소에 커밋된 값은 공개키뿐이다.

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEwQzk5REI1QzI1QzY3Q0IKUldUTFoxekN0WjNKRU8zWGhqWlo2VXEzclF0RXoyRmJCY2Z4eGwvK2FGbE5LSmVwcW9RTmoyWm0K
```

릴리스는 검증된 커밋에 날짜 태그를 만든 뒤 그 태그를 원격에 푸시해 시작한다. 날짜 태그
`v2026.08.26.1`은 `scripts/sync-version.mjs`에서 `2026.826.1`로 변환되어 CI 빌드의 앱과
`latest.json`에 함께 주입된다. `.github/workflows/release.yml`은 날짜 버전 태그를 받아 서명된
updater 번들, 동반 `.sig`, 그리고 `latest.json`을 같은 GitHub Release에 게시한다. 수동 실행은
Actions UI에서 같은 날짜 태그 ref를 선택할 때만 사용한다.

## 한계

- macOS 코드사이닝: `APPLE_CERTIFICATE` 시크릿이 없으면 기존 로직대로 서명 없이 번들링된다.
  업데이터 서명(minisign)과 Apple 공증은 별개이며, 공증되지 않은 빌드는 Gatekeeper 경고를 받는다.
- MSIX / Microsoft Store 채널은 스토어가 갱신을 담당하므로 인앱 업데이터 경로를 쓰지 않는다.
- Linux `.deb`에는 업데이터 경로가 없다. `latest.json`의 `linux-x86_64`는 AppImage 기반이다.
- 인앱 업데이터는 데스크톱 셸에서만 동작한다. 원격 웹 클라이언트에서는 `isTauri()` 가드가 걸려
  플러그인을 호출하지 않으며, 현재 버전은 `unknown`으로 표시된다.
- 설치된 이전 버전 앱을 GitHub Release에 대해 클릭하여 다운로드·설치·재시작하는 최종 물리 E2E는
  데스크톱 조작 제약 때문에 수동으로 남는다. 재현 절차는 `docs/AUTO_UPDATE_MANUAL_E2E.md`에 있다.
