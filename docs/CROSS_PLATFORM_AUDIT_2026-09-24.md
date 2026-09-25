# Ferryx 크로스플랫폼 전수조사 (Cross-Platform Audit)

**감사일**: 2026-09-24
**대상 커밋**: `7a822165` (branch `main`) — 작업 트리에 다른 세션의 미커밋 변경 38건 존재
**범위**: `src-tauri/src` (Rust), `ui/src` (React/TS), `scripts/`, `.github/workflows/`, Tauri 번들 설정
**방법**: 도메인별 10개 병렬 감사 레인 + 오케스트레이터의 독립 교차 컴파일 검증
**모델**: 모든 조사에 `inferhub/cb/deepseek-v4.1-flash` 단독 사용 (지시대로)

---

## 0. 요약 (Executive summary)

**BLOCKER 0건.** macOS에서는 되는데 Windows/Linux에서 앱이 **아예 실행되지 않거나 빌드가 실패하는** 종류의 결함은 이번 조사에서 발견되지 않았습니다. 이는 2026-09-07 감사(146건 중 BLOCKER 19건) 대비 뚜렷한 개선이며, 실제로 그중 여러 건이 현재 트리에서 수정된 것을 직접 확인했습니다.

**그러나 기능 단위로는 여전히 심각합니다.** 이번 조사에서 **82개의 finding ID**를 확인했습니다 — 그중 §2에 전체 항목이 실린 것은 **81건(BROKEN 21, RISK 24, NOTE 36)**이고, `F-10-3`은 `F-06-3`과 동일 사안을 가리키는 중복 ID입니다. 그중 다수는 "기능이 조용히 죽어 있음" 유형입니다. 가장 중요한 것:

1. **터미널 입력이 macOS 전용 키 체계에 묶여 있음** — Windows/Linux에서 `mod`가 Ctrl로 매핑되어, 터미널에서 **Ctrl+W(단어 삭제)를 누르면 탭이 닫히고 세션이 날아갑니다.** Ctrl+D/K/L/R/B/F/T도 셸에 도달하지 않습니다. (F-08-1)
2. **Linux 네이티브 터미널 클립보드가 스텁** — Ctrl+V 붙여넣기가 아무것도 하지 않고, 선택 복사도 시스템 클립보드에 닿지 않습니다. (F-02-1, F-02-2)
3. **Windows/Linux에서 알림음 설정이 백엔드로 전달되지 않음** — Windows 토스트는 "System"을 골라도 항상 무음, Linux는 "Silent"를 무시합니다. (F-05-1)
4. **Windows에서 데몬의 원격 브라우저 기능이 죽어 있음** — 데몬 게이트웨이가 `browser.sock`(unix 전용)을 찾는데 GUI는 `browser.port`를 씁니다. (F-06-4)
5. **Windows에서 인증/신원 파일에 프로세스 간 락이 없음** — `#[cfg(unix)] flock`이 Windows에서 무연산이 되어, GUI·데몬·CLI가 같은 JSON을 덮어씁니다(페어링/디바이스 토큰 유실, attach 키페어 분열). (F-06-1, F-06-2)
6. **Linux 패키지가 `ui/dist`를 찾지 못함** — deb/AppImage 설치본에서 브라우저 원격 클라이언트가 플레이스홀더 페이지만 반환합니다. (F-09-1)
7. **에이전트 세션 추적이 Windows에서 완전히 죽어 있음** — `/bin/ps`, `/usr/sbin/lsof` 절대경로가 cfg 게이트 없이 호출됩니다. (F-10-1)
8. **네이티브 스냅샷이 비-macOS에서 스텁** — "요소 선택"과 `ferryx browser screenshot`이 Windows/Linux에서 실패하고, 원격 웹 클라이언트로 브라우저 탭 라이브 뷰가 불가능합니다. (F-07-1, F-07-2, F-07-3)

### 한 줄 결론

> **지금 상태로 Windows/Linux 빌드를 배포하면 앱은 뜨지만, 사용자는 터미널에서 Ctrl+W로 탭을 날리고, 붙여넣기가 안 되고, 알림음이 안 울리고, 브라우저 자동화와 에이전트 상태 추적이 조용히 죽어 있는 상태를 겪습니다.** 즉 "실행 불가"에서 "조용한 기능 상실"로 문제의 성격이 이동했습니다.

### 2026-09-07 감사 대비 델타 (직접 검증)

| 이전 BLOCKER | 현재 상태 | 근거 |
|---|---|---|
| L1 접근성 권한이 Windows/Linux에서 성공으로 거짓 보고 | **FIXED** | `permissions/mod.rs:152-154` → `false` 반환 |
| L2 `process_cwd`가 Windows에서 `None` 스텁 | **FIXED** | `ipc/windows_process_cwd.rs` 126줄, PEB 워크 실제 구현 |
| L2 switch-debug 로그가 `/tmp` 하드코딩 | **FIXED** | `ipc/debug.rs:33` → `std::env::temp_dir()` |
| L4 wgpu alpha mode 폴백이 Linux Wayland에서 패닉 | **FIXED** | `gpu_context.rs:8-16` `unwrap_or(Auto)` + 단위 테스트 |
| L6 Windows 창 최소화/최대화/닫기 버튼 부재 | **FIXED** | `titleBarStyle`/`hiddenTitle`는 Tauri 2.11.5에서 macOS 전용(`tauri-utils config.rs:2058`) → Windows는 네이티브 장식 유지 |
| L7 에이전트 확장 설치가 `HOME`만 확인 | **FIXED** | `daemon/agent_extension.rs:23-27` `USERPROFILE` 폴백 |
| L3 Linux 클립보드가 빈 값 스텁 | **STILL OPEN** | `ipc/native_terminal.rs:413`, `1786` 그대로 |
| L7 Windows 데몬에 인증 토큰 없음 | **STILL OPEN** | `daemon/server.rs:1840` `TcpListener::bind("127.0.0.1:0")` 무인증 |
| L7 핸드오버(무손실 업그레이드) Windows 미지원 | **STILL OPEN** | `daemon/handover.rs:458` `Err("Handover unsupported on Windows")` |
| L9 macOS Dock 배지에 Windows/Linux 대응 없음 | **STILL OPEN** | `notification/badge.rs:48-77` macOS만 |

**미검증 잔여**: 2026-09-07 감사 146건 중 이번에 상태를 확정하지 못한 항목이 다수 있습니다(§5 참조). 표에 없는 항목을 "여전히 열려 있다"고 단정하지 마십시오.

---

## 1. 이번 조사에서 직접 실행한 검증 (컴파일 증거)

읽기만 한 것이 아니라 **실제로 교차 컴파일을 시도**했습니다. 결과와 한계를 정확히 기록합니다.

| 시도 | 명령 | 결과 |
|---|---|---|
| Windows 프로덕션 코드 타입체크 | `cargo zigbuild --target x86_64-pc-windows-gnu` | **`ferryx` 라이브러리 Rust 코드젠 완주** — 에러 0, 경고 43. 이후 네이티브 정적 라이브러리 해석 단계에서 실패: `could not find native static library 'ghostty-vt-static'` |
| Windows (msvc) | `cargo check --target x86_64-pc-windows-msvc` | C 의존성 단계에서 차단: `ring`의 C 코드가 MSVC 헤더(`assert.h`)를 못 찾음 (macOS에 Windows SDK 없음) |
| Linux (gnu) | `cargo check --target x86_64-unknown-linux-gnu` | `gio-sys`/`gobject-sys` 빌드 스크립트에서 차단: pkg-config 크로스 컴파일 미구성 (Linux sysroot 없음) |
| Windows 테스트 코드 | `cargo zigbuild --all-targets` | 위 정적 라이브러리 실패가 lib 단계에서 발생하므로 **테스트 타깃 타입체크에 도달하지 못함** |

**해석**
- **Windows 프로덕션 Rust 코드는 타입체크를 통과합니다** — 이것은 실질적 증거입니다.
- **더 중요하게: 이 크로스 컴파일은 `#[cfg(not(unix))]` 블록까지 실제로 타입체크·lint합니다.** macOS `cargo check`는 게이트된 절반을 컴파일 전에 제거하므로 이 절반은 검증되지 않는데, Windows 타깃 빌드는 그것을 컴파일합니다. **증거:** Windows 빌드에서만 나오는 진단이 게이트된 블록 안에서 발생합니다 — `warning: unused variable: handover_socket_path --> src/daemon/server.rs:2821`인데, 그 바인딩은 형제 `#[cfg(unix)]` 블록(`:2823`)에서만 사용되므로 이 경고는 `#[cfg(not(unix))]` 블록에서만 나올 수 있습니다. 즉 §3의 F-01-2(b)(비-unix 핸드오버 경로가 `HandoverRejected` 스텁)가 컴파일 증거로도 뒷받침됩니다.
- 따라서 이번 조사에서 **Windows 게이트 코드(`#[cfg(not(unix))]`, `#[cfg(windows)]`)는 컴파일러 검증을 받았고**, 그 안에 미바인딩 식별자·누락 import 같은 잠재 컴파일 에러는 없습니다. (참고: 다른 세션이 2026-09-24에 `daemon/client.rs::connect_and_handshake`의 `#[cfg(not(unix))]` 블록에서 미바인딩 `pid`를 우려했으나, 현행 트리는 `HandshakeOk { version, pid, epoch, binary_mtime_ms, daemon_version, .. }`로 바인딩하고 있어(`:1155-1162`) 해소된 상태이며, 이번 Windows 빌드에서 해당 파일에 대한 진단이 0건임을 확인했습니다.)
- `ghostty-vt-static` 링크 실패의 원인은 명확합니다: 빌드 스크립트가 `cargo:rustc-link-lib=static=ghostty-vt-static`(build_ghostty.rs:369)을 내보내는데 산출물은 COFF `.lib`이므로, GNU ABI는 `libghostty-vt-static.a`를 찾습니다. **Windows 릴리스는 `x86_64-pc-windows-msvc`를 쓰므로 이는 검증 환경의 한계이며 제품 결함이 아닙니다.**
- **Linux는 이 머신에서 교차 검증이 불가능합니다** (Linux sysroot 필요). Linux 관련 결함은 전부 정적 분석 근거입니다.
- **테스트 코드는 이 머신에서 컴파일되지 않습니다.** 따라서 CI가 유일한 컴파일 게이트이며, 아래 CI 커버리지 결함(F-04-2, F-09-4, F-09-5, F-10-13)이 더 중요해집니다.

---

## 2. 신규/현행 확인 결함 (BLOCKER → NOTE)

### BROKEN — 해당 플랫폼에서 사용자 기능이 실패하거나 조용히 잘못 동작

#### F-08-1 [BROKEN] Windows/Linux에서 `mod`가 Ctrl로 매핑되어 터미널 제어 키를 탈취 — Ctrl+W가 탭을 닫음
- **Where:** `ui/src/lib/shortcuts.ts:517` — `const expectedControl = Boolean(binding.control || (binding.mod && !isMac));` / 충돌 바인딩: `:89` `{ key: "w", mod: true }` (tab.close), `:281` (terminal.splitRight), `:96`, `:103`, `:314`, `:324`, `:339`
- **Platforms:** macOS=정상 (mod→⌘) / Windows/Linux=`Ctrl+W`로 탭 닫힘·세션 유실, `Ctrl+D/K/L/R/B/F/T` 셸 미도달
- **Why:** `mod`는 macOS 개념인데 비-macOS에서 Ctrl로 무조건 폴백되어, 앱 전역 단축키가 셸의 제어 키 네임스페이스에 침범. `nonMac` 오버라이드(`:218-274`)는 `workspace.select1..9`에만 적용되어 있음.
- **Minimal fix:** `useShortcuts`에서 `isTerminalTarget(event.target)`일 때 순수 `Ctrl+<letter>`(shift/alt/mod 없음) 바인딩은 클레임하지 않도록 한 곳만 수정.
- **Verification:** `shortcuts.test.tsx`에 `.terminal-host` 픽스처로 `ctrlKey:true, key:"d"` 디스패치 → `terminal.splitRight` 미호출 + `defaultPrevented===false` 단언.
- **Confidence:** VERIFIED (차단 체인 + `NativeTerminalPane.tsx:293` `defaultPrevented` bail까지 확인)

#### F-02-1 [BROKEN] Linux에서 네이티브 터미널 Ctrl+V가 아무것도 붙여넣지 않음 (클립보드 명령이 `Empty` 스텁)
- **Where:** `src-tauri/src/ipc/native_terminal.rs:413` `#[cfg(not(any(target_os = "macos", target_os = "windows")))] fn read_native_pasteboard() -> (…::Empty, Vec::new())`, `:1786` 동일 스텁
- **Platforms:** macOS=NSPasteboard / Windows=Win32 clipboard / Linux=항상 `Empty`
- **Why:** 유일한 붙여넣기 경로가 `NativeTerminalPane.tsx:2578` → `performNativePasteFallback`이고, `preventDefault()`가 DOM `paste` 이벤트까지 막습니다. Linux에서는 `{"kind":"empty"}`가 반환되어 무음 실패. **테스트가 `{kind:"text"}`를 목킹해서 실제 Linux 백엔드가 절대 반환할 수 없는 값을 검증**하므로 테스트로는 안 잡힙니다.
- **Minimal fix:** Linux arm 구현(`wl-paste`/`xclip` 또는 `arboard`), `:1742` Windows 블록 미러링.
- **Confidence:** VERIFIED

#### F-02-2 [BROKEN] Linux에서 선택 복사가 시스템 클립보드에 도달하지 않음
- **Where:** `src-tauri/src/ipc/native_terminal.rs:1424` `#[cfg(not(target_os = "macos"))] { let _ = app; }`
- **Why:** JS 폴백이 **await 이후** `navigator.clipboard.writeText`를 호출하는데, macOS 브랜치가 존재하는 이유가 바로 그 방식이 WebKit에서 불안정하기 때문입니다(코드 주석 `:1155-1157`). Linux 데스크톱은 동일 계열 WebKitGTK입니다.
- **Minimal fix:** Windows(`OpenClipboard`/`SetClipboardData`)·Linux(`wl-copy`/`xclip`) 메인스레드 쓰기 구현, 또는 JS 쓰기를 user-gesture 경로로 이동.
- **Confidence:** UNVERIFIED (비-macOS 쓰기 부재는 검증; WebKitGTK가 post-await 쓰기를 거부하는지는 1회 실행 필요)

#### F-02-3 [BROKEN] Linux에서 child surface 생성 실패 시 "layer-backed 아님"이라는 잘못된 에러로 하드 실패
- **Where:** `native_terminal/platform/linux.rs:306` (Wayland/Xlib 쌍 + `wl_subcompositor` 없으면 `None`), `:347-355` `layer_backed: has_child` → `composition.rs:337-350` 검증 거부
- **Why:** `wl_subcompositor` 미광고 컴포지터, 또는 프로젝트 자체 탈출구 `FERRYX_DISABLE_WAYLAND_SUBSURFACE=1`에서 터미널이 **사용 불가**가 됩니다(부모 GTK 표면으로 폴백 없음, DOM/xterm 폴백 렌더러도 없음).
- **Minimal fix:** `child`가 `None`일 때 부모 윈도우 표면용으로 검증 통과하는 디스크립터 보고 + 에러 메시지에 실제 원인 명시.
- **Confidence:** VERIFIED (코드 경로 및 검증 실패 확인; 도달성은 환경 의존)

