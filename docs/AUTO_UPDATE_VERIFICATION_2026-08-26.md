# 자동업데이트 기능 구현 검증 (2026-08-26)

**대상 커밋:** `3c18d91` (main, 워킹트리 변경 다수 포함)
**결론: 자동업데이트는 구현되어 있지 않다.** 프론트엔드에 타입 선언만 존재하고, 그 타입을 구현·호출하는 코드가 없다. Tauri updater 플러그인, 설정, 서명 키, 릴리스 매니페스트 모두 부재.

## 요소별 판정

| 요소 | 상태 | 증거 |
| --- | --- | --- |
| Rust updater 플러그인 의존성 | 미비 | `src-tauri/Cargo.toml` 의 tauri 관련 의존성은 `tauri`, `tauri-plugin-notification`, `tauri-plugin-dialog` 뿐. `grep -c plugin-updater` → `src-tauri/Cargo.lock` 0건 |
| JS updater 플러그인 의존성 | 미비 | `ui/package.json` 에 `@tauri-apps/plugin-updater` 없음. `bun.lock`·`package.json` 0건 |
| `tauri.conf.json` updater 설정 | 미비 | `plugins` 키 자체가 없음 (`'plugins' in conf` → `False`). 따라서 `plugins.updater.endpoints`, `pubkey`, `createUpdaterArtifacts` 모두 없음 |
| 플러그인 등록 | 미비 | `src-tauri/src/lib.rs:261-262` — `.plugin(tauri_plugin_notification::init())`, `.plugin(tauri_plugin_dialog::init())` 두 개만 등록 |
| capability 권한 | 미비 | `src-tauri/capabilities/default.json` permissions: `core:default`, `core:window:allow-start-dragging`, `dialog:allow-open`, notification 4종. `updater:*` 없음 |
| 업데이트 서명 키 | 미비 | `~/.tauri` 디렉터리 없음. `.github/workflows/*` 에 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 0건 |
| 릴리스 매니페스트 / 서명 아티팩트 | 미비 | `.github/workflows/release.yml` 은 dmg/exe/msi/msix/AppImage/deb 수집 + `SHA256SUMS.txt` 생성 후 `softprops/action-gh-release` 로 업로드. `latest.json` 생성 없음, `.sig` 파일 없음. `find` 목록에 `*.app.tar.gz` 패턴은 있으나 `createUpdaterArtifacts` 가 꺼져 있어 생성되지 않음 |
| 프론트엔드 UI 진입점 | 미비 | `ui/src` 전체에서 `check for update` / `quitAndInstall` 호출부 0건. `SettingsDialog.tsx` 에 업데이트 섹션 없음 |

## `ui/src/lib/updater.ts` 의 실제 정체

43줄 파일이며, 이름과 달리 자동업데이트 동작을 전혀 수행하지 않는다.

- 죽은 코드: `UpdateStatus` 타입, `UpdaterApi` 인터페이스(`getStatus`/`onStatus`/`check`/`download`/`quitAndInstall`), `window.api.updater` 전역 선언 — 구현체도 호출부도 없음. `UpdaterApi` 참조는 같은 파일 내 두 줄(29, 40)뿐.
- 실제 사용되는 부분: `registerWindowCloseGuard` / `flushCloseGuards`. `ui/src/App.tsx:63,452` 에서 창 종료 시 세션 영속화(`persistSession`)를 위해 쓰이고 `ui/src/App.test.tsx:1667` 이 이를 검증한다. 즉 이 파일은 사실상 close-guard 모듈이다.

## 지금 배선해도 막히는 추가 블로커

버전 필드가 릴리스 태그와 분리되어 있다. `src-tauri/tauri.conf.json` 의 `version` 과 `src-tauri/Cargo.toml` 의 `version` 이 모두 `0.1.0` 으로 고정된 반면, `release.yml` 은 날짜 태그(`vYYYY.MM.DD[.N]`)만으로 릴리스를 만들고 태그 버전을 번들에 주입하지 않는다(MSIX 잡만 `steps.version` 으로 태그를 사용). updater 를 켜더라도 설치된 앱이 자기 버전을 항상 `0.1.0` 으로 보고하므로, 릴리스 매니페스트 버전 비교 기준이 무너진다. `docs/CROSS_PLATFORM_RELEASE_GUIDE.md:213` 도 버전 범프를 수동 체크리스트 항목으로만 두고 있다.

## 참고: "auto-update" 문구

`site/src/lib/downloads.ts:90` 의 `Install & auto-update via Microsoft Store` 는 사실이다 — Store 배포 채널이 갱신을 담당한다는 의미이며, 앱 내장 자동업데이트를 뜻하지 않는다. 랜딩 페이지의 다른 다운로드 경로(dmg/exe/AppImage/deb)는 수동 재설치만 가능하다.

## 구현에 필요한 최소 항목

1. `tauri-plugin-updater` (Rust) + `@tauri-apps/plugin-updater` (JS) 추가, `lib.rs` 에 등록.
2. `tauri.conf.json` 에 `plugins.updater` (`endpoints`, `pubkey`) 및 `bundle.createUpdaterArtifacts: true`.
3. `tauri signer generate` 로 키 생성 → 공개키는 설정에, 비밀키/암호는 GitHub Secrets(`TAURI_SIGNING_PRIVATE_KEY`, `..._PASSWORD`)로.
4. `capabilities/default.json` 에 `updater:default`.
5. `release.yml` 에서 태그 버전을 `tauri.conf.json`/`Cargo.toml` 에 주입하고, `latest.json` + `.sig` 를 릴리스 자산으로 업로드(또는 `tauri-action` 으로 대체).
6. `ui/src/lib/updater.ts` 의 선언을 실제 플러그인 호출로 구현하고, 설정 화면 또는 기동 시 확인 진입점 연결.
