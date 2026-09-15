# 경쟁 사용자 반응 → Ferryx 적용 분석 (2026-09-12)

**입력 문서:** `docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md`
**기준 커밋:** `02e62e49` (main, 작업 트리 dirty)
**분석 방식:** 입력 문서의 만족/불만 항목을 우리 소스와 1:1로 대조. 아래 모든 "우리 현황"은 이 세션에서 직접 확인한 파일·라인 근거만 적는다.

---

## 0. 결론 먼저

입력 문서에서 **새로 배울 게 있는 항목은 사실 많지 않다.** Herdr의 에이전트 상태 사이드바, cmux의 완료 알림, T3의 모바일 릴레이, Orca의 워크트리 격리·벤더 중립성 — 상위 도구들의 *만족 요인* 대부분은 Ferryx에 이미 구현돼 있다.

진짜 가치는 반대편, **불만 목록**에 있다. 이 카테고리 1·2위 도구(Orca 25GB 스왑/370 고아 데몬, cmux 80GB OOM)가 똑같은 실패 모드로 욕을 먹고 있다. **다만 Ferryx는 그 두 실패 모드를 이미 해결한 상태다**(§2 재검증 결과). 따라서 이 둘은 신규 작업이 아니라 회귀 방지 대상이다.

우선순위 요약 (2026-09-12 코드 재검증 후 수정):

- **P0 — 없음.** 최초 초안에서 P0로 올렸던 GPU 드로어블 누수와 고아 데몬은 **둘 다 이미 해결됨**(wgpu v30 업그레이드 / flock + 드레이닝 은퇴). §2 참조.
- **P1** — 워크트리 용량 표시·정리, 에이전트 상태 워치독(축소판). *죽은 코드 계열(Design Mode·인박스·모바일 채팅)은 2026-09-12 사용자 결정으로 전부 보류.*
- **P2** — 헤드리스 리눅스 서버 배포 문서화(이미 되는데 안 알림), 프라이버시 선언 페이지, 다중 원격 호스트 장시간 입력 지연 소크 테스트, 중복 데몬 프로세스 레벨 회귀 테스트.
- **채택 금지** — 내장 에디터, 로그인 강제, 상시 프로세스 모니터, 로컬 기능의 클라우드 종속.

---

## 0-1. 실행표 (이것만 보고 결정해도 된다)

| # | 항목 | 현재 상태 | 할 일 | 비용 |
|---|---|---|---|---|
| 1 | Design Mode (브라우저 비주얼 피드백) | 없음. 인터페이스만 | **보류** (죽은 코드 defer, 2026-09-12) | 큼 |
| 2 | 워크트리 디스크 관리 | **범위 확정**: 독립 기능 + 온디맨드 스캔 | A~F 6단계 → `docs/FERRYX_WORKTREE_DISK_SCOPE_2026-09-12.md` | 중 |
| 3 | ~~OSC 133 블록~~ | **제외** (2026-09-12 사용자 결정) | 없음 | - |
| 4 | 에이전트 상태 워치독 | 없음 | 조정 패스 + 강제 초기화 | 중 |
| 5 | 대기 작업 인박스(A1) | 범위 확정됨(원격 포함 완전판) | **보류** (죽은 코드 defer) · 확정서 `docs/FERRYX_ATTENTION_INBOX_SCOPE_2026-09-12.md` | 큼 |
| 6 | 모바일 채팅·승인(C1·C2) | 죽은 코드, 백엔드 일부 | **보류** (죽은 코드 defer) | 중 |
| 7 | 헤드리스 서버 배포 | 코드는 됨, 문서 0 | 설치 가이드 + systemd | 소 |
| 8 | 프라이버시 선언 | 없음 | 사이트 페이지 1장 | 소 |
| 9 | 원격 다중호스트 지연 | 측정 기록 없음 | 소크 테스트 | 소~중 |
| 10 | 선택 영역 유지 | 테스트 없음 | 회귀 테스트 1개 | 소 |
| 11 | 중복 데몬 차단 | 동작함, 테스트 없음 | 프로세스 테스트 1개 | 소 |
| 12 | GPU 드로어블 누수 | **해결됨** (wgpu v30) | 없음 | - |
| 13 | 고아 데몬 누적 | **해결됨** (flock+은퇴) | 없음 | - |

권고 순서: **2 → 4 → 7·8 → 9·10·11**. 죽은 코드 3항목(1·5·6)은 보류, 3은 제외. 7·8은 코드 변경이 거의 없어 언제든 끼워 넣을 수 있다.