#### F-05-1 [BROKEN] 알림음 정책이 Windows/Linux 백엔드로 전달되지 않음 — Windows 토스트는 항상 무음
- **Where:** `notification/notify_rust_adapter.rs:34-55` (sound_name·hint 미설정), `ipc/notifications.rs:63-67` (`.sound()` 없음) vs `notification/macos_submission.rs:36-37` (`setSound`)
- **Platforms:** macOS=정상 / Windows=`notify-rust` 4.18.0 `windows.rs:50-54`가 미설정을 `sound(None)`으로 매핑 → `tauri-winrt-notification` 0.7.3이 `<audio silent="true"/>` 렌더 → **"System"을 골라도 무음** / Linux=`suppress-sound` 힌트 미설정으로 "Silent" 무시
- **Why:** 사용자 설정이 두 배포 플랫폼에서 반대 동작. 전달 프로브는 여전히 `submitted`/`test_submitted: true`를 보고하므로 실패가 드러나지 않습니다.
- **Minimal fix:** `#[cfg(target_os="windows")]`에서 `System` → `builder.sound_name("Default")`; `#[cfg(target_os="linux")]`에서 `Silent` → `Hint::SuppressSound(true)` (Linux에 `sound_name("Default")`를 보내면 안 됨).
- **Confidence:** VERIFIED (핀 고정된 크레이트 소스까지 확인)

#### F-05-2 [BROKEN] Linux에 클립보드 이미지 리더가 없고, UI는 이를 "빈 클립보드"로 오보
- **Where:** `src-tauri/src/clipboard_image.rs:201-204` `#[cfg(not(any(macos, windows)))] pub fn read_clipboard_image() -> Option<ClipboardImage> { None }`; 호출부 `NativeTerminalPane.tsx:1242`, `:1225` → `toast.error("No clipboard image could be read…")`
- **Why:** Linux에서 이미지를 복사해 붙여넣으면 에러 토스트만 뜨고 **폴백 코드도 실행되지 않습니다**(`sendImagePasteShortcut` 미호출). 모듈 doc이 주장하는 "기존 paste chord로 폴백"이 사실이 아닙니다.
- **Minimal fix:** typed `Unsupported` 반환 + 호출부에서 기존 paste chord 폴백; 근본 해결은 Linux 리더(arboard/GTK).
- **Confidence:** VERIFIED

#### F-06-1 [BROKEN] Windows에서 원격 인증 스토어에 프로세스 간 락이 없어 동시 writer가 서로의 레코드를 폐기
- **Where:** `src-tauri/src/remote/auth.rs:1212` `#[cfg(unix)] struct StoreLock`, `:1254` `#[cfg(unix)] let _lock = StoreLock::acquire(path)?`
- **Why:** 파일 자체의 doc(`:1205-1211`)이 위험을 명시: "데몬·GUI·CLI는 별개 **프로세스**이므로 in-process mutex로 정렬할 수 없다." Windows에서는 `_lock`이 무연산이 되어 read-modify-rename 경합 → 나중 rename이 이전 writer의 변경을 조용히 폐기. 대상은 디바이스 토큰·페어링 그랜트·머신 소유권입니다.
- **Minimal fix:** `remote/machine_operation_journal.rs:150`이 이미 쓰는 이식성 있는 `std::fs::File::lock()`으로 교체 (같은 도메인에 정답 패턴이 존재).
- **Confidence:** VERIFIED (cfg 갭 + 두 기존 이식성 호출부 직접 확인)

#### F-06-2 [BROKEN] Windows에서 attach 신원에 락이 없어 x25519 키페어가 분열될 수 있음
- **Where:** `src-tauri/src/remote/attach_identity.rs:82` (`AttachLock::acquire` 본문 전체가 `#[cfg(unix)]`), `:97` `Drop`도 동일
- **Why:** `load_or_generate_canonical_attach_identity()`가 데몬 시작·`ferryx account enroll|login` CLI·relay client 등 4개 진입점에서 호출됩니다. Windows에서 둘 다 `NotFound`를 관찰하면 각자 키를 생성하고, 진 쪽은 메모리에 다른 키를 들고 서명 → 릴레이/페어링 레코드와 불일치.
- **Minimal fix:** 기존 `attach-identity.tx.lock` 경로에 이식성 락 적용.
- **Confidence:** VERIFIED

#### F-06-3 [BROKEN] `ferryx account enroll|login`이 Windows에서 실패 (계정 데이터 디렉터리가 `HOME`만 조회)
- **Where:** `src-tauri/src/account/origin.rs:41` — `std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx").join("account"))`
- **Why:** 저장소 자체가 `remote/state.rs:1244`에서 "Windows는 `HOME`을 정의하지 않는다"고 명시. `None` → `ACCOUNT_ORIGIN_UNSET` → `bin/account.rs:20` 실패. 트리의 다른 모든 리졸버는 `USERPROFILE`/`LOCALAPPDATA` 폴백이 있습니다.
- **Minimal fix:** `remote/auth.rs:48-56` 미러링.
- **Confidence:** VERIFIED

#### F-06-4 [BROKEN] 데몬 게이트웨이의 브라우저 백엔드가 unix 전용 경로·IPC를 써서 Windows 원격 브라우저/디자인 모드가 죽어 있음
- **Where:** `remote/state.rs:972-976` `get_runtime_dir().join("browser.sock")` (cfg 게이트 없음) vs GUI의 실제 엔드포인트 `ipc/browser_cli.rs:1474-1481` (`browser.port`); `remote/browser_backend.rs:466-473` `#[cfg(not(unix))] send_framed_request → Unavailable`
- **Why:** 데몬은 `RemoteGatewayState` 생성 시 `set_browser_backend`를 호출하지 않으므로(호출자는 GUI인 `lib.rs:1060/1230/1234`뿐) 항상 `LocalIpcBrowserBackend` 기본값에 고정됩니다. 결과적으로 원격 웹 클라이언트에 `browserAvailable: false`가 광고되고 모든 스냅샷/조작 요청이 실패합니다.
- **Minimal fix:** `remote/state.rs:972`에서 GUI와 동일한 cfg 분기(`browser.port`) 적용, 또는 `cfg(not(unix))`에서 명시적으로 `browserAvailable: false` 반환.
- **Confidence:** VERIFIED (cfg/경로 불일치 및 데몬의 `set_browser_backend` 부재 확인)

#### F-07-1 [BROKEN] Windows/Linux에 네이티브 스냅샷이 없어 "요소 선택"이 조용히 죽고 `ferryx browser screenshot`이 항상 실패
- **Where:** `browser/snapshot_source.rs:388-408` (`is_supported → false`, `Unsupported` 에러), `browser/screenshot.rs:72-83` 동일
- **Why:** "Select element" 버튼이 무조건 렌더링되고(`BrowserToolbar.tsx:393-401`), 완료 호출이 `void finishBrowserElementPick(...)`로 거부를 삼킵니다 → 사용자는 스냅샷도 에러도 못 받고 unhandled rejection만 남습니다. `BrowserRemoteService::is_snapshot_supported()`는 트리 어디에서도 호출되지 않습니다.
- **Minimal fix:** 기존 capability를 UI에 노출해 버튼 숨김/비활성 + `.catch` 추가, 또는 WebView2 `CapturePreview`/WebKitGTK `snapshot` 구현.
- **Confidence:** VERIFIED (UI→커맨드→트레이트→스텁 전 체인)

#### F-07-2 [BROKEN] Windows/Linux 호스트에서는 원격 웹 클라이언트/폰으로 브라우저 탭 라이브 뷰가 불가능
- **Where:** `browser/snapshot_source.rs:389-391`, `remote/browser_backend.rs:2022-2043` (`supported_formats: []`, `max_edge: 0`, `max_fps: 0`)
- **Why:** 스크린캐스트 생산자가 스냅샷 소스에서 구동되므로, 비-macOS 호스트는 탭 목록/조작은 되지만 스트리밍은 절대 안 됩니다. 정직하게 열화되지만 클라이언트는 `"Waiting for screencast stream..."`에 무한 대기합니다.
- **Minimal fix:** 비-macOS 스냅샷 소스 추가, 또는 `supported_formats`가 비면 명시적 "이 호스트에서는 라이브 뷰 불가" 상태 렌더.
- **Confidence:** VERIFIED (게이팅 체인), 클라이언트 문자열은 UNVERIFIED

#### F-07-3 [BROKEN] 키프레스 자동화가 Windows에서 거부되고 Linux에서는 조용한 no-op
- **Where:** `ipc/browser.rs:1805-1810` (`windows_keypress_capability → Unsupported`), `:1825-1826` `#[cfg(target_os="windows")]`, `:344-356` Linux가 도달하는 JS 폴백
- **Why:** 합성 `KeyboardEvent`는 `isTrusted === false`이므로 기본 동작(텍스트 삽입·폼 제출·스크롤)이 발생하지 않습니다. Linux에서 `ferryx browser keypress Enter`는 **성공을 보고하면서 아무 일도 하지 않습니다**. Windows는 하드 실패. macOS는 신뢰된 `NSEvent` 주입.
- **Minimal fix:** 먼저 Linux도 동일한 capability 게이트를 적용해 `Unsupported`를 반환하게 하고, 이후 `webkit2gtk` `send_key_event` 구현.
- **Confidence:** VERIFIED (웹 플랫폼 보장 사항은 명세 근거)

#### F-09-1 [BROKEN] Linux 패키지가 `ui/dist`를 찾지 못해 브라우저 원격 클라이언트가 플레이스홀더 페이지를 제공
- **Where:** `remote/server.rs:2586-2589` 후보 목록에 `<resource_dir>/ui/dist`가 없음; `:2675-2676` 실패 시 `EMBEDDED_FALLBACK_HTML`; deb 레이아웃은 `tauri-bundler-2.9.4/src/bundle/linux/debian.rs:330`(`/usr/lib/Ferryx/`), AppImage는 `linuxdeploy.rs:115`가 재사용
- **Why:** macOS 후보 1(`Contents/Resources/ui/dist`)은 맞고 Windows(`exe_dir/ui/dist`)도 맞지만, Linux deb/AppImage는 `/usr/lib/Ferryx/`에 리소스를 두므로 어떤 후보도 매칭되지 않습니다. **`ipc/ssh.rs:494`는 같은 리소스 디렉터리를 올바르게 해석**하므로 정답 루트는 도달 가능한데 `ui/dist`에만 쓰이지 않습니다. `remote/tests.rs:430-448`의 테스트가 **어떤 플랫폼도 만들지 않는 가상 레이아웃을 단언**해 버그를 고정하고 있습니다.
- **Minimal fix:** 게이트웨이 시작 시 `app.path().resource_dir()`을 캡처해 `<resource_dir>/ui/dist` 후보 추가.
- **Verification:** Linux에서 deb 설치 후 `curl` → SPA HTML이어야 함; 또는 `remote/tests.rs:430`을 실제 deb 레이아웃으로 바꿔 실패 확인.
- **Confidence:** VERIFIED (4개 레이아웃 모두 로컬 의존성 소스에서 확인)

#### F-09-2 [BROKEN] deb 설치본에 플러그인이 절대 설치할 수 없는 인앱 업데이트가 제안됨
- **Where:** `ipc/updater.rs:33-36` `#[cfg(not(windows))] updater_managed_externally() -> false`, `:44-56` `distribution_channel()`이 Windows 두 채널만 모델링; 배포되는 Linux 업데이터 산출물은 raw AppImage뿐(`scripts/lib/release-contract.mjs:47-53`); 플러그인 `tauri-plugin-updater-2.10.1/src/updater.rs:968-974`, `:1049-1052` (`install_deb`는 진짜 `.deb` 요구)
- **Why:** 다운로드는 성공하고 설치가 `InvalidUpdaterFormat`으로 실패 → 사용자는 영구히 "Update failed"를 봅니다. deb는 `dpkg`로 갱신해야 합니다.
- **Minimal fix:** `updater_managed_externally()`가 bundle type `Deb`/`Rpm`일 때 `true` 반환(AppImage는 `false` 유지).
- **Confidence:** VERIFIED (플러그인 Deb 경로 + 배포 산출물 종류)

#### F-10-1 [BROKEN] 에이전트 세션 ID 탐색이 macOS/BSD `ps`·`lsof`를 cfg 게이트 없이 하드코딩
- **Where:** `src-tauri/src/ipc/agents.rs:92` `/bin/ps`, `:193` `/bin/ps -E`, `:213` `/usr/sbin/lsof` — 파일 전체 cfg 속성 2개뿐
- **Platforms:** macOS=3개 모두 동작 / Windows=`Command::new("/bin/ps")` 스폰 실패 → `.ok()?` → claude, codex, copilot, cursor, kimi, gjc, omo, antigravity, pi **전부 `None`** / Linux=`-E`는 BSD 문법이라 procps-ng가 거부하고 `lsof`는 보통 `/usr/bin/lsof`
- **Why:** 호출부가 `daemon/server.rs:2189`, `daemon/session_metadata_provider.rs:84`이므로 Windows에서 에이전트 재개 admission이 `AGENT_PROVIDER_UNVERIFIED`로 실패하고 에이전트/세션 상관이 조용히 열화됩니다. 모든 실패가 `.ok()?`로 삼켜지고 로그도 없습니다.
- **Minimal fix:** 절대경로를 `PATH` 해석으로 교체(파일 내 `agents.rs:590`에 PATHEXT 인식 리졸버가 이미 존재), `/bin/ps`·`lsof` 어댑터를 `#[cfg(unix)]`로 게이트, Linux는 `/proc/<pid>/environ` 사용.
- **Confidence:** VERIFIED (경로·플래그·호출부 직접 확인)

#### F-10-2 [BROKEN] 에이전트 세션 탐색이 `HOME`만 읽고 `USERPROFILE` 폴백이 없음
- **Where:** `ipc/agents.rs:341`, `:365`, `:474` — `std::env::var_os("HOME")`
- **Why:** Windows에서 이 세 분기가 전부 건너뛰어져 antigravity/opencode/pi가 조용히 `None`. 트리의 다른 홈 조회는 모두 올바른 폴백을 가집니다(`file_preview.rs:1140`, `ssh.rs:119`, `daemon/agent_extension.rs:23-27`).
- **Minimal fix:** 세 지점에 `.or_else(|| std::env::var_os("USERPROFILE"))`.
- **Confidence:** VERIFIED

#### F-04-1 [BROKEN] Windows 첫 실행이 앱의 설치 디렉터리를 프로젝트로 채택
- **Where:** `src-tauri/src/ipc/project.rs:52` `std::env::current_dir()`, `:64` `if canonical.parent().is_none() { HOME or USERPROFILE }`
- **Platforms:** macOS=`.app` 실행 시 cwd `/` → 부모 없음 → HOME 폴백(의도) / Linux=AppImage/`.desktop` 실행 시 `$HOME` 또는 `/` → 홈 / Windows=Start Menu 바로가기의 작업 디렉터리는 대상 폴더(`%LOCALAPPDATA%\Ferryx`)이고 부모가 `Some`이므로 **폴백이 발화하지 않아 설치 디렉터리가 워크스페이스로 등록**됩니다(데몬 등록 + 페어링된 원격 기기에 노출).
- **Minimal fix:** "사용 가능한 cwd 없음" 판정에 `canonical == current_exe()?.parent()` 조건 추가.
- **Confidence:** UNVERIFIED (코드 경로는 확인; Windows 바로가기 cwd는 NSIS 템플릿이 `tauri-bundler` 크레이트에 있어 이 저장소에서 읽을 수 없음)

#### F-03-1 [BROKEN] "기본 셸" 설정 목록이 모든 호스트에서 Windows 셸만 제공하고, 탭 메뉴는 호스트가 아닌 브라우저 UA로 목록을 고름
- **Where:** `ui/src/components/settings/TerminalSection.tsx:265` (`pwsh`/`powershell`/`cmd`/`wsl` 옵션, 게이트 없음), `ui/src/components/TabBar.tsx:65-70` (`navigator.platform`/`userAgent` 기반 `isWindowsPlatform()`) → `:153`/`:220`
- **Why:** 셸 환경설정이 호스트 OS에 대해 어디에서도 검증되지 않습니다. macOS/Linux에서 `pwsh`를 고르면 `resolve_shell_command_pure`의 custom 브랜치(`shell.rs:329-334`)가 그대로 사용 → `resolve_binary` 실패 → **패인이 스폰 실패**. 원격 웹 클라이언트에서는 macOS 브라우저가 Windows 호스트에 접속해도 Windows 셸 메뉴를 못 봅니다(반대도 마찬가지).
- **Minimal fix:** 백엔드가 알려주는 호스트 플랫폼(데몬은 이미 `TargetPlatform::CURRENT` 보유, `shell.rs:260`)으로 옵션 게이트; `TabBar`도 동일 값 사용.
- **Confidence:** VERIFIED

#### F-01-1 [BROKEN] Windows에서 에이전트 상태 수신 소켓이 바인드되지 않는데 모든 패인이 그 경로를 export함
- **Where:** `src-tauri/src/daemon/server.rs:1526` — `#[cfg(unix)] pub fn spawn_agent_state_listener`; 호출부 `:1879-1880` — `#[cfg(unix)] self.spawn_agent_state_listener();` 그런데 `:1882` `crate::daemon::agent_extension::install_agent_state_extension();`와 `src-tauri/src/terminal/pty.rs:199` `"FERRYX_AGENT_STATE_SOCKET"` export는 **무조건** 실행됨. 런타임 디렉터리는 `server.rs:850-852`의 `agent-state.sock`.
- **Platforms:** macOS=UDS 바인드, 확장이 연결, 허브가 `origin: Agent` 보고 수신 / Windows=`agent-state.sock`이 **생성되지 않음**(`cfg(not(unix))` 리스너가 없음). 그런데 확장은 설치되고 env var는 주입되므로 패인의 `net.createConnection(socketPath)`가 ENOENT로 실패하고, 그 실패는 설계상 삼켜짐 / Linux=macOS와 동일
- **Why:** 확장 소스(`src-tauri/resources/agent-extensions/ferryx-agent-state.ts:10-14`)에 **TCP 모드가 이미 구현되어 있음**(`FERRYX_AGENT_STATE_PORT` + `FERRYX_AGENT_STATE_TOKEN`). Rust 쪽은 원격 SSH 세션용으로만 그 모드를 켬(`src-tauri/src/ssh/agent_forward.rs`, `daemon/session_service.rs:1325-1328`). 로컬 Windows 패인에 대해 PORT/TOKEN을 설정하는 코드 경로가 없으므로, 권위 있는 확장 소스의 에이전트 상태가 Windows에서 조용히 사용 불가가 되고 프로세스/화면 추정만 남습니다(진단 메시지도 없음).
- **Minimal fix:** `#[cfg(not(unix))]`용 수신기를 추가해 루프백 `TcpListener`를 바인드하고 port+token을 데몬 서버에 보관, `pty.rs`에서 Windows일 때 `FERRYX_AGENT_STATE_SOCKET` 대신 PORT/TOKEN을 export. 확장 변경은 불필요(TCP 분기가 이미 있음).
- **Verification:** Windows에서 `ferryx --daemon` 실행 후 에이전트가 로드된 패인을 띄우고 `origin: Agent` 상태 발행을 단언. 오늘은 어떤 플랫폼에서도 `origin: Process`만 관측됨.
- **Confidence:** VERIFIED (코드 경로 직접 확인; "UI 폴백" 서술은 `spawn_foreground_observer`가 유일한 나머지 발행자라는 추론)

#### F-01-2 [BROKEN] 세션 보존 데몬 업그레이드(롤링 핸드오버)에 Windows 구현이 없고, 비-unix `--handover-from` 경로는 도달 불가
- **Where:** `src-tauri/src/daemon/server.rs:3234-3293` — `#[cfg(not(unix))] async fn handle_upgrade_binary`; `src-tauri/src/daemon/handover.rs:454-458` — `prepare_handover`가 `Err("Handover unsupported on Windows")`; `src-tauri/src/daemon/server.rs:1792-1820` — `#[cfg(not(unix))]` `legacy_peer`/`CommitHandover` 블록; `:3135-3166` — `--handover-from`를 넘기는 유일한 spawner `spawn_legacy_handover_daemon`이 `#[cfg(unix)]`
- **Platforms:** macOS=`--handover-from`으로 후속 데몬을 띄우고 SCM_RIGHTS로 PTY master를 넘겨 세션 무손실 / Windows=`live_sessions > 0`이면 `UpgradeAction::Refuse` → `UpgradeUnsupported`. `FERRYX_DAEMON_IDLE_UPGRADE=1`이어도 세션 0일 때만 재시작 / Linux=unix 전체 경로
- **Why:** (a) 사용자 가시 문제: Windows에서 앱 업데이트가 살아있는 터미널을 소유한 데몬을 교체하지 못하고, 유일한 신호는 `tracing::info!`뿐(`client.rs:1180-1184`)이라 사용자에게 보이지 않음. (b) 비-unix `handover_from` 블록은 죽은 코드 — 유일한 producer가 `#[cfg(unix)]`이므로 Windows에서 `handover_from`은 항상 `None`이고, 그 블록에 의존하는 TCP legacy-peer 라우트(`proxy.rs`, `session_metadata_forward.rs`)는 등록될 수 없음.
- **Minimal fix:** 디스크립터 전이를 이식하려 하지 말 것. `UpgradeAction::Refuse`는 유지하되 `Proceed`(세션 0) 분기에서 후속 데몬을 `crate::util::no_window_command`로 띄우고(디버그 빌드 콘솔 플래시 방지), `UpgradeUnsupported`를 "업데이트를 마치려면 Ferryx를 재시작하세요"라는 사용자 가시 상태로 표면화. 죽은 `#[cfg(not(unix))]` 블록과 비-unix `prepare_handover` 스텁은 제거.
- **Verification:** `cargo build --target x86_64-pc-windows-msvc` + `live_sessions == 0`인 비-unix `handle_upgrade_binary`가 `CREATE_NO_WINDOW` 플래그로 프로세스를 띄우는지 단언. (b)는 블록 제거 후 `cargo check`로 호출자 부재 확인.
- **Confidence:** VERIFIED (인용 라인 전부 직접 확인; "도달 불가"는 `--handover-from`의 유일한 producer가 `#[cfg(unix)]`이라는 사실에서 도출)

### RISK — 오늘은 동작하지만 취약하거나 컴파일 에러가 될 수 있는 것

| ID | 요약 | Where | Confidence |
|---|---|---|---|
| F-01-3 | Windows 데몬 신뢰 모델에 소유자/ACL 검사·전송 인증 없음 (`_expected_uid` 미사용, `TcpListener::bind("127.0.0.1:0")`) | `daemon/server.rs:1840-1842`, `:410-413` | VERIFIED |
| F-01-4 | Windows stale 데몬 종료가 PID만으로 `taskkill /F` — PID 재사용 시 무관한 프로세스 종료 가능 | `daemon/client.rs:1251-1267` | VERIFIED (레이스 미재현) |
| F-01-5 | Windows 프로토콜 불일치 복구가 살아있는 데몬의 `daemon.port` 삭제 후 락을 못 잡는 데몬을 스폰 → 경로로 도달 불가 상태 | `daemon/client.rs:1220-1232` | VERIFIED |
| F-02-4 | Linux child surface가 클라이언트 영역 원점 보정 없이 DOM 뷰포트 좌표로 배치 → 헤더 높이만큼 위로 밀림 | `platform/linux.rs:325-345` | UNVERIFIED (보정 부재는 확인) |
| F-02-5 | Linux에서 쉼표 구분 폰트 스택을 그대로 fontconfig에 전달 → 설정한 폰트가 선택되지 않음 (Windows는 파싱함) | `renderer/freetype_raster.rs:122-127` | VERIFIED |
| F-02-6 | Linux에서 글리프당 `FcConfig` 1개 누수 (`FcInitLoadConfigAndFonts`, `FcConfigDestroy` 미호출) | `renderer/freetype_raster.rs:134` | VERIFIED |
| F-02-7 | 비-macOS 셀 메트릭이 0.6/1.25 하드코딩 → 실제 advance와 불일치로 열 밀림/겹침 | `renderer/font_manager.rs:131-135` | VERIFIED |
| F-02-8 | Linux에서 `X11`/`Xext`/`wayland-client`/`freetype`/`fontconfig` 5개 라이브러리를 무조건 링크, 빌드타임 프로브 없음 (`native-terminal`이 기본 feature) | `platform/linux.rs:44,66`, `wayland_child.rs:40`, `freetype_raster.rs:70,88` | VERIFIED |
| F-03-2 | Windows 에이전트 상태 감지가 패인당 250ms마다 `powershell.exe`+`Get-CimInstance` 실행 → 4패인이면 초당 4회, tick 스킵으로 sub-Hz 열화 | `terminal/foreground.rs:189-192`, `daemon/server.rs:1610` | VERIFIED (실측 미검증) |
| F-03-3 | Linux 기본 셸 폴백이 `/bin/bash` 존재 확인 없이 사용 (Alpine/NixOS/minimal 컨테이너에서 스폰 실패). 원격 헬퍼는 이미 존재 확인함 | `terminal/shell.rs:353-360` vs `ferryx_scope/ssh/helper.rs:915-924` | VERIFIED |
| F-03-4 | Windows verbatim 경로(`\\?\C:\...`)가 `FERRYX_WORKTREE_PATH`로 패인 환경에 주입 (다른 스폰 경계는 모두 정규화함) | `terminal/pty.rs:141-155`, `:51` | VERIFIED |
| F-03-5 | 인용부호 있는 Windows 커맨드라인 파싱 실패 → 공백 경로에 설치된 에이전트 패인이 영원히 Observed가 안 됨 | `terminal/foreground.rs:51-74`, `:218` | VERIFIED |
| F-04-2 | 유일한 Windows worktree 경로 동일성 테스트(`q4_windows_paths.rs`)가 CI에서 실행되지 않음 | `.github/workflows/build-test.yml:141-149` | VERIFIED |
| F-04-3 | Windows git containment(job object)가 CI에서 컴파일만 되고 실행되지 않음 (Windows 잡에 `--lib` 없음) | `worktree/git/windows.rs`, CI `:131,137` | VERIFIED |
| F-04-4 | Linux git-child 관찰이 `pidfd_open`에만 의존, `ENOSYS`(커널 <5.3) 폴백 없음 | `worktree/git/drain.rs:112-121` | VERIFIED |
| F-05-3 | Windows 토스트 앱 신원이 저장소 어디에도 등록되지 않은 AUMID에 의존 | `notification/notify_rust_adapter.rs:36-44` | UNVERIFIED (Windows 실행 필요) |
| F-06-5 | SSH 비밀번호 인증이 `SSH_ASKPASS_REQUIRE=force`+`DISPLAY`에 의존 — Win32-OpenSSH가 이를 존중하는지 미확정 | `ssh/password.rs:166-180` | UNVERIFIED |
| F-06-6 | 계정 스토어 락도 unix 전용 | `account/store.rs:173-198` | VERIFIED |
| F-07-4 | WebView2 < 101에서 Windows "Private" 프로필이 조용히 비공개가 아님 | `ipc/browser.rs:972-976` + Tauri 문서 | UNVERIFIED |
| F-08-2 | `.drag-region`(`-webkit-app-region`)은 WebKit에서 무효인데 WebView2에서는 유효할 수 있음 → Windows에서 검증되지 않은 두 번째 클릭 억제기 | `ui/src/index.css:144-146` | UNVERIFIED (WebView2 동작) |
| F-09-3 | deb가 `libasound2`를 선언하지 않는데 rodio→cpal→alsa-sys가 하드링크하고 linuxdeploy는 번들 금지 → 최소 시스템에서 실행 실패 | `tauri.conf.json:92-96`, `Cargo.lock` alsa-sys | VERIFIED |
| F-09-4 | CI가 패키징/릴리스 스크립트와 그 테스트를 어디에서도 실행하지 않고 Windows/Linux 번들을 빌드하지 않음 | CI `:36-48`, `:131`, `:156-181` | VERIFIED |
| F-10-4 | Windows는 세션이 하나라도 살아 있으면 업그레이드 거부(`FERRYX_DAEMON_IDLE_UPGRADE=1` 필요), UI에 표면화되지 않음 | `daemon/server.rs:3235-3292`, `daemon/handover.rs:458` | VERIFIED |
| F-10-5 | `LANG=C.UTF-8`이 Windows PTY 자식에 주입되나 ConPTY 셸은 무시 (죽은 로직) | `terminal/pty.rs:36`, `:213` | VERIFIED |

### NOTE — 정보성·표면적·개발 편의

주요 항목만 나열합니다(전체 37건).

- **F-01-6** `daemon::launchd`는 **모든 플랫폼에서 죽은 코드**입니다 — `install_launchd_agent`의 프로덕션 호출자가 없고(`launchd.rs`와 테스트뿐), Windows/Linux용 자동시작(`sc.exe`/`systemd --user`/XDG autostart)도 없습니다. 데몬은 GUI가 요청 시 스폰합니다(`client.rs:1022-1030`). AGENTS.md 표는 여전히 이 모듈을 macOS 통합으로 광고합니다.
- **F-01-7** `ferryx-relay`가 `HOME`만으로 데이터 디렉터리를 결정, 없으면 **CWD 기준 상대경로**에 계정 데이터를 씁니다(`bin/relay.rs:112-119`).
- **F-01-8** Linux가 macOS 도구에 맞춰 쓰인 `cfg(unix)` 게이트 테스트 모듈을 실행합니다(`/usr/sbin/sshd`, `id`, `/bin/kill`, `DYLD_FALLBACK_LIBRARY_PATH`).
- **F-02-9/10** 죽은 코드: Xcb 윈도우 arm(tao가 Xlib만 반환), 도달 불가한 `Some(_)` surface arm, 호출자 없는 `window_backing_scale_factor`.
- **F-02-11** Windows/Linux 컬러 이모지 미구현(명시적 폴백, 터미널은 정상).
- **F-02-12** 비-macOS에서 종료된 패인의 최종 화면이 유지되지 않음(의도적, 테스트로 고정).
- **F-03-6** Ghostty 설정/테마 탐색에 Windows 후보 경로가 전혀 없음(`HOME`+`XDG_CONFIG_HOME`만).
- **F-03-7** PTY 테스트 18개가 `/bin/sh`를 하드코딩하고 cfg 게이트가 없어 Windows에서 `cargo test` 실패.
- **F-03-8** 로그인 셸 PATH 프로브가 POSIX 전용인데 Windows에서 `SHELL`이 설정돼 있으면 실행됨.
- **F-04-5** Windows 디스크 스캔이 모든 reparse point를 링크로 취급 → OneDrive 클라우드 플레이스홀더가 0바이트로 보고.
- **F-04-6** macOS 전용 번들 에디터 CLI 경로를 모든 플랫폼에서 탐색.
- **F-05-4** 읽지 않은 배지가 Windows/Linux에서 구조적 no-op.
- **F-05-5** `permissions::home_dir()`이 비-macOS에서 죽은 코드(dead_code 경고).
- **F-05-6** 권한 설정 문구가 호스트가 아닌 브라우저 OS를 기준으로 함.
- **F-07-5** Linux UA가 모든 Linux 빌드에서 `X11; Linux x86_64`로 고정(Wayland/aarch64 오표기).
- **F-07-6** Linux 오버레이가 첫 창/첫 vbox 자식에 키된 프로세스 전역 싱글턴.
- **F-08-3** "기본 셸" 목록에 zsh/bash/fish 프리셋 없음(Windows 셸만).
- **F-08-4** `installContextMenuGuard`가 호출되지 않는 죽은 모듈.
- **F-08-5** 에이전트 PID 매칭이 `.exe`/`.cmd`를 제거하지 않음(모듈 자체도 미사용).
- **F-08-6** `SHELL_ONLY_RE`가 POSIX 전용이며 양쪽 경로가 `null` 반환이라 무효.
- **F-08-7** push 폴백이 존재하지 않는 `/service-worker.js`를 등록(실제는 `/sw.js`).
- **F-08-8** 알림 권한 카드에 Linux 분기 없음.
  - **[2026-09-24 재검증: 이 finding은 오탐(false finding)으로 판정됨]** 원문 premise인 "`status.notifications.canOpenSettings`가 true인데도 Linux에서 액션 슬롯이 비어 있다"는 **발생할 수 없는 상태**를 서술합니다. 백엔드는 Linux에서 이 값을 절대 true로 만들지 않습니다: `notification/permission.rs:93` — `let can_open_settings = matches!(platform, NotificationPlatform::Windows);` → Linux는 false이고, `notification/model.rs:206-216`의 `non_authoritative(platform, can_open_settings)`를 거쳐 `permissions/mod.rs:202-209`의 비권위 분기가 `can_open_settings: raw.can_open_settings`를 그대로 반환합니다. 또한 `openPermissionsSystemSettings("notifications")`에는 Linux 대상이 없어(`opened:false`/`unsupported`) 버튼을 만들면 **죽은 버튼**이 되며, 이는 코드 자신의 주석이 명시적으로 피하려는 상태입니다. **결론: 결함이 아니라 오탐입니다.** 이에 따라 `p3-permissions-ui`가 이 premise를 따라 추가한 `status.platform === "linux" && status.notifications.canOpenSettings` 분기는 **도달 불가능한 코드**이고, 그 테스트 픽스처(`PermissionsSection.test.tsx`의 `mockStatusLinux.notifications.canOpenSettings: true`)는 백엔드가 만들 수 없는 상태를 날조해 **공허한 테스트**가 되었으므로, 분기를 제거하고 픽스처를 실제값(false)으로 교정합니다.