---

## 1. 이미 확보한 것 (경쟁사 불만을 우리는 이미 회피 중)

새로 할 일이 없다는 뜻이고, 동시에 **마케팅에서 말하지 않고 있는 자산**이라는 뜻이기도 하다.

> **판정 기준(2026-09-12 강화):** 파일이 존재한다는 사실은 기능의 근거가 아니다. 아래 항목은 **배선까지 확인**한 것만 남겼다 — UI는 상위 화면에서 import·마운트되어야 하고, Tauri 커맨드는 `lib.rs`의 핸들러 목록에 등록돼 있어야 하며, 백엔드 모듈은 호출자가 있어야 한다.

- **Ghostty의 `TERM=xterm-ghostty` SSH 참사 [S26] → 회피됨.** `src-tauri/src/terminal/pty.rs:144`와 `src-tauri/src/ferryx_scope/ssh/helper.rs:307` 모두 `TERM=xterm-256color`를 설정하고, `COLORTERM=truecolor`를 별도로 붙인다. libghostty를 엔진으로 쓰면서도 terminfo 파편화는 상속하지 않았다. 회귀 테스트도 `terminal/tests.rs`에 있다.
- **Ghostty 1.0의 Cmd+F 부재 → 회피됨(배선 확인됨).** `TerminalSearchOverlay.tsx`가 `TerminalPane.tsx:181`에서 마운트되고, `cmd_native_terminal_search`는 `lib.rs:1096`에 등록돼 `native_terminal/search.rs`(스크롤백 인지 검색)로 내려간다.
- **Ghostty의 "설정창 없이 텍스트 파일" 불만 → 회피됨.** `SettingsDialog` + Ghostty config 임포트(`terminal/preferences.rs`)로 GUI와 파일 설정이 공존한다.
- **Herdr의 에이전트 상태 가시성 [S31] (그들의 1등 만족 요인) → 이미 있음.** `ui/src/components/Sidebar.tsx`의 `StatusDot`, `AgentCards.tsx`(`Working`/대기 상태), `Sidebar.activity.test.tsx`·`sshActivity.test.tsx`.
- **cmux의 완료 알림 링 [S40] → 이미 있음(배선 확인됨).** `state/workspaceStore.ts:449`가 활동 전이에서 `notificationTarget`을 만들고, `lib/notificationCoordinator.ts:184`가 `isCompletionEdge`를 판정해 사운드·데스크톱 알림을 보낸다. `App.tsx:28`에서 `NotificationCoordinator`가 마운트되고, 백엔드 `cmd_notification_dispatch`/`_play_sound`/`_set_badge_count`는 `lib.rs:1136-1144`에 등록돼 있다. 클릭 복귀는 `lib/notificationActivation.ts`.
- **T3의 모바일 릴레이 [S46] → 이미 있음, 심지어 더 감.** `src-tauri/src/remote/relay_client.rs`·`relay_server.rs`(머신 토큰, audience 서명), `ui/src/remote/*`(RemoteApp, Pairing, MobileHostDrawer, push). T3는 릴레이가 제품의 핵심 세일즈 포인트인데 우리는 릴레이 서버까지 자체 운영 가능하다.
- **T3의 `-p` 스크래핑 ToS 리스크 [S51] → 구조적으로 회피됨.** `terminal/shell.rs`에 `-p`/`--print`/headless 호출 경로가 없다. 에이전트를 실제 PTY에서 대화형으로 띄우므로 "비공식 헤드리스 래핑" 범주에 들어가지 않는다.
- **Warp의 강제 로그인·무단 LLM 전송 [S20][S21][S22], T3의 상시 프로세스 모니터 [S50] → 구조적으로 회피됨.** 앱 본체에 텔레메트리/애널리틱스/크래시 리포터 의존성이 없다(저장소 전역 검색 결과 `telemetry|analytics|posthog|sentry`는 벤더 ghostty 소스와 사이트 opt-in 스크립트뿐). 기본 외부 통신은 `src-tauri/tauri.conf.json:34-36`의 업데이터 엔드포인트 하나, 그리고 사용자가 켰을 때의 릴레이/푸시뿐이다. MIT 라이선스(`LICENSE`).
- **Orca 데몬 세대 누적(daemon-vNN) [S11] → 설계상 부분 회피.** 우리는 버전별 데몬을 새로 띄우지 않고 **같은 프로세스를 in-place `exec`로 교체**한다: `daemon/client.rs:412 maybe_trigger_upgrade_if_stale` → `daemon/server.rs:250 perform_daemon_exec_with_path`(+`clear_cloexec`로 PTY fd 승계). 세션을 죽이지 않고 세대가 쌓이지도 않는 구조다. 중복 기동 차단과 자동 은퇴까지 포함한 전체 검증은 **§2 "해결됨 B"** 참조.
- **Windows Terminal의 스토어 종속 [S6] → 회피 중.** `ipc/updater.rs`가 MS Store 관리 설치와 NSIS 인앱 업데이트를 분기 처리한다. 스토어 밖 직접 배포 경로를 계속 유지할 것.