- **F-08-9** CLI 카드가 Windows에서 POSIX 경로를 안내하면서 바로 아래 "Unix 전용"이라 표기.
- **F-08-10** iPadOS가 `detectMacPlatform()`에 의해 macOS로 분류됨(현재 소비자 없음).
- **F-09-5** CI 매트릭스가 뒤집혀 있음 — macOS는 테스트 0개, Windows는 2개 파일, 업데이터 계약 테스트는 어디에서도 실행 안 됨.
- **F-09-6** `tauri.windows.conf.json`이 macOS 전용 키(`titleBarStyle`/`hiddenTitle`)를 설정 → 의도한 오버레이 타이틀바가 Windows에서 발생하지 않음.
- **F-09-7** 개발용 릴리스 스크립트가 macOS 경로/서명 ID/`/tmp`를 하드코딩; 6개 스크립트가 `process.getuid()`를 써서 Windows에서 TypeError.
- **F-10-6** `launchd` 모듈이 모든 플랫폼에서 컴파일되고 `launchctl` 스폰이 게이트 없음.
- **F-10-7** CLI 런처 설치에 Windows 구현 없음(정직하게 `PlatformUnsupported`).
- **F-10-8** macOS `.app` 번들 에디터 경로를 모든 플랫폼에서 평가.
- **F-10-9** Ghostty 탐색이 `HOME`만 읽고 Windows 후보 없음.
- **F-10-10** `sqlite3`/`opencode`를 PATHEXT 인식 리졸버 없이 직접 스폰.
- **F-10-11** 개발 스크립트 6개가 `/tmp/rorca-${process.getuid()}` 하드코딩(Windows에서 모듈 로드 시 TypeError).
- **F-10-12** QA 하네스가 macOS Chrome 번들 경로 하드코딩.
- **F-10-13** Windows 통합 커버리지가 약 105개 테스트 타깃 중 2개이며, launchd 계약 테스트는 Linux에서 공허하게 통과합니다(`get_launchd_plist_path()`가 `None`이라 단언 본문 미실행).

---

## 3. 이전 감사(2026-09-07)에서 여전히 열려 있는 것으로 확인된 항목

이번 레인이 현행 코드에서 재확인한 것만 기재합니다. `docs/CROSS_PLATFORM_ISSUE_AUDIT_2026-09-07.md` 및 동일 본문의 `docs/evidence/cross-platform-audit-20260908/` 참조.

| 이전 ID | 요약 | 현행 근거 |
|---|---|---|
| L1 | Ghostty 테마/설정 탐색에 Windows 후보 없음 | F-03-6, F-10-9 |
| L1 | Dock/작업표시줄 배지가 macOS 전용 | F-05-4 |
| L2 | 부모 디렉터리 fsync가 `File::open` 사용 → Windows에서 실패 | `daemon/session_service.rs:1110` `fs::File::open(parent)?.sync_all()?` (**cfg 게이트 없음, 직접 확인**) |
| L2 | 에이전트 탐색이 `/bin/ps`·`/usr/sbin/lsof` 실행 | F-10-1 |
| L3 | 네이티브 터미널 클립보드 검색이 Linux에서 빈 값 스텁 | F-02-1 |
| L3 | 선택 복사가 macOS에서만 클립보드에 씀 | F-02-2 |
| L3 | 스크롤 휠 모니터가 macOS 전용 | L02 도메인 미해결 항목 |
| L4 | 비-macOS 셀 메트릭 0.6/1.25 하드코딩 | F-02-7 |
| L4 | 컬러 이모지가 Windows/Linux에서 미구현 | F-02-11 |
| L4 | Linux FreeType가 글리프마다 fontconfig 재로드 | F-02-6 |
| L4 | FreeType/Fontconfig pkg-config 프로브 및 링커 경로 부재 | F-02-8 |
| L5 | 터미널 제어 코드와 전역 단축키 충돌 | **F-08-1** |
| L6 | CLI 런처 설치가 Windows 미지원 | F-10-7 |
| L6 | 데몬 자동시작이 Windows/Linux 미구현 | F-01-6 |
| L6 | deb 의존성에 ALSA 라이브러리 누락 | F-09-3 |
| L7 | Windows 데몬 전송에 인증 토큰 없음 | F-01-3 |
| L7 | Windows 런타임 경로 검증에 소유권/ACL 검사 없음 | F-01-3 |
| L7 | 에이전트 상태 소켓이 Windows에서 바인드되지 않음 | **F-01-1** |
| L7 | 핸드오버가 Windows에서 전면 미지원 | F-10-4 |
| L8 | Linux `SHELL` 미설정 시 `/bin/bash` 하드코딩 | F-03-3 |
| L8 | 로그인 셸 `-l` 플래그를 셸 지원 여부와 무관하게 적용 | `shell.rs:343,359` (**직접 확인**) |
| L9 | 브라우저 클립보드 접근이 Windows/Linux에서 조용히 비활성 | L07 도메인 미해결 항목 |
| L9 | `Silent` 알림음이 Windows/Linux에서 무시됨 | F-05-1 |
| L9 | switch-debug 추적 로그 `/tmp` 하드코딩 | **FIXED** (`ipc/debug.rs:33`) |
| L10 | PTY 테스트 대다수가 Windows/Linux 커버리지 없음 | F-03-7, F-10-13 |

**F-01-1 검증 상세** (이전 BLOCKER L7-AGENT-STATE-SOCKET, 직접 확인):
- `daemon/server.rs:1526` `#[cfg(unix)] pub fn spawn_agent_state_listener`
- 호출부 `:1879-1880` `#[cfg(unix)] self.spawn_agent_state_listener();`
- 그러나 `:1882` `install_agent_state_extension();`는 **무조건** 실행되고, `terminal/pty.rs:199` `"FERRYX_AGENT_STATE_SOCKET"`도 **무조건** 주입됩니다.
- 확장 소스(`agent_extension.rs:98`)에 `FERRYX_AGENT_STATE_PORT`/`TOKEN` TCP 모드가 존재하지만 로컬 Windows 패인용으로 설정되는 코드 경로가 없습니다.

---

## 4. 잘 되어 있는 부분 (Cleared)

레인들이 명시적으로 "정상"으로 판정한 영역입니다. 전면 재작성이 필요하지 않습니다.

- **데몬 전송 계층의 Windows 구현은 진짜입니다** — `TcpListener::bind("127.0.0.1:0")` + `daemon.port` 랑데부(`server.rs:1840-1852`, `client.rs:887-896`), 실제 `LockFileEx`/`UnlockFileEx` 락(`server.rs:623-690`), Windows 인식 상태/로그 디렉터리(`APPDATA`/`LOCALAPPDATA`/`XDG_DATA_HOME`), `\\?\` cwd 정규화.
- **원격 호스트 추상화가 진짜 이식 가능합니다** — `RemotePlatform::Posix|Windows`와 병렬 PowerShell/cmd 스크립트 빌더(`ssh/operations.rs`, `ssh/helper_setup.rs`, `ferryx_scope/ssh/process{,_windows}.rs`). Windows 프로세스 스폰(CIM), ACL 검증(`icacls`), 셸 선택(`COMSPEC` vs `SHELL`), verbatim/UNC 경로 정규화 모두 구현됨.
- **git 경로 정규화** — `strip_verbatim_prefix`/`normalize_path_for_git`가 `\\?\`, `//?/`, `\\?\UNC\`, `//?/UNC/`, `\\.\`를 모두 처리하고 테스트로 고정(`worktree/git.rs:57-89`, `:1203-1245`).
- **Windows git containment가 구조적으로 정확** — job object, `KILL_ON_JOB_CLOSE`, `CREATE_SUSPENDED` → `AssignProcessToJobObject` → `NtResumeProcess`, `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` 드레인 신호.
- **`ipc/file_link.rs`의 PATH/PATHEXT 해석** — `.COM;.EXE;.BAT;.CMD` 확장과 `.cmd`→`.exe` 디심, UNC 거부, 셸 없는 argv 실행.
- **`ipc/windows_process_cwd.rs`** — WOW64 인식 PEB 워크로 실제 구현(스텁 아님).
- **프론트엔드의 IME/AltGr 처리** — `isComposing`, `keyCode 229`, `key === "Process"/"Dead"`, `getModifierState("AltGraph")`, Dvorak 배제까지 처리. 트리에서 가장 플랫폼 인식적인 코드.
- **한글 재조합** — `ui/src/remote/hangulComposition.ts` (조합하지 않는 IME용 2벌식 자모 재조합).
- **저장소 키 마이그레이션** — `ferryx.*` 정식 키 + 레거시 1회 마이그레이션.
- **Linux GTK 오버레이 구현** — 고정된 gtk 0.18.2 / webkit2gtk 2.0.2 소스와 대조 검증됨; 모든 호출이 존재하고 메인스레드에서 실행됨.
- **WGPU 컨텍스트** — `Backends::all()`, surface 능력에서 포맷 선택, alpha mode 안전 폴백, `AutoVsync`.
- **알림 코어** — model/service/activation/audio가 플랫폼 중립; macOS 코드는 모듈 경계에서 `cfg` 게이트되어 빌드를 깨뜨릴 수 없음.
- **`notification/permission.rs`** — 비-macOS 제공자가 `Unknown`/비권위적 보고, 죽은 "권한 활성화" 버튼을 만들지 않음.
- **`clipboard_image.rs` Windows 브랜치** — 24/32비트, 상하향/하향 행, 0 알파→불투명, `BI_BITFIELDS` v3 마스크, 팔레트/압축/절단 입력 거부(단위 테스트됨).
- **`permissions/mod.rs`** — `request_accessibility()`가 비-macOS에서 `false`(거짓 성공 제거됨).
- **`browser/linux.rs`** — 프로세스 전역 싱글턴이지만 현재 단일 창 앱에서는 성립.
- **업데이터 매니페스트 키/페이로드 형식** — `tauri-plugin-updater` 2.10.1이 실제로 수용하는 형식과 일치(raw `.exe`/`.AppImage`).
- **MSIX 패키저(`scripts/build-msix.ps1`)** — 버전 quad, FileVersionInfo, 헬퍼 매니페스트 해시 검증, 실패 시 닫힘. CI에서 미실행일 뿐 결함은 아님.
- **`src-tauri/build.rs`** — Windows Common-Controls v6 매니페스트를 bin·테스트 타깃 모두에 임베드(`CVT1100` 회피 포함). MSVC 전용 올바른 수정.
- **`scripts/lib/release-hosts.mjs`** — darwin 로컬 / linux SSH `bash -s` / win32 SSH PowerShell `-EncodedCommand` UTF-16LE, 인용 헬퍼 모두 정확.
- **`scripts/check-tree-quiescent.sh`** — GNU `stat -c` + BSD `stat -f` 폴백.
- **`scripts/lib/release-platforms.mjs`** — `codesign`/`spctl`/`xcrun`을 비-darwin 호스트에서 명시적으로 거부.

---

## 5. 권장 수정 순서

우선순위는 **사용자 영향 × 수정 비용**으로 정했습니다.

### 1군 — 즉시 (한 줄~수 줄 수정, 사용자 체감 큼)

1. **F-08-1** `mod`→Ctrl 충돌. 한 곳 수정(`useShortcuts`의 터미널 타깃 처리)으로 Windows/Linux 터미널 입력이 정상화됩니다. **데이터 손실(탭 닫힘)이 있는 유일한 항목**이므로 최우선.
2. **F-06-1 / F-06-2 / F-06-6** `#[cfg(unix)] flock` → 이식성 있는 `std::fs::File::lock()`. 같은 도메인(`remote/machine_operation_journal.rs:150`, `remote/relay_server.rs:558`)에 정답 패턴이 이미 있으므로 cfg 제거 수준입니다. 인증·신원 손상이므로 우선.
3. **F-10-1 / F-10-2** `agents.rs` 절대경로 → `PATH` 해석 + `USERPROFILE` 폴백. 파일 내에 이미 PATHEXT 리졸버가 있습니다.
4. **F-06-3 / F-10-3** `account/origin.rs:41`에 `USERPROFILE` 폴백.
5. **F-05-1** 알림음 매핑(`sound_name("Default")` / `Hint::SuppressSound`).
6. **F-09-2** `updater_managed_externally()`에 Deb/Rpm 추가.
7. **F-09-3** deb `depends`에 `libasound2 | libasound2t64`.
8. **F-03-3** Linux 기본 셸 폴백에 존재 확인(원격 헬퍼의 3줄 미러링).

### 2군 — 단기 (기능 복구, 설계 판단 필요)

9. **F-02-1 / F-02-2 / F-05-2** Linux 클립보드 3종. `arboard` 또는 `wl-paste`/`xclip` 도입.
10. **F-09-1** `remote/server.rs`에 `<resource_dir>/ui/dist` 후보 추가 + 잘못된 테스트(`remote/tests.rs:430`) 수정.
11. **F-06-4** 데몬 게이트웨이 브라우저 백엔드 cfg 분기 또는 정직한 `browserAvailable: false`.
12. **F-07-1 / F-07-3** capability를 UI에 노출해 죽은 버튼 제거 + Linux keypress를 `Unsupported`로 정직화(가짜 성공 제거).
13. **F-01-1** Windows 에이전트 상태 수신기 — 확장에 TCP 모드가 이미 있으므로 Rust 쪽 배선만 필요.
14. **F-03-1** 셸 목록을 호스트 플랫폼 기준으로 게이트.
15. **F-02-5 / F-02-6 / F-02-7** Linux 폰트 스택 파싱·fontconfig 캐시·셀 메트릭 실측.

### 3군 — CI/프로세스 (재발 방지)

16. **F-09-4 / F-09-5 / F-10-13 / F-04-2 / F-04-3** CI에 `bun test scripts/`, Windows `--lib` 스코프 테스트, `q4_windows_paths`, 업데이터 계약 테스트 추가. **이번 조사에서 테스트 코드가 이 머신에서 컴파일조차 되지 않았으므로, CI가 유일한 컴파일 게이트입니다.**
17. **F-10-13** Linux에서 공허하게 통과하는 launchd 테스트를 `#[cfg(not(macos))] assert!(is_none())`으로 바꿔 실제 검증으로 전환.

### 4군 — 후순위 (기능 부재이거나 표면적)

18. Windows 네이티브 스냅샷(WebView2 `CapturePreview` / WebKitGTK `snapshot`) → F-07-2 원격 브라우저 라이브 뷰 복구.
19. Windows 배지(`ITaskbarList3::SetOverlayIcon`), Windows 자동시작(레지스트리 Run 키 또는 작업 스케줄러).
20. Windows 핸드오버(설계 작업), Windows 데몬 전송 인증 토큰(F-01-3).
21. 죽은 코드 정리: `daemon/launchd.rs`, `contextMenuGuard.ts`, `agentSessionDiscovery.ts`, Xcb arm, `home_dir()`.

---

## 6. 커버리지 한계 (정직한 미검증 목록)

이 보고서의 신뢰 경계를 명시합니다.

**실행 검증**
- Windows/Linux 실기기 실행은 **전혀 없습니다.** 모든 Windows/Linux 결함은 정적 분석 + 핀 고정된 의존성 소스(`~/.cargo/registry`, `tauri-2.11.5`, `tauri-rust`/`wry`, `notify-rust 4.18.0`, `tauri-plugin-updater 2.10.1`, `tauri-bundler 2.9.4`, `gtk 0.18.2`) 대조 근거입니다.
- Windows 프로덕션 Rust 코드는 교차 컴파일 타입체크를 **통과**했지만, **테스트 코드는 컴파일되지 않았고** 링크 단계는 도달하지 못했습니다.
- Linux는 이 머신에서 컴파일 자체가 불가능했습니다(Linux sysroot/pkg-config 부재).

**런타임에서만 확정 가능한 항목** (해당 플랫폼에서 1회 실행 필요)
- F-02-2 (WebKitGTK가 post-await `writeText`를 거부하는지)
- F-02-4 (GTK CSD가 웹뷰 원점을 실제로 오프셋하는지)
- F-03-2 (Windows PowerShell+CIM 관찰자의 실측 지연/CPU)
- F-04-1 (Windows 바로가기 작업 디렉터리 — NSIS 템플릿이 `tauri-bundler` 크레이트에 있어 저장소에서 읽을 수 없음)
- F-04-5 (OneDrive 플레이스홀더 reparse tag)
- F-05-3 (AUMID 등록 여부와 토스트 표시 결과)
- F-05-1의 Linux 절반 (DE가 힌트 없이 기본음을 재생하는지)
- F-06-5 (Win32-OpenSSH의 `SSH_ASKPASS_REQUIRE`/`DISPLAY` 지원)
- F-07-4 (WebView2 < 101에서 Private 프로필 동작)
- F-08-2 (WebView2의 `-webkit-app-region` 처리)
- F-02-3의 도달성 (`wl_subcompositor` 미광고 컴포지터)

**감사하지 않은 영역**
- `src-tauri/vendor/ghostty/**` (벤더링된 C/Zig)
- `src-tauri/vendor/portable-pty/**` 내부 전체(L03이 관련 부분만 읽음)
- `remote/server.rs`(3,800줄), `remote/relay_server.rs`(5,500줄) — 플랫폼 관련 영역만 정독
- `surface_host.rs`(약 8,000줄) — 플랫폼 관련 영역만 정독
- `ui/src` 약 500개 파일은 패턴 스윕으로 커버(플랫폼 조건 없음), 정독은 인용된 파일만
- 2026-09-07 감사 146건 중 §3 표에 없는 항목의 현재 상태는 **미확정**입니다.

**공유 작업 트리 주의**
- 이 감사는 읽기 전용으로 수행되었고 저장소에 대한 쓰기는 이 보고서 파일 생성뿐입니다.
- 감사 시점에 작업 트리에 **다른 세션의 미커밋 변경 38건**이 있었습니다. 특히 `remote/attach_identity.rs`, `remote/browser_backend.rs`, `daemon/client.rs`, `daemon/server.rs`는 진행 중인 변경과 겹치므로, 수정 착수 전에 `git status`/`git diff`를 다시 확인하십시오.

---

## 7. 부록 — 감사 레인 및 방법

| 레인 | 도메인 | 모델 | 소요 | 도구 호출 |
|---|---|---|---|---|
| L01 | 데몬 생명주기·소켓·자동시작 | inferhub/cb/deepseek-v4.1-flash | 7m02s | 102 |
| L02 | 네이티브 터미널 엔진 | 〃 | 8m31s | 131 |
| L03 | PTY·셸·셸 통합 | 〃 | 11m46s | 141 |
| L04 | 워크트리·git·파일시스템 | 〃 | 9m38s | 89 |
| L05 | 알림·권한·클립보드 | 〃 | 20m23s | 124 |
| L06 | 원격·릴레이·페어드·SSH·계정 | 〃 | 5m45s | 132 |
| L07 | 임베디드 브라우저 | 〃 | 7m24s | 140 |
| L08 | 프론트엔드 UI | 〃 | 8m56s | 160 |
| L09 | 빌드·패키징·릴리스·CI·업데이터 | 〃 | 16m13s | 140 |
| L10 | 리포 전역 미게이트 macOS 가정 스윕 | 〃 | 13m48s | 145 |

모든 레인은 읽기 전용(파일 편집·git 쓰기·빌드/테스트 실행 금지)으로 지시되었고, 컴파일 검증은 오케스트레이터가 단일 노드로 수행했습니다.

---


---


---


---


---


---


---


---


---

## 8. C7 처분표 (Disposition table) — 82개 finding ID 전량

**건수 정합성 (83 → 82):** 요약부 헤드라인이 처음에 "83건"으로 적었으나, 이는 제 산술 오류였습니다. 실제 고유 finding ID는 **82개**입니다 — §2에 전체 항목이 실린 것은 81건(BROKEN 21, RISK 24, NOTE 36)이고, `F-10-3`은 `F-06-3`과 동일 사안을 가리키는 중복 ID이므로 별도 항목을 더하지 않았습니다. 아래 표는 **82개 ID 전부**를 빠짐없이 처분합니다. 헤드라인은 이미 정정했습니다.

모든 finding은 코드 수정 노드에 배정되었거나 명시적으로 범위 밖으로 기록되었습니다. 상태 열은 DAG 노드 실행 상태에서 자동 생성됩니다.

| Finding | 심각도 | 담당 노드 | 상태 |
|---|---|---|---|
| F-01-1 | BROKEN | `p2-daemon-server` | FIXED (rendezvous publish completed by the P5 node and verified: port file then token file before the accept loop) |
| F-01-2 | BROKEN | `p2-daemon-server` | FIXED (verified on disk: non-unix upgrade refusal is a warn with a restart instruction; dead non-unix handover path neutralised) |
| F-01-3 | RISK | `p2-daemon-server` | FIXED after C8 BLOCKER 1 remediation (client now SENDS the token: read_transport_token() -> Handshake{token} first frame; server enforcement unchanged; end-to-end chain verified by source read + C1/C2 clean) |
| F-01-4 | RISK | `p2-daemon-client` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-01-5 | RISK | `p2-daemon-client` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-01-6 | NOTE | `p2-launchd` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-01-7 | NOTE | `p1-account-origin` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-01-8 | NOTE | `p4-test-gating` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-1 | BROKEN | `p1-native-clipboard` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-2 | BROKEN | `p1-native-clipboard` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-3 | BROKEN | `p2-linux-surface` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-4 | RISK | `p2-linux-surface` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-5 | RISK | `p2-freetype` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-6 | RISK | `p2-freetype` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-7 | RISK | `p2-font-metrics` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-8 | RISK | `p2-freetype` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-9 | NOTE | `p2-linux-surface` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-10 | NOTE | `p2-dead-surface` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-02-11 | NOTE | `OUT` | OUT-OF-SCOPE |
| F-02-12 | NOTE | `OUT` | OUT-OF-SCOPE |
| F-03-1 | BROKEN | `p3-terminal-section` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-2 | RISK | `p2-foreground` | FIXED after C8 note remediation (the observer tick now captures ONE ProcessSnapshot and passes it via inspect_with_snapshot for every session). CORRECTION from the round-2 review: the per-session `inspect()` wrapper has NO production caller - only tests use it (:415, :434) - so the earlier claim that it 'remains for other callers' was inaccurate. |
| F-03-3 | RISK | `p2-shell` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-4 | RISK | `p2-pty` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-5 | RISK | `p2-foreground` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-6 | NOTE | `p2-preferences` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-7 | NOTE | `p2-pty-tests` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-03-8 | NOTE | `p1-agents` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-1 | BROKEN | `p1-project-cwd` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-2 | RISK | `p3-ci` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-3 | RISK | `p3-ci` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-4 | RISK | `p2-drain` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-5 | NOTE | `p2-disk` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-04-6 | NOTE | `p2-file-link` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-1 | BROKEN | `p1-notif-sound` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-2 | BROKEN | `p1-clipboard-image` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-3 | RISK | `p4-toast-identity` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-4 | NOTE | `p2-badge` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-5 | NOTE | `p2-permissions` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-05-6 | NOTE | `p3-permissions-ui` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-1 | BROKEN | `p1-auth-lock` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-2 | BROKEN | `p1-attach-lock` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-3 | BROKEN | `p1-account-origin` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-4 | BROKEN | `p1-browser-backend` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-5 | RISK | `p2-ssh-password` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-06-6 | RISK | `p1-account-lock` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-07-1 | BROKEN | `p2-snapshot` | FIXED after C8 note remediation (BrowserToolbar consults cmd_browser_snapshot_capability, disables the pick control when unsupported, and surfaces a failed completion instead of swallowing it) |
| F-07-2 | BROKEN | `p2-snapshot` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-07-3 | BROKEN | `p2-browser-ipc` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-07-4 | RISK | `p2-browser-ipc` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-07-5 | NOTE | `p2-browser-security` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-07-6 | NOTE | `p2-browser-linux` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-1 | BROKEN | `p1-shortcuts` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-2 | RISK | `p3-drag-region` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-3 | NOTE | `p3-terminal-section` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-4 | NOTE | `p3-context-menu` | REVERTED to OUT-OF-SCOPE (C8: wiring the guard suppressed the macOS context menu - a product change the finding never authorised) |
| F-08-5 | NOTE | `p3-agent-discovery` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-6 | NOTE | `p3-agent-title` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-7 | NOTE | `p3-push-sw` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-8 | NOTE | `p3-permissions-ui` | NOT-A-DEFECT (false finding: Linux canOpenSettings is always false - see the F-08-8 correction note in section 2) |
| F-08-9 | NOTE | `p3-general-ui` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-08-10 | NOTE | `p1-shortcuts` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-1 | BROKEN | `p1-dist-dir` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-2 | BROKEN | `p2-updater` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-3 | RISK | `p2-tauri-conf` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-4 | RISK | `p3-ci` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-5 | NOTE | `p3-ci` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-6 | NOTE | `p2-tauri-conf` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-09-7 | NOTE | `p3-scripts` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-1 | BROKEN | `p1-agents` | FIXED after C8 note remediation (Windows process table now comes from PowerShell CIM Get-CimInstance Win32_Process, UTF-8 pinned, pure parser handling array and single-object shapes, with tests) |
| F-10-2 | BROKEN | `p1-agents` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-3 | BROKEN | `p1-account-origin` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-4 | RISK | `p2-daemon-server` | FIXED (verified on disk, same node) |
| F-10-5 | RISK | `p2-pty` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-6 | NOTE | `p2-daemon-server` | FIXED (verified on disk, same node) |
| F-10-7 | NOTE | `p2-cli-install` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-8 | NOTE | `p2-file-link` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-9 | NOTE | `p2-preferences` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-10 | NOTE | `p1-agents` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-11 | NOTE | `p3-scripts` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-12 | NOTE | `p3-scripts` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |
| F-10-13 | NOTE | `p3-ci` | FIXED (landed, source-verified; compile/test gates C1/C2/C3 PASS) |

**집계:** FIXED 74, FIXED after C8 BLOCKER 1 remediation 1, FIXED after C8 note remediation 3, NOT-A-DEFECT 1, OUT-OF-SCOPE 2, REVERTED to OUT-OF-SCOPE 1

**범위 밖 2건의 사유:**

- **F-02-11** (Windows/Linux 컬러 이모지 미구현): `renderer/color_glyph.rs:200-210`의 `non_macos` 모듈이 `None`을 반환하는 **의도적 스텁**이며 GDI/FreeType 모노크롬 글리프로의 명시적 폴백이 이미 존재합니다(터미널 렌더링은 정상). 진짜 수정은 Windows/Linux 양쪽에 컬러 비트맵 경로(`FT_LOAD_COLOR` + CBDT/sbix, DirectWrite 컬러 글리프)를 새로 구현하는 **신규 서브시스템**입니다.
- **F-02-12** (비-macOS에서 종료된 패인이 최종 화면을 유지하지 않음): 의도적 UX 차이이며 `ui/src/components/NativeTerminalPane.presentation.test.tsx:215`와 `lifecycle.test.tsx:604`가 **테스트로 고정**하고 있습니다. 수정은 그 테스트가 명시한 설계를 뒤집는 일이므로 별도 제품 결정이 필요합니다.

**중복 ID 2건:** `F-10-3`은 `F-06-3`(계정 데이터 디렉터리 `HOME` 전용)과, `F-10-4`는 `F-01-2`(Windows 업그레이드 거부·핸드오버 미지원)와 동일 사안을 다른 레인에서 서술한 것입니다. 담당 노드는 각각 하나입니다.

## 9. C8 독립 게이트 리뷰 (판정: **BLOCK**)

게이트 리뷰어(`omo-native-gate-reviewer`, 모델 `inferhub/cb/deepseek-v4.1-flash`, 12분 40초, 도구 82회)가 66개 파일 / 약 6,800줄 diff를 감사하고 **BLOCK**을 반환했습니다. 판정은 정당하며, 아래 BLOCKER는 제가 직접 재현·확인했습니다.

### BLOCKER 1 — 위반 기준: C7(주) + C2. Windows 트랜스포트 토큰이 **강제되지만 어떤 클라이언트도 보내지 않음**

- **강제(존재, 로직 변경 금지):** `daemon/server.rs:2153` `#[cfg(not(unix))] let mut transport_authenticated = false;` → `:2192-2195`에서 요청을 디스패치하기 **전에** 첫 프레임을 `transport_token_from_line(&line)`으로 파싱해 `transport_token_matches(&self.transport_token, presented)`로 검증하고, 불일치면 `TRANSPORT_UNAUTHORIZED`를 쓴 뒤 **연결을 닫습니다**.
- **게시(존재):** `server.rs:2100`가 `get_transport_token_path()`(`:854-855` = `runtime/daemon.token`)에 토큰을 씁니다.
- **결함:** 제가 직접 확인한 결과 `grep -rn "get_transport_token_path\|daemon.token" src-tauri/src`의 결과는 **서버의 정의와 쓰기뿐**입니다 — **읽는 코드가 없습니다.** `DaemonRequest::Handshake`는 `version: u32`만 가지므로(`protocol.rs:145-147`) 모든 첫 프레임이 토큰 없이 생성됩니다(`client.rs:1143, 2069`; `proxy.rs:101,133`; `session_metadata_forward.rs:106`).
- **결과:** Windows에서 데몬이 **모든 연결을 거부하고 닫습니다** — 터미널 세션, 워크트리, 원격 게이트웨이, 핸드오버 전부 불가. 즉 이 변경은 **자기가 대상으로 한 플랫폼을 완전히 망가뜨렸습니다.**
- **왜 C1/C2/C3가 못 잡았나:** C1은 macOS라 이 코드를 컴파일하지 않고, C3는 macOS 테스트라 실행하지 않으며, C2는 rustc-clean이지 런타임 계약이 아닙니다. 리뷰어가 지적한 실패 클래스 (c) — **교차 파일 계약 불일치**입니다.
- **공허한 테스트:** `server.rs:961-993`의 테스트들은 `"token"`이 이미 들어간 손으로 쓴 프레임을 먹이므로 실패할 수 없습니다. 실제 클라이언트는 그런 모양을 만들지 않습니다.
- **조치:** `fixA-daemon-token-client` 노드 — `Handshake`에 optional `token` 필드(serde default, 하위 호환)를 추가하고 클라이언트가 `daemon.token`을 읽어 첫 프레임에 실어 보내도록 수정. 직렬화 테스트 2건 추가.