### 1-1. 코드는 있지만 배선되지 않은 것 = 없는 기능

`ui/src/features/ferryx/` 트리(총 832줄)는 **전체가 상위에서 import되지 않는 설계 스켈레톤**이다. 계획 문서(`FERRYX_COMPETITIVE_GAPS_PLAN_2026-09-05.md`)의 미체크 항목들과 정확히 대응한다. 기능이 있다고 오해하기 쉬우므로 명시한다.

- `design/` (DesignFeedback, session, overlay, model, httpUploader) — §3 P1-0. 네이티브 브리지·커맨드·마운트 전부 없음.
- `control/AttentionInbox.tsx` (18줄), `TaskControls.tsx`, `HostSessionSelection.tsx`, `useInventory.ts` — 계획의 **A1 "전체 프로젝트 대기 작업 모아보기"**에 해당. 마운트 안 됨. 단, 사이드바 단위 활동 집계(`Sidebar.tsx`)는 별도로 살아 있다 — "프로젝트를 가로지르는 단일 목록"이 없는 것.
- `chat/ManagedChat.tsx`, `push/client.ts` — 계획의 **C1·C2(모바일 채팅·승인)**. 백엔드는 `/api/push/subscribe` 라우트까지 있으나 UI가 끊겼다.
- `ui/src/remote/RemoteDesignOverlay.tsx`, `src-tauri/src/remote/design_mode.rs` — 위와 동일한 사유로 죽은 코드.

반대로 **배선까지 확인된 것**: 터미널 검색(`TerminalPane.tsx:181` 마운트 + `lib.rs:1096` 커맨드 등록), 사이드바 활동 표시(`App.tsx:11`), 원격 게이트웨이·릴레이(`remote/server.rs:2139-2159` 라우트 + `:2347` RelayClient 사용), `TERM` 설정(`pty.rs:144`).

---

## 2. 초안의 P0 두 건 — 코드 재검증 결과 **둘 다 이미 해결됨**

> 이 절은 2026-09-12 사용자 지적에 따라 소스와 의존성을 직접 재검증해 수정한 내용이다. 최초 초안은 `docs/FERRYX_MEMORY_FOOTPRINT_AUDIT_2026-09-05.md`(수정 이력 없음)만 보고 미해결로 단정했고, 그 판단은 틀렸다.

### 해결됨 A. 네이티브 터미널 GPU 드로어블 누수 → **wgpu v30 업그레이드로 종결**