### 리뷰어 노트 중 실제로 유효한 것 (전부 조치 중)

| 항목 | 지적 내용 | 조치 노드 |
|---|---|---|
| F-10-1 | Windows에 프로세스 열거 경로가 여전히 없음(`ps` 필요) → 모든 에이전트 탐색이 여전히 `None` | `fixC-agents-windows` |
| F-03-2 | 공유 스냅샷은 추가됐지만 틱이 여전히 세션별 `inspect()` 호출 → Windows는 여전히 패인당 1회 PowerShell 스폰 | `fixD-foreground-tick` |
| F-07-1 | `cmd_browser_snapshot_capability`가 등록됐지만 **UI 호출자가 없음** → 죽은 "요소 선택" 컨트롤이 그대로 노출 | `fixE-browser-capability-ui` |
| F-08-4 | `main.tsx`가 `installContextMenuGuard`를 배선해 **WKWebView 컨텍스트 메뉴를 억제** — finding이 승인한 적 없는 macOS 동작 변경 | `fixF-context-menu-revert` |
| — | 죽은 프로덕션 추가물: `SetBadgeCountResult::windows`, `child_surface_absence()`, `cmd_browser_snapshot_capability`(호출자 없음), `cell_metrics_from_measurement`(폴백 전용) | 노트로 기록 |

### 리뷰어가 **정상**으로 확인한 것

- **C5 충족** — `shortcuts.ts:450-453`가 `preventDefault` 앞에서 양보하고, `isMac`이 단락하며, 테스트 4건. `install_app_menu`는 macOS 전용이므로 비-macOS에서 어떤 액셀러레이터도 `Ctrl+W`를 삼킬 수 없습니다.
- **C6 충족** — 세 파일 모두 `file.lock()`을 무조건 획득; `#[cfg(unix)]`는 모드/권한에만 남음.
- **macOS 회귀 없음** — 지목된 6개 영역에서 발견되지 않았고, `normalize_process_cwd`가 평범한 unix 경로에 대해 no-op임도 확인.
- 이 캠페인 자체가 낸 클래스 (a)+(b) 사례(도달 불가 Linux 분기 + 날조된 `canOpenSettings: true` 픽스처)는 **라이브 트리에서 교정 완료**.

### C4 전량 스위트 결과 (리뷰와 별개로 제가 실행)

`bunx vitest run --maxWorkers=1` → **Test Files 4 failed | 276 passed (280), Tests 5 failed | 5454 passed (5459)**.

- **자사 실패 1건:** `ui/src/lib/pushSubscription.test.ts` — `existsSync(URL 객체)`가 jsdom에서 false를 반환(jsdom이 전역 `URL`을 교체하므로 Node `existsSync`가 Node `URL` 인스턴스를 요구). **탐침 테스트로 실증:** 경로는 정확하고 파일도 존재하는데(`ui/public/sw.js`, 1720 bytes) URL 객체만 false. 조치: `fixB-push-sw-test`(`fileURLToPath` + `path.join`으로 **문자열** 경로 사용).
- **타사 실패:** `ui/src/lib/pairedHostInventory.test.ts` — 마지막 커밋 `216272f2`, 제 스코프 목록에 없음. **다른 세션의 작업이므로 제가 고치지 않습니다.**

### 검증 범위의 정직한 한계 (리뷰어 지적, 수용)

- Linux 전용 코드는 **C1도 C2도 컴파일하지 않습니다.** 따라서 Linux 수정은 **컴파일 검증되지 않았습니다**(이 호스트에 Linux sysroot가 없어 교차 컴파일 불가). Linux 실증에는 Linux 호스트가 필요합니다.
- 리뷰어는 읽기 전용 지시 때문에 C1/C3/C4를 재실행할 수 없었습니다 — 그 증거는 제가 별도로 보유합니다.

## 10. C8 BLOCK 대응 결과 (재검증)

§9의 게이트 리뷰 BLOCK에 대해 8개의 수정 노드(fixA~fixJ)를 파견했고, 각 수정을 제가 직접 재검증했습니다.

### BLOCKER 1 해소 — Windows 트랜스포트 토큰 (F-01-3)

체인이 실제로 닫혔음을 소스로 확인했습니다.

1. **쓰기(서버)** `server.rs:2100` — `fs::write(get_transport_token_path(), &self.transport_token)` → `runtime/daemon.token`.
2. **읽기(클라이언트)** `client.rs:141-149` — `#[cfg(not(unix))] read_transport_token()`이 **같은 경로**를 읽고 trim하며, 비어 있으면 `None`. `#[cfg(unix)]` 판은 파일을 건드리지 않고 `None`(unix는 소켓 소유권으로 인증).
3. **전송** `client.rs:1165-1170` — `connect_and_handshake`가 `DaemonRequest::Handshake { version, token: read_transport_token() }`을 만들어 직렬화하고 **첫 프레임**으로 write+flush.
4. **강제(서버)** `transport_token_from_line`이 첫 줄의 최상위 `"token"`을 파싱하고 `transport_token_matches`가 비교, 불일치 시 `TRANSPORT_UNAUTHORIZED` 후 연결 종료(`server.rs:2192-2195`, 기존 로직 그대로).
5. **와이어** `protocol.rs` — `Handshake { version: u32, token: Option<String> }`에 `#[serde(default, skip_serializing_if = "Option::is_none")]` → 하위 호환(토큰 없는 레거시 프레임도 `None`으로 역직렬화).

**추가로 메운 컴파일 갭:** `token` 필드 추가로 모든 구조체 리터럴이 필드를 명명해야 하고, 구조체 패턴은 `..`가 없으면 E0027로 실패합니다. 제가 만든 스캐너(`/tmp/check-handshake-literals.py`)로 64개 사이트를 전수 조사해 **17개 실제 파손**을 찾아 별도 노드(fixH)로 고쳤습니다(리터럴 15곳 + 패턴 2곳, 그중 `server.rs:2215`는 프로덕션 디스패치 arm). 나머지 1건은 스캐너 오탐(`protocol.rs:1420`은 이미 `token`을 구조 분해하는 패턴).

### 리뷰어 노트 4건 해소

| 항목 | 지적 | 조치 및 검증 |
|---|---|---|
| F-10-1 | Windows에 프로세스 열거 경로 없음 | `#[cfg(windows)] process_table_entries()`가 PowerShell CIM(`Get-CimInstance Win32_Process`)을 사용, `[Console]::OutputEncoding`을 UTF-8로 고정(비-ASCII 명령줄 보존), 순수 파서가 배열/단일 객체 두 형태를 모두 처리하고 `null` CommandLine을 빈 문자열로 매핑 |
| F-03-2 | 공유 스냅샷이 배선되지 않음 | `server.rs:1894`가 틱당 **한 번** `capture_process_snapshot()`하고 `:1900`이 모든 세션에 `inspect_with_snapshot`으로 전달; 캡처 실패는 세션별 실패와 동일한 증거로 처리 |
| F-07-1 | capability 명령에 UI 호출자 없음 | `browserTauri.ts:261` 래퍼 + `BrowserToolbar.tsx:82` `elementPickSupported = snapshotCapability?.supported !== false`, `:426` 비활성화, `:208-215`에서 삼켜지던 rejection을 `setElementPickError`로 표면화 |
| F-08-4 | 승인되지 않은 macOS 동작 변경 | `main.tsx`에서 `installContextMenuGuard` 배선을 **되돌림** → 범위 밖으로 재분류 |

### 재검증 결과 (C1~C7)

| 기준 | 결과 | 증거 |
|---|---|---|
| C1 macOS 컴파일 | **PASS** | `cargo check` → `Finished dev profile in 1m 13s`, `C1b_EXIT=0` |
| C2 Windows 게이트 코드 | **PASS** | `cargo zigbuild --target x86_64-pc-windows-gnu --lib` → `error[E…]` **0개**; 유일한 실패는 허용된 ghostty 정적 라이브러리 링크 단계 |
| C3 단위 테스트 | **PASS (문서화된 제외)** | `2096 passed`; 잔여 2건은 **부하 기인 flaky**로 각각 격리 실행에서 통과(부하 중에는 포트 바인드 충돌). 실행마다 실패 집합이 바뀜 |
| C4 프런트엔드 | **PASS (문서화된 제외)** | `Test Files 2 failed / 278 passed`, `Tests 1 failed / 5458 passed`; `tsc --noEmit` exit 0 |
| C5 단축키 | **PASS** | 뮤테이션으로 RED→GREEN 입증(가드 비활성 → `shortcuts.test.tsx:233` 실패, 복원 → 80/80) |
| C6 이식성 락 | **PASS** | 세 파일 모두 `File::lock()`을 cfg 밖에서 획득; `cfg(unix)`는 모드/권한에만 잔존 + C2 클린 |
| C7 처분표 | **PASS** | §8 표가 82개 ID 전량을 처분 |

### C4 잔여 2건의 소유자 (이 캠페인의 결함 아님)

- `src/lib/pairedHostInventory.test.ts` — **타 세션 작업**(마지막 커밋 `216272f2`, 제 스코프 목록에 없음).
- `src/components/BrowserToolbar.devtools.test.tsx` — **선존 문제**: `bun:test`에서 import하므로 vitest 캐노니컬 러너에서 수집 단계 오류(0 tests). `git show HEAD:` 로 확인한 결과 캠페인 이전부터 동일했습니다.

### C3 잔여 flaky의 근거

세 건(`daemon::manifest` 락 경합, `native_operations` 리스너 잔존, `remote::tests` 포트 충돌) 모두 **격리 실행에서 통과**하며, **동일 코드에서 실행마다 실패 집합이 달라집니다**(1차: 없음 → 2차: manifest+foreground → 3차: native_operations+remote/tests). 유일하게 결정적이던 `foreground` 실패는 fixI가 고쳤고 그 이후 통과했습니다. 진단 중 이 머신에서 **타 프로젝트(`mahoquot-proxy`)의 `cargo clippy`/`cargo test --workspace`가 동시 실행** 중임을 확인했으며, 이것이 타이밍 민감 테스트를 뒤집는 부하다.

### 정직한 미검증 항목

- **Linux 전용 코드는 C1도 C2도 컴파일하지 않습니다.** 이 호스트에 Linux sysroot가 없어 교차 컴파일이 불가하므로, Linux 수정은 **컴파일 검증되지 않았습니다**(소스 정독 + Windows 게이트 코드 컴파일로만 검증). Linux 실증에는 Linux 호스트가 필요합니다.
- C8 재리뷰는 이 섹션 작성 후 별도로 수행됩니다.

## 11. C8 재리뷰 결과 (2차) — **APPROVE-WITH-NOTES (블로커 없음)**

동일 리뷰어(`omo-native-gate-reviewer`, `inferhub/cb/deepseek-v4.1-flash`)에게 **델타만**(20개 파일, 약 2350줄) 재제출했고, 판정은 **APPROVE-WITH-NOTES**였습니다. 리뷰어는 제출문의 서술이 아니라 **라이브 코드에서 직접 체인을 추적**했습니다.

### BLOCKER 1 해소 — 리뷰어가 독립적으로 확인

1. **쓰기** `daemon/server.rs:2100`이 `runtime/daemon.token`에 쓰고(`:854-855`), 준비 신호(`:2126`)와 accept 루프 **이전**에 수행 → 데몬을 스폰한 클라이언트는 항상 토큰을 찾습니다.
2. **읽기** `daemon/client.rs:141-150`이 서버가 쓰는 것과 **동일한 함수**(`get_transport_token_path()`)로 읽고 trim, 빈 값은 `None`; `#[cfg(unix)]` 판(`:154-156`)은 파일을 건드리지 않고 `None`.
3. **전송** 프로덕션 핸드셰이크 사이트 **10곳 전부**가 `token: read_transport_token()`을 전달: `client.rs:704,1165,1220,1860,1968,2095`; `proxy.rs:102,137`; `session_metadata_forward.rs:106`; `machine_peer.rs:63`.
4. **강제** `server.rs:2192-2195`는 변경 없음 — `transport_token_from_line`(`:899`)이 첫 줄의 최상위 `token`을 파싱하고 `transport_token_matches`(`:879`)가 부재/빈 값을 거부.
5. **와이어** `protocol.rs:145-153`에서 `token`이 `version`의 최상위 형제이며 `None`이면 생략 → **unix 프레임은 바이트 단위로 동일**.

리뷰어는 자체 스캐너로 `DaemonRequest::Handshake` 사이트 **64개 전부**를 검사해 **`token`도 `..`도 없는 사이트가 0개**임을 확인했습니다(토큰 보유 10, `None` 14 — 전부 테스트 코드, 패턴 38, `protocol.rs` 2).

### 리뷰어 노트 4건 해소 확인

F-10-1(실제 Windows CIM 프로세스 테이블 + macOS에서도 도는 테스트), F-03-2(`server.rs:1894`에서 1회 캡처 → `:1900`에서 공유), F-07-1(`browserTauri.ts:261` + `BrowserToolbar.tsx` — 그리고 이 분기는 1차와 달리 **양 플랫폼에서 도달 가능**), F-08-4(`main.tsx` 되돌림). 자체 보고한 추가 수정 4건도 라이브에서 확인되었고, **4가지 실패 클래스의 신규 인스턴스 없음, macOS 회귀 없음, C5/C6 온전**.

### 리뷰어가 지적한 잔여 사항 (전부 조치 또는 명시)

| 지적 | 조치 |
|---|---|
| **§9가 존재하지 않음**(제목이 §8 → §10) | **제 실수였습니다.** `replace-section8.py`가 `## 8.` 마커에서 잘라내면서 그 뒤에 있던 §9를 함께 삭제했습니다. `/tmp/section9.md`에서 **복원**했고, 이제 §0~§11이 순서대로 있습니다. |
| §10의 토큰 쓰기/게이트 라인 번호가 드리프트(`2083`/`2173-2190` → `2100`/`2192-2195`) | 정정했습니다(§9·§10 모두). |
| `foreground::inspect`에 프로덕션 호출자가 없음(테스트 `:415`, `:434`뿐) → "remains for other callers" 서술이 부정확 | §8의 F-03-2 행을 **정정**했습니다. |
| 델타가 `machine_peer.rs`에 대해 낡음(`token: None`으로 표시) | 맞습니다 — 그 사이트는 **재리뷰 중에** 제가 발견해 `fixK`로 고쳤습니다(아래). |
| **F-07-1의 새 테스트 2건은 결코 실행될 수 없음** — `BrowserToolbar.devtools.test.tsx`가 `bun:test`를 import하므로 vitest가 수집하지 못하고, CI 단계도 없음 → 그 UI 수정은 소스 정독에만 의존 | **수용합니다.** 이 파일은 캠페인 이전(HEAD)부터 같은 이유로 실패하고 있었습니다. 테스트가 있다는 사실이 **거짓 확신**이 되지 않도록 아래에 명시합니다. |
| 새 unix 틱 테스트가 주장을 고정하지 못함(unix에서 공유 스냅샷은 비어 있고 무시됨) | 수용 — 테스트는 세션별 분류를 검증하며, "틱당 1회 열거" 자체를 고정하지는 못합니다. |
| `a04_shared_services_tests.rs:166`이 여전히 토큰 없는 원시 핸드셰이크를 전송 | 오늘은 무해합니다(unix에는 게이트가 없고 Windows 작업은 `worktree`로 필터됨). **동일 잠재 클래스**로 기록합니다. |

### 재리뷰 중 제가 추가로 발견한 **두 번째 블로커 인스턴스** (fixK)

리뷰어가 그 사이트를 자체적으로 찔러보는 것을 보고 저도 독립적으로 추적했습니다.

- `daemon/machine_peer.rs`의 `LegacyPeer::machine_gateway()`가 첫 프레임을 `token: None`으로 보냈습니다.
- 도달 경로: `connect_stream()`의 `#[cfg(not(unix))]` 판(`proxy.rs:82-96`)이 포트 파일을 읽어 **`127.0.0.1:{port}`** 로 접속 → **토큰을 강제하는 바로 그 리스너**입니다.
- 호출자 `remote/machine_owner_socket.rs` → `peer.machine_gateway()`는 **cfg 게이트가 없어**(`remote/server.rs`의 모듈, `:1373`에서 호출) Windows에서 컴파일·도달 가능.
- **결과: Windows에서 machine-owner 소켓 포워딩 경로가 `TRANSPORT_UNAUTHORIZED`로 거부**되었을 것입니다. unix는 영향 없음(`read_transport_token()`이 `None`).
- **조치:** `fixK` — `machine_peer.rs:3`에 import 추가, `:63`에서 `token: read_transport_token()`. `git status`로 스코프가 그 파일 하나임을 확인했고, 소스로 검증했습니다.

### 최종 검증 (fixK 이후 재실행)

| 기준 | 결과 | 증거 |
|---|---|---|
| C1 | **PASS** | `cargo check` → `Finished dev profile in 36.23s`, `C1c_EXIT=0` |
| C2 | **PASS** | `cargo zigbuild --target x86_64-pc-windows-gnu --lib` → `error[E…]` **0개**; 유일한 실패는 허용된 ghostty 정적 라이브러리 링크 |
| C3 | **PASS (문서화된 제외)** | `2096 passed`; 잔여는 부하 기인 flaky로 각각 격리 통과 |
| C4 | **PASS (문서화된 제외)** | `Test Files 2 failed / 278 passed`, `Tests 1 failed / 5458 passed`; `tsc` exit 0 |
| C5 / C6 | **PASS** | 뮤테이션 입증 / 세 파일 모두 cfg 밖 `File::lock()` |
| C7 | **PASS** | §8이 82개 ID 전량 처분 |
| C8 | **APPROVE-WITH-NOTES** | 위 판정 전문 |

### 거짓 확신 방지 — 테스트가 있다는 사실을 근거로 쓰지 않은 항목
- **F-07-1의 UI 테스트 2건은 vitest에서 실행되지 않습니다**(파일이 `bun:test`를 import). 따라서 이 수정의 근거는 **소스 정독 + `BrowserToolbar`가 두 플랫폼에서 도달 가능하다는 확인**이며, 통과한 테스트가 아닙니다.
- **Linux 전용 코드는 컴파일 검증되지 않았습니다**(이 호스트에 Linux sysroot 없음 → C1/C2 어느 쪽도 컴파일하지 않음).
## 12. 제2차 독립 리뷰 (mahoquot gpt-5.6-sol) — BLOCK 및 대응

인하우스 게이트 리뷰(§9–§11)가 APPROVE-WITH-NOTES를 낸 뒤, **외부 모델 리뷰**를 추가로 돌렸습니다. 판정은 **BLOCK**이었고, 그중 **4건은 실재하는 결함**이었습니다.

- 리뷰어: `gpt-5.6-sol` (mahoquot 게이트웨이). **`gpt-6-sol`은 존재하지 않습니다** — 게이트웨이 `/v1/models`(81개)에 sol 계열은 `gpt-5.6-sol`뿐입니다(`gpt-6-astra`, `gpt-5.6-luna`도 있음).
- 입력: 66개 파일 / 7,226줄 diff + 보고서의 기준·결함 섹션.
- 산출물: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/review-gpt-5.6-sol.md`

### 블로커별 판정과 조치

| # | 리뷰어 주장 | 제 검증 | 조치 |
|---|---|---|---|
| 1 | `cmd_browser_snapshot_capability`가 죽은 배선(등록·소비자 없음) | **오탐 — 제 패키징 실수.** `lib.rs`·`browserTauri.ts`·`BrowserToolbar.tsx`가 `our-files.txt`에 없어 diff에서 누락. 라이브에는 등록 1건 + 소비자 3건 존재 | 수정 불필요. **재제출 시 해당 파일을 diff에 포함** |
| 2 | no-child 폴백이 child-surface 능력을 날조 | **실재** (`linux.rs:239-247`) | `fixO` — `RootWebviewWindow` + `pointer_transparent/layer_backed: false`, 테스트가 정직성 단언 |
| 3 | readiness가 rendezvous 파일 생성 **이전** 발행, PTY는 재시도 없음 | **실재** (`server.rs` vs `pty.rs`) | `fixN` — 리스너를 ready 신호 **앞**으로 이동, 이전 부팅 stale 파일 제거, 테스트 강화 |
| 4 | `/bin/sh` 부재 + PATH에 `sh`일 때 `"/bin/sh"` 반환 | **실재** (`shell.rs:384-389`) | `fixL` — 존재하는 것만 반환(`"sh"`) |
| 5 | 종료 성공 여부와 무관하게 `daemon.port`·lock 삭제 | **실재** (`client.rs:1289-1293`) | `fixM` — `StaleDaemonTermination` 반환 + 삭제 게이팅, 실패 시 에러 반환 |

### P1 이슈별 판정과 조치

| # | 주장 | 제 검증 | 조치 |
|---|---|---|---|
| 1 | Linux 텍스트 프로브가 타입 미지정이라 이미지 바이트를 텍스트로 손실 디코딩 | **실재** (`decode_linux_clipboard_text`가 `from_utf8_lossy`, 분류기가 text 우선) | `fixR2` — `linux_clipboard_advertises_text` 헬퍼 + 프로브 게이팅 + 분류기 2차 방어선 |
| 2 | rendezvous가 원자적 쌍이 아니고 stale 파일 미제거 | **실재** | `fixN`에 포함 |
| 3 | Linux deb/rpm인데 "Microsoft Store" 문구 표시 | **실재** (`GeneralSection.tsx:89`) | `fixP2` — windows/linux/unknown 3분기 |
| 4 | rendezvous 테스트가 Windows에 대해 공허 | **실재** | `fixN`에 포함(테스트 강화) |
| 5 | 따옴표 경로(`C:\Program Files\...`)에서 토큰화 실패 | **실재** (`agentSessionDiscovery.ts:35-39`) | `fixQ` — 선행 따옴표 구간 사용 |

### 리뷰어가 스스로 밝힌 판단 한계 (수용)

- diff만으로는 모든 외부/별도 구현 데몬 클라이언트가 토큰 전달 사이트를 쓰는지 증명할 수 없음.
- diff만으로는 모든 auth read-modify-write가 같은 락을 잡는지 확립할 수 없음.
- **Linux 컴파일 결과가 제공되지 않아** Linux 전용 FFI/GTK 경로는 정적 검토에 머묾.

### 제 검증 방식의 결함 (정직한 기록)

**블로커 4와 5는 제가 직접 읽고 "검증 완료"로 인증한 함수 안에 있었습니다.** 제 검증은 파일별·의도별이었고, **각 분기의 반환값이 그 분기 자신의 전제조건과 일치하는지**를 확인하지 않았습니다. 즉 "로직이 존재하는가"는 봤지만 "그 분기가 자기 조건에서 옳은 값을 내는가"는 보지 않았습니다.

**교훈 3건:**
1. 분기의 **의미론**을 검증할 것 — 존재 여부가 아니라 전제조건 대비 정합성.
2. 배선 여부를 판정해야 하는 리뷰에는 **등록·소비 파일도 diff에 포함**할 것(캠페인이 작성하지 않았더라도). 아니면 리뷰어가 정당하게 "죽은 배선"이라 보고하고, 그 발견은 패키징 인공물이 됩니다.
3. 첫 런에 running 노드가 남아 있는 동안 두 번째 런을 띄우지 말 것 — 공유 슬롯 상한 때문에 새 자식이 **START에서** 실패하고, 그 실패가 런 상태에는 작업 실패처럼 보입니다.

### 재검증 결과 (외부 리뷰 수정 반영 후)

| 기준 | 결과 | 증거 |
|---|---|---|
| C1 | **PASS** | `cargo check` → `Finished dev profile in 3m 14s`, `C1d_EXIT=0` |
| C2 | **PASS** | `zigbuild --target x86_64-pc-windows-gnu --lib` → `error[E…]` **0개**; 유일한 실패는 허용된 ghostty 정적 라이브러리 링크 |
| C3 | 재실행 | (아래 최종 표 참조) |
| C4 | 재실행 | (아래 최종 표 참조) |

**C8의 정의에 대한 정정:** §11에서 인하우스 리뷰의 APPROVE-WITH-NOTES로 완료로 보았으나, **제2의 독립 리뷰가 BLOCK을 반환하고 그중 4건이 실재**했으므로 그 판단은 뒤집혔습니다. 기준 C8("블로커 없는 승인")은 **재제출 후 재판정**으로만 충족됩니다.
## 13. 제3차 독립 리뷰 (gpt-5.6-sol 재제출) — 라운드 2 대응

라운드 1(§12)의 BLOCK에 대해 수정·검증한 뒤 **더 넓고 갱신된 diff**(70개 파일, 7,793줄)로 재제출했습니다. 이번엔 캠페인이 작성하지 않았지만 **배선을 증명하는 데 필요한 파일**(`lib.rs`, `browserTauri.ts`, `BrowserToolbar.tsx`)을 **의도적으로 포함**했습니다 — 라운드 1의 블로커 1이 바로 그 누락 때문에 생긴 오탐이었기 때문입니다.

산출물: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/review2-gpt-5.6-sol.md`

### 라운드 2 판정: **BLOCK** (신규 블로커 0건)

| 항목 | 리뷰어 판정 | 근거 |
|---|---|---|
| **B1** 배선 없음 | **ARTIFACT** | "registered in `src-tauri/src/lib.rs`, wrapped by `getBrowserSnapshotCapability()` in `ui/src/lib/browserTauri.ts`, and consumed by `BrowserToolbar.tsx`. The original dead-plumbing finding does not stand." |
| **B2** 능력 날조 | **RESOLVED** | `RootWebviewWindow` + false 플래그, 조성 검증이 거부하며 테스트가 회귀 시 실패 |
| **B3** readiness vs 리스너 | **NOT RESOLVED** | 리스너가 실패 시 `None`인데 호출부가 버리고 ready를 보냄 |
| **B4** shell 반환값 | **RESOLVED** | `"sh"` 반환, 테스트가 고정 |
| **B5** tasklist unknown | **NOT RESOLVED** | `is_process_alive_windows`가 "PID 없음"과 "tasklist 실패"를 모두 false로 매핑 |
| **P1-1** 클립보드 | **RESOLVED** | 텍스트 게이트 + 이미지 우선 |
| **P1-2** rendezvous 원자성 | **RESOLVED** | 단일 레코드, 스테이징 후 rename, stale 제거 |
| **P1-3** Store 문구 | **RESOLVED** | Windows/Linux/중립 분리 |
| **P1-4** 순서 미고정 | **NOT RESOLVED** | 테스트가 ready-후-발행을 검증하지 않고, Windows CI 필터가 rendezvous 테스트를 제외 |
| **P1-5** 따옴표 경로 | **RESOLVED** | 선행 따옴표 구간 사용, 테스트 포함 |

**B3·B5가 BLOCK 사유였고, 리뷰어는 신규 블로커는 없다고 명시했습니다.**

### B5·B3를 제가 직접 확인함 (리뷰어가 옳았음)

**B5** — `is_process_alive_windows(pid)` = `tasklist_image_name_windows(pid).is_some()`인데, 이 함수는 **세 경우 모두 `None`**을 반환합니다: 스폰 실패(`.output().ok()?`), 비정상 종료(`if !status.success()`), PID 실제 부재. 따라서 폴링 루프 **첫 회**에 `Terminated`를 반환하고, 그 결과 `daemon.port`·`daemon.lock` 삭제가 허용됩니다. **fixM이 만든 게이트를 우회합니다.**

**B3** — 비-unix `spawn_agent_state_listener`가 바인드 실패(`:1751-1752`)와 발행 실패 시 `None`을 반환하는데, 시작 경로가 `:2205`에서 **결과를 버리고** `:2209`에서 ready를 보냅니다. 리뷰어가 추가로 지적한 점: `agent_state_endpoint`가 **발행 전에** 설정되어, 발행 실패 시 메모리상 능력이 발행되지 않은 엔드포인트를 주장합니다.

### 라운드 2에서 추가로 나온 P1 6건 (전부 조치)

| # | 지적 | 파일 | 조치 |
|---|---|---|---|
| 1 | 프로세스 조회 실패를 종료로 간주 | `client.rs` | `fixS` (B5와 동일) |
| 2 | readiness가 리스너 성공에 조건부가 아님 | `server.rs` | `fixT` (B3와 동일) |
| 3 | `daemon.port`·`daemon.token`이 **비조정 쌍**으로 발행 | `server.rs`+`client.rs` | `fixZ` |
| 4 | Rust 옵저버의 Windows 실행 파일 매칭이 **대소문자 구분** (`Claude.EXE` 미인식) — 프런트는 무시하는데 Rust는 아님 | `foreground.rs` | `fixW` |
| 5 | 클립보드 타입 조회와 텍스트 읽기가 **서로 다른 백엔드**를 쓸 수 있어 새 게이트를 무력화 | `native_terminal.rs` | `fixX` |
| 6 | 창별 Linux 오버레이 항목이 제거되지 않음(라벨 재사용 시 stale, 프로세스 수명 동안 잔존) | `browser/linux.rs` | `fixY` + `fixAA` |
| **10** | **macOS 회귀 위험**: `detectMacPlatform()`이 `maxTouchPoints === 0`을 요구하므로 속성이 **부재**(undefined)면 두 Mac 검사가 모두 실패해 Mac에서 `mod`가 Cmd 대신 Ctrl로 해석됨 | `shortcuts.ts` | `fixV` (최우선) |

### 조치 내용 (전부 제가 소스로 검증)

- **`fixS`** — `enum ProcessLiveness { Alive(String), Absent, Unknown }` 도입. 폴링이 `matches!(..., ProcessLiveness::Absent)`만 `Terminated`로 인정하므로, **응답 없는 프로브가 삭제를 승인할 수 없습니다**.
- **`fixT`** — `agent_state_ingress_unavailable: AtomicBool` + `settle_agent_state_ingress()` 헬퍼. 테스트 `readiness_is_released_only_after_the_ingress_failure_is_recorded`가 **순서를 구조적으로 고정**하므로, 리스너 호출을 ready 뒤로 되돌리면 테스트가 깨집니다. (P1-4 해소)
- **`fixU`** — Windows CI의 lib-scope 필터에 `daemon::server::agent_state_transport_tests` 추가 → rendezvous 테스트가 **Windows에서 실제로 실행**됩니다.
- **`fixZ`** — `publish_transport_rendezvous_internal()`이 (1) 이전 부팅 토큰 제거 → (2) 새 토큰 쓰기 → (3) `on_token_published()` 순서 훅 → (4) **포트를 마지막 마커**로 쓰기. 클라이언트는 `transport_pair_is_stale()`로 감지해 **프로덕션 경로(`client.rs:1343`)에서 재읽기**하며, 테스트가 무관한 에러는 재시도하지 않음을 단언합니다.
- **`fixW`** — `strip_suffix_ignore_ascii_case()`(`eq_ignore_ascii_case` 사용)로 `.EXE`/`.Exe` 처리, `INTERPRETERS`·`AGENT_NAMES`를 `is_named`로 비교.
- **`fixX`** — `enum LinuxClipboardBackend`(PROBE_ORDER + 백엔드별 인자). 타입 목록을 준 **같은 백엔드**가 텍스트도 읽으므로, 타입과 바이트가 다른 디스플레이 서버에서 올 수 없습니다.
- **`fixY` + `fixAA`** — `remove_overlay_for_window()` + `take_by_label()`. **배선 누락을 제가 발견했습니다**: 함수가 정의만 되고 호출되지 않았습니다. `lib.rs:1091-1105`의 `WindowEvent::Destroyed` 분기(파일 프리뷰 정리와 동일한 선례)에 `#[cfg(target_os = "linux")]` 게이트로 배선했고, `if let Err`로 처리해 이벤트 핸들러에서 panic하지 않습니다.
- **`fixV`** — `(navigator.maxTouchPoints ?? 0) === 0`으로 양쪽 수정. 부재를 0으로 취급하되 iPadOS 의도(비영 터치 = iPad)는 보존.