- **과거 결함:** wgpu-hal 24.0.4가 NSView 백킹 레이어에 `WgpuObserverLayer` 서브레이어 + KVO를 심어 놓고 teardown이 없어, attach마다 Retina 드로어블 ~24MB가 영구 retain(45분에 1.54GB).
- **해결 근거:**
  - `src-tauri/Cargo.toml:85` → `wgpu = { version = "30", optional = true }`, `Cargo.lock` 기준 `wgpu`/`wgpu-core`/`wgpu-hal` 모두 **30.0.1**.
  - **wgpu-hal 30.0.1에는 `layer_observer.rs` 파일 자체가 없다.** 레이어 생성은 `wgpu-hal-30.0.1/src/metal/mod.rs:169`에서 `raw_window_metal::Layer::from_ns_view(...)`로 위임된다.
  - `raw-window-metal 1.1.0`의 `ObserverLayer`는 `impl Drop`(`src/observer.rs:49`)에서 `contentsScale`/`bounds` KVO를 **명시적으로 해제**하고, 루트 레이어를 weak로 잡아 선-해제 순서 문제도 처리한다. Surface가 보유하는 레이어는 objc2 `Retained<CAMetalLayer>`라 drop 시 자동 release된다.
  - 커밋 `29ea50be feat(native-terminal): upgrade wgpu to v30 and remove local wgpu-hal vendor patch` (2026-09-07).
  - `docs/FERRYX_METAL_OWNERSHIP_INVESTIGATION_2026-09-05.md` §4에 결론이 이미 기록돼 있다: *"Upstream wgpu v30 migrated Metal layer creation to `raw-window-metal`, completely replacing `WgpuObserverLayer` and resolving the layer leak upstream."* 우리 쪽에서 업스트림 PR(gfx-rs/wgpu#10271)까지 올린 뒤 로컬 벤더 패치를 제거한 순서다.
- **남은 일:** 별도 구현 없음. `docs/FERRYX_MEMORY_FOOTPRINT_AUDIT_2026-09-05.md`가 v24 시점 서술 그대로라 읽는 사람을 오도하던 문제는 **처리함** — 그 문서 상단에 "v30에서 해결됨" STATUS 주석을 추가했다(2026-09-12). 회귀 방지가 필요하면 attach/detach 150회 후 IOSurface 증가가 선형이 아님을 확인하는 측정 1회로 충분하다(신규 개발 아님).
- **정리 코멘트:** `src-tauri/src/native_terminal/platform/macos.rs:190-200`의 주석이 아직 "wgpu-hal 24 (`src/metal/layer_observer.rs`)"를 근거로 설명하고 있다. 동작은 여전히 유효(서브레이어 구조는 raw-window-metal도 동일)하지만 **주석이 stale**이므로 다음에 그 파일을 만질 때 갱신할 것.

### 해결됨 B. 고아 데몬 불변식 → **flock + 드레이닝 은퇴로 이미 강제 중**

- **재검증 결과(코드 근거):**
  - **중복 기동 차단:** `daemon/server.rs:696 acquire_daemon_locks`가 persistent/legacy 락 파일에 `libc::flock(LOCK_EX|LOCK_NB)`(`:576`)를 건다. 두 번째 획득은 실패하며, 그 계약은 `server.rs:3921-4004`의 테스트들이 이미 검증한다(단일/이중 락, 충돌, 해제 후 재획득).
  - **실패 시 자기 종료:** 락 실패는 `start()` → `run_daemon_headless` → `main.rs:38`로 전파돼 `eprintln!` 후 `std::process::exit(1)`. 즉 두 번째 데몬은 살아남지 못한다.
  - **핸드오버 중 2개는 설계된 정상 상태:** 새 데몬이 `CommitHandover`를 보내면 구 데몬은 `Draining`으로 전환해(`handover.rs:290`) 기존 PTY를 계속 서빙하고, 새 데몬은 `handover_routes.json`으로 라우팅한다. 구 데몬을 강제 종료하면 세션이 전멸하므로 이건 **금지 사항**이다(AGENTS.md).
  - **자동 은퇴:** `handover.rs:317 check_retirement_if_empty` → 세션이 비면 `retire()` → 레거시 소켓·라우트 정리 후 `std::process::exit(0)`(`:346`). 호출 지점은 (1) `Close`/`CommitHandover` 요청 처리 직후(`server.rs:1547`, `:2165`), (2) 세션 종료 클린업 태스크(`server.rs:2673`). 세션 0개로 핸드오버하면 `CommitHandover` 처리 직후 즉시 은퇴한다.
  - **현재 머신 실측:** `ps` 결과 `ferryx --daemon`은 **정확히 1개**(PID 36170, `--handover-from` 인자로 기동, uptime 1d22h). 누적 없음.
- **2026-09-05에 2개가 보였던 이유:** Orca식 세대 누적이 아니라 위 드레이닝 창(구 데몬이 잔여 세션을 서빙 중)일 가능성이 가장 높다. 디버그/릴리스 데몬은 런타임 디렉터리가 분리돼 있어(`server.rs:120-130`) 개발 중이면 정상적으로 2개가 공존할 수도 있다.
- **남은 진짜 공백(작음, P2):**
  1. **프로세스 레벨 회귀 테스트 없음.** 락 헬퍼 단위 테스트는 있지만 "두 번째 `ferryx --daemon`이 비정상 종료 코드로 죽는다"를 끝에서 끝까지 검증하는 테스트는 없다.
  2. **드레이닝 가시성 없음.** 장수 세션(며칠 도는 에이전트) 때문에 구 데몬이 오래 남는 건 설계대로지만, 사용자는 그 상태를 볼 방법이 없다. 설정 진단에 "데몬 1 + 드레이닝 N(세션 M개 보유)"를 노출하면 오해와 오조작(강제 kill)을 막는다.

---

## 3. P1 — 다음 분기 안에

### P1-0 (최우선). 인앱 브라우저 비주얼 피드백(Design Mode) — **실제로 없다**

- **경쟁 근거:** Orca가 내장 브라우저 Design Mode로 칭찬받는 항목 [S9]. 프론트 작업에서 "이 버튼 여백 8px"를 말로 설명하는 대신 요소를 집어서 에이전트에 넘기는 루프다. **경쟁 문서의 만족 요인 중 Ferryx가 유일하게 못 가진 핵심 기능이다.**
- **우리 현황(배선 기준 재검증):** 계약·상태기기까지는 설계돼 있고, **실행 경로는 통째로 없다.**
  - `ui/src/features/ferryx/design/session.ts`(89줄)는 `DesignBridge`·`AttachmentUploader`·`DesignDelivery` **인터페이스만** 정의한다. 주석에 "Native bridge owns ... compositor capture and PNG crop"이라 쓰여 있지만 **그 네이티브 브리지가 없다**: `lib.rs`/`ipc/`에 design 관련 Tauri 커맨드 0건, `remote/server.rs` 라우트 목록에도 design 엔드포인트 없음.
  - `src-tauri/src/remote/design_mode.rs`는 74줄짜리 DTO + `HashMap` 스테이징 스토어다. `stage`/`get`과 단위 테스트만 있고 **호출하는 쪽이 없다**(`mod.rs:23`의 `pub mod` 선언이 전부).
  - `ui/src/features/ferryx/design/DesignFeedback.tsx`(69줄, `design-mode-toggle` 버튼 포함)과 `ui/src/remote/RemoteDesignOverlay.tsx`는 **어느 화면에도 마운트되지 않는다** — `features/ferryx/` 밖에서의 import 0건.
  - `BrowserPane.tsx`·`BrowserToolbar.tsx`에 요소 선택·영역 드래그·스크린샷 캡처 코드 없음.
- **빠진 것(실제 구현 범위):** (1) 게스트 웹뷰에 주입하는 hover 하이라이트·선택 오버레이 스크립트와 이벤트 채널, (2) 오버레이를 제외한 컴포지터 캡처 + 선택 영역 PNG 크롭(네이티브), (3) 선택 요소의 selector/computed CSS 추출, (4) 메모 입력 UI를 실제 브라우저 툴바에 마운트, (5) 캡처+메모를 대상 터미널/에이전트로 전달하는 브리지(이미 있는 SSH 이미지 붙여넣기 경로 재사용 가능).
- **비용:** 크다(네이티브 캡처 + 게스트 웹뷰 주입 + 전달 파이프라인). 다만 인터페이스·테스트 스침이 이미 있어 설계 단계는 건너뛴 수 있다.


### P1-A. 워크트리 용량 표시와 회수 ("디스크 회계", Vibe Kanban #765)

> **용어 설명:** "디스크 회계"는 워크트리마다 디스크를 얼마나 쓰는지 **측정 → 표시 → 회수**하는 사이클을 뜻한다. 워크트리는 체크아웃 사본이라 각자 자기 `node_modules`/`target`/`.next`를 갖는다. Rust·프런트 혼재 프로젝트면 워크트리 하나가 수 GB~수십 GB이고, 에이전트가 워크트리를 찍어낼수록 선형으로 늘어난다. 지금 Ferryx는 삭제 기능은 있지만 **어느 워크트리가 몇 GB를 먹는지, 마지막으로 쓴 게 언제인지를 아예 모른다.** 사용자는 디스크가 꽉 차야 알아차리고 `du`로 직접 뒤져야 한다.

- **경쟁 근거:** 태스크가 끝나도 워크트리와 `node_modules`가 남아 이틀 만에 26GB 잠식 [S39]. 사용자가 도구를 버리는 실질적 이유다.
- **우리 현황:** 삭제·프룬 자체는 있다(`worktree/manager.rs:581 remove_worktree`, `git.rs:394 git_worktree_prune`, `WorktreeDeleteDialog.tsx`). 그러나 **용량을 재거나 보여주는 코드가 전무하다** — `worktree/*.rs`와 워크트리 UI에서 `disk|size_on_disk|bytes` 검색 0건. `WorktreeList.tsx`도 크기/마지막 사용 시각을 표시하지 않는다.
- **할 일:** 워크트리별 디스크 사용량(캐시된 백그라운드 계산, 반드시 `crate::ipc::run_blocking`)과 마지막 활동 시각을 사이드바/삭제 다이얼로그에 표시. "머지됨 + N일 미사용 + M GB" 기준의 정리 제안. 브랜치 안전장치는 기존 `delete_worktree_and_branch_with_prune_status` 재사용.
- **가치:** 경쟁사가 실제로 죽은 지점이고, 우리 구현 비용은 중간 이하.

### ~~P1-B. OSC 133~~ — **제외됨 (2026-09-12 사용자 결정)**

> 아래 분석은 기록으로만 남긴다. 결론: 마커를 뿜는 주체가 셸이라 에이전트 페인에서는 `omo` 실행 = 블록 1개뿐이고, 에이전트 CLI는 아직 마커를 뿜지 않는다(claude-code#26235·#32635는 요청만 열린 상태). 값이 나오는 곳은 플레인 셸 페인뿐이라 채택하지 않는다.

> **2026-09-12 정정:** 초안의 "Warp 가치 90%" 평가는 과대평가다. OSC 133 마커는 **셸이** 뿜는 것인데, 에이전트 페인은 TUI가 화면을 점유하므로 셸 입장에선 `omo` 실행 = **거대한 블록 1개**일 뿐이다. 에이전트 대화 내부 탐색에는 쓸모가 없다. 에이전트 CLI가 직접 마커를 뿜으면 달라지지만, Claude Code에는 그걸 요청하는 이슈(anthropics/claude-code#26235, #32635)가 열려 있기만 하다.
>
> 생태계 현황: Warp 전유 기능이 아니다. FinalTerm에서 출발해 iTerm2가 대중화했고 Ghostty(1.3+)·WezTerm·kitty·VS Code 통합 터미널(OSC 633으로 확장)·Windows Terminal이 모두 지원하는 표준이다.

- **경쟁 근거:** Warp의 만족 1·2위가 "블록 단위 출력 관리"와 "명령/출력 분리 복사" [S18][S19]. 이 카테고리에서 유일하게 우리가 못 흉내 낸 UX 자산이다.
- **우리 현황:** 기반이 **이미 깔려 있다.** `src-tauri/src/native_terminal/sys/types.rs:227`에 `semantic_prompt_boundary` 플래그가 있고 `selection.rs`가 이를 셀 메타로 실어 나른다. 그런데 이 값을 소비하는 코드가 없다.
- **정리된 가치(이 둘만 진짜):** (1) **정확한 종료 알림** — `D` 마커가 exit code를 싣고 오므로, 플레인 셸 페인의 긴 빌드·테스트가 "exit 1로 끝났다"를 휴리스틱 없이 알릴 수 있다. 현재 알림은 `agent_detect` 화면 판독에만 의존하므로 경합이 아니라 보완이다. (2) **명령 출력만 잘라 에이전트에 전달** — 지금은 스크롤·드래그로 범위를 잡아야 한다. (3) 부수적: 프롬프트 점프·실패 블록 하이라이트. 모두 **플레인 셸 페인 한정**이고, 셸 통합 스니펫 설치(opt-in)가 전제다.
- **판단:** 플레인 셸 페인을 많이 쓰지 않으면 **건너뛰어도 된다.** 우선순위를 P1 끝으로 낮춘다.
- **주의:** Warp처럼 출력을 LLM에 자동 전송하지 않는다. 전송은 항상 사용자가 명시적으로 트리거한다 [S22].

### P1-C. 에이전트 상태 워치독 (T3 #4713 / #2343)

- **경쟁 근거:** 중단 버튼을 눌러도 오케스트레이터와 세션 상태가 디싱크돼 UI가 영원히 'working'으로 굳는다 [S48]. 며칠 뒤 돌아오면 대화 맥락이 사라진다 [S49].
- **중요 정정(2026-09-12):** 상태는 익스텐션 보고가 아니라 **화면 판독**으로 결정된다. `surface_host.rs:574`가 렌더 스냅샷의 행·타이틀을 `agent_detect::default_engine()`에 넣고, 11개 매니페스트(claude·codex·omo·opencode·cursor·gjc 등) 규칙을 전역 평가해 최상위 우선순위 매치를 채택한다. 매치가 없으면 `engine.rs:90`이 **이전 상태를 HOLD**하며, 이건 의도된 불변식이다(테스트 이름: `"absence of evidence must hold, never reset"`). 따라서 **시간 기반 타임아웃 강등은 틀린 처방** — 조용히 오래 일하는 에이전트를 멋대로 완료 처리한다.
- **세션 종료는 이미 처리됨:** `server.rs:1792`·`:2657`·`:2695`가 `agent_states.remove(session_id)`를 호출한다. 남는 구멍은 **세션은 살아 있고 안의 에이전트만 끝난 경우** — 셸 프롬프트로 돌아오면 어느 매니페스트도 매치하지 않아 HOLD가 마지막 상태(대개 Working)를 무기한 붙든다. 감지도 출력이 있을 때만 도는 트레일링 방식이라 새 출력이 없으면 재평가도 안 된다.
- **할 일(축소됨):** (1) **프로세스 증거 기반 해제** — PTY foreground 프로세스가 에이전트에서 셸로 돌아왔으면 그건 "증거 없음"이 아니라 종료의 적극적 증거다. 이때만 상태를 해제한다(HOLD 불변식과 충돌 없음). (2) 수동 "상태 초기화" 액션 하나. (3) ~~주기적 타임아웃 강등~~ — 철회. 별건: 세션별 트랜스크립트 디스크 보존(현재 `output_hub.rs:7` 세션당 512KiB, 초과 시 `ReplayGap`)은 독립 작업이며 계획 문서의 **A3 "과거 대화 검색·재개"**에 해당한다.

### P1-D. 죽은 코드 처리 결정 (대기 작업 인박스·모바일 제어)

§1-1에서 드러난 832줄은 방치하면 계속 "있는 기능"으로 오독된다(이번 분석에서 실제로 그러했다). 두 가지 중 하나를 골라야 한다.

- **배선:** `AttentionInbox`는 사이드바 활동 집계(`Sidebar.tsx`)가 이미 있으므로 "프로젝트를 가로지르는 단일 목록" 화면에 붙이기만 하면 된다. 상대적으로 싼. `ManagedChat`/`push`는 백엔드 라우트(`/api/push/subscribe`)가 있어 중간 비용.
- **삭제:** 지금 로드맵에 없다면 지우고 계획 문서에만 남긴다. 다음 사람(사람이든 에이전트든)이 같은 착각을 반복하는 비용을 없앤다.

중간 상태(코드는 있고 배선은 안 됨)를 그대로 두는 것만 안 된다.

---

## 4. P2 — 저비용 고효율 (주로 "이미 되는데 안 알리는 것")

### P2-A. 헤드리스 리눅스 서버 배포를 제품 기능으로 문서화

- **경쟁 근거:** Orca 사용자는 GUI 일체형 때문에 서버에 XFCE를 깔아야 했고, cmux는 macOS 전용이라 원격 SSH 제어 유스케이스가 아예 없다 [S44][S45]. 이 카테고리의 구조적 공백이다.
- **우리 현황:** `ferryx --daemon` + Axum 게이트웨이 + 원격 웹 클라이언트로 **이미 가능하다.** 그런데 `docs/`에도 `site/src/pages`에도 헤드리스/서버 설치 안내가 없다(검색 0건). 사이트 페이지는 `index.astro`/`404.astro`뿐.
- **할 일:** 설치 한 줄 + systemd 유닛 + 릴레이/포워딩 가이드 + "모니터 없는 빌드 서버에서 에이전트 돌리기" 유스케이스 페이지. 코드 변경 거의 없이 경쟁 우위 문구가 하나 생긴다.

### P2-B. 프라이버시 선언 페이지

- **경쟁 근거:** Warp는 설정을 다 꺼도 유지되는 TLS 연결 [S21]과 무단 LLM 전송 [S22]으로 기업 차단까지 갔고, T3는 끌 수 없는 프로세스 모니터로 같은 비판을 받았다 [S50]. 이 카테고리에서 신뢰는 실제 구매 요인이다.
- **우리 현황:** 앱에 텔레메트리가 없고 사이트 애널리틱스는 opt-in(`site/src/components/SiteAnalytics.astro`). 그런데 그걸 **아무 데서도 선언하지 않는다**(사이트에 privacy 페이지 없음).
- **할 일:** "기본 상태에서 나가는 네트워크 연결의 전체 목록"을 명시한 페이지 — 업데이터(GitHub releases), 사용자가 켠 릴레이/푸시, 그 외 없음. 검증 가능한 형태(패킷 캡처 재현 절차)로 쓰면 Warp 반례로 직접 작동한다.

### P2-C. 다중 원격 호스트 장시간 입력 지연 소크 테스트

- **경쟁 근거:** Herdr는 원격 5대 연결 후 몇 시간이면 타이핑이 고통스러워진다 [S32]. 우리는 SSH 원격 호스트를 최근에 들였으므로 **같은 함정 바로 앞에 서 있다.**
- **우리 현황:** 배칭 상한은 있다(`ipc/terminal.rs:21 BATCH_MAX_BYTES = 32KiB`, 10ms 코얼레싱). 그러나 다중 호스트·장시간 부하에서의 p95 키스트로크→에코 측정 기록이 없다.
- **할 일:** 원격 3~5대 × 수시간 소크 러너로 지연 분포를 수집하고, 호스트별 백프레셔를 검증. `docs/`의 스루풋 측정 방법론 재사용.

### P2-D. 선택 영역이 재드로우에 풀리지 않는지 회귀 테스트

- **경쟁 근거:** Herdr는 과도한 재드로우로 외부 터미널의 텍스트 선택을 계속 해제시켜 파워 유저 원성을 샀다 [S33].
- **우리 현황:** 네이티브 선택은 구현돼 있고(`native_terminal/selection.rs`, 드래그 선택 회귀 문서 존재) 렌더러는 행 재사용(`reused_rows`)을 한다. 대량 출력 중 선택이 유지되는지에 대한 명시적 테스트는 보이지 않는다.
- **할 일:** "초당 수천 줄 출력 중에도 기존 선택 범위가 유지된다" 회귀 테스트 1개.

---

## 5. 보류 (가치는 인정, 지금은 아님)

- **인라인 이미지(Kitty graphics / Sixel) [S16].** 벤더 ghostty는 `kitty_graphics: true`로 빌드되지만(`vendor/ghostty/src/terminal/build_options.zig:155`) 우리 WGPU 렌더러는 글리프 아틀라스만 그린다. 에이전트가 차트·스크린샷을 터미널에 뿌리는 워크플로가 늘면 가치가 오른다. 렌더러 작업량이 커서 지금은 아님.
- **폰트 리가처 [S15].** 요구는 실재하지만(이슈 1,400+ 찬성) 에이전트 워크로드에서의 실익은 낮다.
- **칸반형 태스크 보드 [S35].** Vibe Kanban의 만족 요인이나, 우리 DAG 뷰어 + 대기 작업 모아보기(A1)가 같은 문제를 우리 방식으로 푼다. 중복 투자 금지.
- (Design Mode 항목은 §3 P1-0으로 이동. "원격에는 이미 있다"는 초안의 서술은 **오류**였다 — 스텝과 미마운트 컴포넌트를 구현으로 오독했다.)

---

## 6. 채택 금지 목록 (경쟁사가 증명한 실패)

- **내장 코드 에디터를 만들지 않는다.** Orca는 느린 타이핑과 Vim 바인딩 부재로 "일상용으로 못 쓴다"는 평가를 받았다 [S12]. 에디터는 사용자의 에디터에 맡긴다.
- **로그인을 강제하지 않는다.** Warp의 초기 계정 강제는 철회 후에도 신뢰를 회복하지 못했다 [S20].
- **로컬 기능을 클라우드에 묶지 않는다.** Vibe Kanban은 폐업 직전 로컬 프로젝트까지 export-only로 잠갔다 [S37][S38]. 우리 릴레이는 언제나 opt-in이고, 릴레이가 죽어도 로컬은 100% 동작해야 한다.
- **끌 수 없는 백그라운드 수집기를 넣지 않는다** [S50].
- **에이전트 출력을 사용자 동의 없이 모델에 보내지 않는다** [S22].

---

## 7. 이 분석의 한계

- 입력 문서의 스타 수·저장소 경로·이슈 번호는 이 세션에서 재검증하지 않았다(네트워크 교차 확인 미수행). 따라서 **순위나 인용 자체를 근거로 쓰지 않았고**, 우리 저장소에서 확인한 코드·문서만 판단 근거로 삼았다.
- **초안의 P0 두 건은 모두 오판이었다.** 원인은 `FERRYX_MEMORY_FOOTPRINT_AUDIT_2026-09-05.md`만 읽고 그 뒤의 해결 기록(`FERRYX_METAL_OWNERSHIP_INVESTIGATION_2026-09-05.md` §4, 커밋 `29ea50be`)과 데몬 락/드레이닝 코드를 확인하지 않은 것. 교훈: "수정 코드가 안 보인다"는 미해결의 증거가 아니다 — 수정이 **의존성 업그레이드**로 이뤄지면 우리 저장소에는 흔적이 Cargo.toml 한 줄뿐이다.
- 남은 미확정: v30 이후의 IOSurface 증가율을 런타임으로 측정한 기록은 없다. 회귀가 의심될 때만 attach/detach 반복 측정을 하면 된다.
- 이 문서는 커밋되지 않은 작업 트리(다른 세션의 수정 포함) 위에서 작성됐다. 인용한 라인 번호는 `02e62e49` 기준이며 일부 파일은 로컬 수정 상태다.