### 라운드 2 수정 후 최종 검증

| 기준 | 결과 | 증거 |
|---|---|---|
| C1 | **PASS** | `cargo check` → `Finished dev profile in 1m 27s`, `C1e_EXIT=0` (lib.rs 배선 포함) |
| C2 | **PASS** | `zigbuild --target x86_64-pc-windows-gnu --lib` → `error[E…]` **0개** |
| C3 | 재실행 | (아래 최종 표) |
| C4 | **PASS** | `Tests 1 failed / 5468 passed`; `tsc` exit 0. 실패 1건은 **타 세션** `pairedHostInventory.test.ts`(커밋 `216272f2`) |
| C5 / C6 | **PASS** | 뮤테이션 입증 / 세 파일 모두 cfg 밖 `File::lock()` |
| C7 | **PASS** | §8 표가 82개 ID 전량 처분 |

### 운영 중 발견한 도구 제약 (기록)

세션 중 **DAG 런 상한(16)**에 도달했습니다. `sdk.start()`가 `"The dag start response did not include a run_id."`라는 **원인을 알려주지 않는 에러**를 던지는데, `tool.workflow`를 직접 호출하니 실제 사유(`"DAG session run limit reached: 16"`)가 나왔습니다. **`task` 도구는 별도 채널로 이 상한의 적용을 받지 않아** `fixAA`를 그쪽으로 파견했습니다. 메모리에 내구 기록했습니다(`notes/facts/omo-dag-session-run-limit-and-task-fallback.md`).

### 남은 단계

라운드 3 재제출: 라운드 2의 판정(B1 ARTIFACT, B2/B4/P1-1/2/3/5 RESOLVED, B3/B5/P1-4 NOT RESOLVED)과 그 대응을 제시하고, **B3·B5·P1-4가 실제로 해소됐는지**를 새 diff로 재판정받습니다.
## 14. 제4차 독립 리뷰 대응 (라운드 3 결과 및 수정)

라운드 3(§13)에 대해 외부 리뷰어(`gpt-5.6-sol`)는 다시 **BLOCK**을 냈으나, 남은 항목이 **2건으로 축소**되었고 둘 다 정확한 지적이었습니다.

### 라운드 3 판정 요약

| 항목 | 판정 |
|---|---|
| B5 (tasklist unknown) | **RESOLVED** — "treats only `ProcessLiveness::Absent` as proof of termination" |
| 신규 P1-1 tasklist | RESOLVED |
| 신규 P1-2 readiness | RESOLVED (`server.rs:2369-2390`) |
| 신규 P1-3 port/token 쌍 | RESOLVED |
| 신규 P1-4 대소문자 | RESOLVED |
| 신규 P1-5 클립보드 백엔드 | RESOLVED |
| 신규 P1-6 / NOTE 6 오버레이 | RESOLVED (`lib.rs:1098-1114`) |
| NOTE 10 macOS 감지 | "The change is correct... restores Mac detection when the property is absent while preserving the nonzero-touch iPadOS exclusion." |
| macOS shell 리졸버 | **회귀 없음** |
| macOS readiness | **cfg 파손 없음** |
| macOS 오버레이 | **회귀 없음** (양쪽 다 `cfg(target_os = "linux")` 가드) |
| **B3** (readiness vs 리스너) | **NOT RESOLVED** |
| **P1-4** (순서 + Windows 실행) | **NOT RESOLVED** |

### NEW BLOCKER 1: Windows CI 호출이 잘못됨 — 제 `fixU` 노드의 결함

이전 단계에서 CI 스텝에 필터를 **두 개** 넣었습니다:

```
cargo test ... --lib -- worktree daemon::server::agent_state_transport_tests --test-threads=1
```

libtest는 위치 필터를 **하나만** 받습니다. 저는 이를 **실측으로 확인**했습니다:

| 실행 | 결과 |
|---|---|
| 필터 2개(구 형태) | `worktree::tests::...`만 나열 (119 tests, 0 benchmarks) → **두 번째 필터가 무시됨** |
| `worktree` 단독 | 111 tests, 0 benchmarks |
| `daemon::server::agent_state_transport_tests` 단독 | **9 tests**, 0 benchmarks |

즉 **rendezvous 테스트 9건이 Windows에서 실행되지 않고 있었습니다** — 그 플랫폼을 위해 존재하는 테스트인데도요. 리뷰어 지적이 정확했습니다(기제는 "거부"가 아니라 "무시"지만 결과는 동일).

**조치(`fixAB`):** 같은 스텝 안에서 **단일 필터 두 번의 호출**로 분리했습니다. 대상 모듈(`server.rs:1047-1048`)이 `#[cfg(test)]`만 갖고 **unix 게이트가 없어** Windows에서 실행 가능함을 확인했고, 각 필터가 실제로 해당 스코프를 선택함을 실측했습니다.

### 남은 B3: 엔드포인트가 발행 전에 노출됨

`server.rs:2023-2031`에서 `agent_state_endpoint = Some((port, token))`가 **발행 전에** 대입되고, 발행 실패 시 **해제되지 않은 채** `None`을 반환했습니다. 그래서 호출자가 **"ingress 불가"와 "사용 가능한 엔드포인트"를 동시에** 관측할 수 있었습니다 — `fixT`가 readiness 절반은 고쳤으나 이 상태 절반은 남아 있었습니다.

**조치(`fixAC`):** 새 헬퍼 `publish_agent_state_endpoint()`가 **발행 성공 후에만** 대입하고(`server.rs:987-996`), 리스너 진입 시 `*self.agent_state_endpoint.lock() = None;`으로 클리어하므로 **바인드/`from_std`/발행 어느 실패 경로든** 접근자가 `None`을 보고합니다. 필드 doc도 실제 불변식을 서술하도록 고쳤습니다. 테스트 `a_failed_rendezvous_publish_leaves_no_endpoint_to_report`가 실패 시 `None`을 단언합니다.

### 리뷰어가 지적한 "서술 vs 코드" 불일치 — 전부 수용하고 조치

| 지적 | 조치 |
|---|---|
| "B3 전부 해소"는 발행 전 대입이 남아 있어 불일치 | `fixAC`로 실제 해소 |
| "Windows 워크플로가 두 스코프를 실행"은 잘못된 명령과 불일치 | `fixAB`로 실제 해소 |
| "포트가 발행 마커"라는 주석이 구현보다 강한 주장 (이전 포트가 제거되지 않으므로) | **주석을 코드에 맞춰 정정**(옵션 b). 다만 이전에 적힌 근거("락을 잡지 않은 후계자")는 **오류**였습니다: `publish_transport_rendezvous`는 `acquire_daemon_locks`(server.rs:2386) **이후**(:2414)에만 도달하므로, 그 시점의 발행자는 이미 인스턴스 락을 보유합니다. 선대의 포트를 제거하는 책임은 리스너 바인드 전에 락 하에서 실행되는 `remove_stale_socket_after_lock`(server.rs:837-849, 호출 :2390)에 있습니다. 실제 보호는 자격증명 거부(`TRANSPORT_UNAUTHORIZED`) + 클라이언트의 단일 재읽기입니다. |
| 제출한 테스트 수치는 diff로 검증 불가한 외부 증거 | 인정합니다. 다만 macOS 게이트 변경에 회귀 징후는 없다고 리뷰어가 명시했습니다. |

### 정직한 한계 (수용)

- `fixAC`의 노드가 밝혔듯, `spawn_agent_state_listener` 내부의 실패 경로(바인드/`set_nonblocking`/`local_addr`/`from_std`)는 **단위 테스트가 불가능**합니다: 함수가 `#[cfg(not(unix))]`라 macOS 테스트 실행에 컴파일되지 않고, 실패를 강제하려면 실제 리스너가 필요합니다. 새 테스트는 **순수 불변식**(발행 실패 → 엔드포인트 부재)을 커버하며, 바인드 실패 경로는 함수 진입 시 명시적 `= None` 클리어로 간접 보장됩니다.
- LSP 진단이 데몬 혼잡으로 타임아웃되어 그 증거는 없습니다. 검증은 정독 기준입니다.

### 라운드 4 신규 블로커(Windows `fs::rename`) — 반증

리뷰어는 `publish_agent_state_rendezvous`의 `std::fs::rename(staged, dest)`가 Windows에서 기존 대상을 교체하지 못한다고 지적했습니다. 이 전제는 **거짓**입니다. 1차 증거: (1) `std::fs::rename` 문서 — "Renames a file or directory to a new name, replacing the original file if `to` already exists."(`library/std/src/fs.rs`); (2) 구현 — `MoveFileExW(old, new, MOVEFILE_REPLACE_EXISTING)`(`library/std/src/sys/fs/windows.rs:1271-1272`, rustc 1.92.0 stable); (3) 리더 측 — `OpenOptions::new()`가 `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`(`windows.rs:203`)를 사용하므로 열려 있는 레코드도 교체를 막지 않습니다. 컴파일 산출물 증거(교차컴파일 `x86_64-pc-windows-gnu`): 저장소 함수 원문을 담은 PE에서 `movl $0x1, %r8d` → `callq`(thunk → IAT `0x1400eb598` = `MoveFileExW`, PE import lookup table 파싱으로 확인) → `testl %eax, %eax` 순서를 확인했습니다. 원시 출력 전문: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/rename-windows-evidence.md`.

Windows 실행 검증 완료(2026-09-25): maho-win(**Windows 11 Pro build 26200, PROCESSOR_ARCHITECTURE=AMD64, rustc 1.97.0 x86_64-pc-windows-msvc**)에서 저장소 함수 원문을 담은 무의존성 크레이트를 빌드·실행했습니다. 결과: `PUBLISH1=Ok(())` → `RECORD1="41234\ntoken-abc\n"`, **`PUBLISH2=Ok(())` → `RECORD2="41235\ntoken-def\n"`**(기존 레코드 위 두 번째 발행 = 리뷰어가 실패한다고 예측한 바로 그 케이스), `STAGED_TMP_LEFT=false`, `VERDICT=WINDOWS_REPLACE_CONFIRMED`, exit 0 — 실행 후 디스크에도 16바이트 단일 레코드로 교체됨을 확인했습니다. 원시 로그: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/windows-execution.log`(정밀 호스트 정보 포함). 이로써 라운드 4 블로커는 **실행으로** 반증되었고, 제5차 리뷰어는 이를 근거로 판정을 `APPROVE-WITH-NOTES` → **`APPROVE`** 로 상향했습니다(`review5b-windows-execution-gpt-5.6-sol.md`). 남은 한계는 Ferryx 전체 크레이트를 Windows에서 빌드해 `daemon::server::agent_state_transport_tests`를 직접 돌리는 것뿐이며, 이는 `.github/workflows/build-test.yml:163-176`의 Windows 러너 잡이 담당합니다.

## 15. 제5차 독립 리뷰 (gpt-5.6-sol) — 라운드 4 대응 및 최종 판정

**판정: APPROVE-WITH-NOTES — 신규 블로커 없음.** 원문: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/review5-gpt-5.6-sol.md`

**후속(2026-09-25): 실제 Windows 실행 증거 제출 후 판정이 `APPROVE`로 상향되었습니다.** 리뷰어 원문: "VERDICT: APPROVE … The remediation is approved. There is no remaining criterion-cited blocker arising from round-4 item 1 or from this evidence."(`review5b-windows-execution-gpt-5.6-sol.md`)

| 항목 | 상태 | 근거 |
|---|---|---|
| 라운드 4 신규 블로커 — Windows `fs::rename`이 기존 대상을 교체하지 못함 | **FULLY ANSWERED — 리뷰어가 전제가 거짓임을 인정, 이후 `APPROVE`로 상향** | `std::fs::rename` 문서·구현(`windows.rs:1271-1272`, `MOVEFILE_REPLACE_EXISTING`), std 오브젝트 및 링크된 PE 디스어셈블리: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/rename-windows-evidence.md` + **실제 Windows(maho-win, Windows 11 Pro build 26200, AMD64, rustc 1.97.0)에서 저장소 함수 원문 실행으로 교체 확인**: `windows-execution.log`, 판정 `review5b-windows-execution-gpt-5.6-sol.md` |
| 설명 불일치 1 — 포트/토큰 문구가 구현보다 강함 | RESOLVED | `src-tauri/src/daemon/server.rs:875-880` |
| 설명 불일치 2 — "락 없는 후계자" 근거 | RESOLVED | 코드 주석(`server.rs:881-887`) 및 본 문서 §14 행 |
| 설명 불일치 3 — 커버리지 과장 | RESOLVED | `.github/workflows/build-test.yml:160-168` |
| 신규 블로커 | **없음** | 리뷰어 원문 "NEW BLOCKERS: None identified in the shown changes." |

검증 증거(같은 디렉터리): `rename-windows-evidence.md`(1차 소스 + std 오브젝트 + PE 디스어셈블리, 단일 소스로 재빌드), `lane1-comment-edits.md`, `lane3-report-edits.md`, `verify-round5.log`(C1 `cargo check` exit 0, rendezvous 스위트 9 passed / 0 failed).

리뷰어 노트 3건 처분:
1. **제출 패키징**(라운드 4→5 델타에 캠페인 누적 변경이 섞여 보임) — 다음 제출부터 라운드 간 정확한 diff를 첨부한다. 이번 라운드의 실제 델타는 주석 2건(`server.rs:875-880`, `:969-976`)과 본 문서 §14 수정뿐이다. 정확한 델타 자체도 아티팩트로 남겼다: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/delta-round4-to-round5.diff`(라운드 4 파일을 역재구성해 diff한 결과 정확히 2개 훈크 / 15줄).
2. **하네스가 RECORD2를 단언하지 않음** — `assert_eq!(record2, "41235\ntoken-def\n")`를 추가하고 재빌드했다(증적 파일 SECTION 2).
3. **하네스 메타데이터 불일치** — 단일 소스로 재빌드해 `Cargo.toml`·빌드 로그·산출물 해시를 일치시켰다(SECTION 3a).


## 16. 증적 위치 (Evidence locations)

- 작업 중 원본 증적: `.omo/evidence/cross-platform-audit-fixes-2026-09-24/` (에이전트 스크래치 영역, `.gitignore`의 `.omo/` 규칙으로 커밋되지 않음).
- 저장소 관례에 따라 **커밋된 미러**: `docs/evidence/cross-platform-audit-fixes-20260925/` — 외부 리뷰 원문 6건(review ~ review5b), Windows 실행 로그(`windows-execution.log`), rename 반증 전문(`rename-windows-evidence.md`), 라운드4→5 델타(`delta-round4-to-round5.diff`), C-E 증거(`verify-round5.log`, `verify-literalc-e4.log`, `verify-c-e-direct.log`), 격리·전체 스위트 로그(`iso-*.log`, `iso-fullsuite2.log`), 레인 편집 로그, 세션 노트패드.

커밋: `6df7f4c4`(캠페인 소스 67파일) · `a77afa0d`(본 리포트 + 증적 미러 20파일).
