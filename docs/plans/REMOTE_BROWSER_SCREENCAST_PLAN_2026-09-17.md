# 원격 브라우저 화면 스트리밍 및 조작 구현 계획

**작성·개정일:** 2026-09-17  
**목표:** Ferryx 내장 브라우저의 실시간 화면 보기와 navigate / click / fill / keypress / eval을 기존 원격 웹·모바일 클라이언트에 제공합니다.  
**상태:** 참조 검증을 반영한 구현 계획입니다. 구현은 시작하지 않았으며, 제품 선택은 §8에 분리했습니다. 이번 산출물은 이 Markdown 한 파일이고 코드 변경·빌드·제품 테스트·커밋은 하지 않습니다.

**조사 기준:** Ferryx 최초 조사 기준은 요청된 `722fdf5c`입니다. 최초 조사 당시 HEAD `1f369e5f`의 browser/remote 및 지정 UI 경로는 해당 기준과 diff가 없었으며, 아래 Ferryx 인용은 실제 읽은 소스의 행 번호입니다. Orca는 사용자가 `aad41b1a4` 기준으로 제공한 읽기 전용 `.omo/upstream-orca/` 참조본을 대조했습니다. 인용에는 복사본 접두사를 붙이지 않고 **원래 저장소 상대 경로**를 사용합니다. 복사본에 별도 Git 이력 검증을 했다는 뜻은 아닙니다. 이전 세대의 참조 접근 문제는 해소되었으며, 형제 저장소나 부모 디렉터리에 접근하지 않았습니다.

## 1. 아키텍처 개요

### 1.1 확인한 현재 구조

| 현재 구현 | 설계에 반영할 경계 | Ferryx 근거 |
|---|---|---|
| WKWebView snapshot을 bitmap/PNG로 변환하여 파일에 저장하고 경로를 반환합니다. | 메모리 캡처 primitive만 재사용하고 원격 경로에 임시 파일·출력 경로를 만들지 않습니다. | `src-tauri/src/browser/screenshot.rs:62-190` |
| macOS 이외의 screenshot은 Unsupported입니다. | 플랫폼 공급자와 capability를 분리하고 미지원 기능을 성공으로 표시하지 않습니다. | `src-tauri/src/browser/screenshot.rs:193-204` |
| BrowserState에는 webviewLabel·worktreePath 등 내부 정보가 있습니다. | 외부 전용 DTO만 공개합니다. | `src-tauri/src/browser/model.rs:108-150` |
| CLI는 command/type 태그를 사용하며 click/fill/keypress는 Act의 action입니다. | 명령 이름과 enum을 구분하고 raw CLI forwarding을 금지합니다. | `src-tauri/src/ipc/browser_cli.rs:19-105`, `src-tauri/src/browser/model.rs:252-276` |
| browser CLI에는 별도의 로컬 capability credential이 있습니다. | GUI–daemon 인증에만 사용하고 원격에 전달하지 않습니다. | `src-tauri/src/ipc/browser_cli.rs:107-163` |
| GUI는 daemon 기반 gateway manager를 사용하지만 BrowserManager와 webview는 GUI 프로세스가 소유합니다. | Axum handler가 AppHandle을 직접 가진다고 가정하지 않습니다. | `src-tauri/src/lib.rs:912-925`, `src-tauri/src/ipc/remote.rs:23-53` |
| gateway는 auth·terminal backend·desktop selection·socket tickets를 소유합니다. | 별도 RemoteBrowserBackend가 필요합니다. | `src-tauri/src/remote/state.rs:706-748` |
| 디바이스에는 View/Control 및 Mirror/Machine 구분이 있습니다. | Machine scope를 desktop browser 공유 잠금의 우회권으로 사용하지 않습니다. | `src-tauri/src/remote/auth.rs:213-246` |
| WS는 Bearer로 발급한 30초 일회용 target-bound ticket을 사용합니다. | 영구 token을 URL에 넣지 않고 browser target을 기존 allowlist에 추가합니다. | `src-tauri/src/remote/server.rs:177-220`, `src-tauri/src/remote/server.rs:248-328` |
| relay에도 target allowlist와 terminal/events 전용 route가 있습니다. | direct gateway와 relay를 모두 확장해야 합니다. | `src-tauri/src/remote/relay_server.rs:1088-1096`, `src-tauri/src/remote/relay_server.rs:1150-1233` |
| RemoteApp mirror 영역은 RemoteTerminal을 렌더링합니다. | browser viewer는 별도 컴포넌트/WS로 추가하고 terminal framing은 유지합니다. | `ui/src/remote/RemoteApp.tsx:910-936`, `ui/src/main.tsx:17-61` |

### 1.2 구성과 데이터 경로

```text
원격 웹 / 모바일
  기존 PIN pairing → device capability token
  Bearer HTTP → 일회용 socket ticket
  browser 전용 WS [JSON control + binary image]
       │ direct 또는 기존 relay
       ▼
Axum gateway / daemon
  auth·revocation·desktop 공유 범위·rate limit
  RemoteBrowserBackend / bounded writer
       │ 인증된 로컬 browser IPC (영구 원격 token 전달 없음)
       ▼
Tauri GUI BrowserRemoteService
  실제 BrowserManager / browserId → native webview
  서비스 전체 단일 remote driver broker
  browser별 공유 capture producer
  main-thread takeSnapshot → 소유 픽셀/이미지 → 인코딩 → 최신 frame
```

**프레임 경로:** 구독·credit·공유 범위 확인 → GUI main thread에서 takeSnapshot 시작 → 메모리 이미지와 geometry 검증 → JPEG/PNG 인코딩 → 로컬 framed IPC → gateway의 구독자별 최신 슬롯 → WS → decode/draw → frame ACK입니다. WKWebView에는 CDP가 없으므로 `Page.startScreencast`, Chromium debugging port, CDP 세션을 도입하지 않습니다. 서버 캡처를 모바일 카메라/화면 공유로 대체하지도 않습니다.

**조작 경로:** 크기 제한된 JSON → 스키마/인증/Control/desktop 공유 범위 → driver lease·세대·중복 요청 확인 → 로컬 IPC → GUI broker의 실행 직전 재검증 → 기존 browser helper 또는 제한된 native input adapter → 크기 제한·경로 정제된 응답입니다. 페이지에 Tauri invoke 권한을 주지 않습니다.

`browserId`는 독립적인 브라우저 세션의 식별자입니다. terminal sessionId·leafId·tab index와 섞지 않습니다. **탭 전환 API, 탭 선택 순서, 원격 조작에 의한 자동 desktop 탭 전환은 설계하지 않습니다.** 기존 `identify`는 보이는 세션 중 가장 최근 생성된 세션을 고르는 의미를 유지합니다 (`src-tauri/src/ipc/browser.rs:858-883`).

### 1.3 gateway HTTP 및 WS 메시지 표

신규 HTTP 표면은 `GET /api/v1/browser/sessions?workspaceId=...&worktreeSlug=...`, `GET /api/v1/browser/identify`, ticket-authenticated WS `GET /api/v1/browser/{browserId}`입니다. 목록은 현재 desktop 공유 범위만 반환합니다. identify가 권한 밖의 세션을 가리키면 다른 세션으로 대체하지 않고 null을 반환합니다. `/api/v1/capabilities`에는 browserAvailable·지원 명령·인코딩·상한을 추가합니다. GUI가 없으면 BROWSER_UNAVAILABLE이며 terminal은 그대로 사용 가능합니다.

relay는 기존 `/host/{machineId}` 접두사와 `hostTransportUrl`/`remoteSocketUrl`을 재사용합니다 (`ui/src/remote/remoteClient.ts:3-34`). browser와 terminal은 서로 다른 WS를 사용합니다.

| 방향 | type 또는 binary kind | 의미 |
|---|---|---|
| S→C | `browserHello` | 인증 후 browserId·browserInstanceId·desktopEpoch·프로토콜/capability를 알립니다. |
| C→S | `browserSubscribe` | requestId·viewerInstanceId·품질 옵션으로 보기 구독을 요청합니다. |
| S→C | `browserSubscribed` | 서버 발급 subscriptionId·streamId·적용 품질·현재 세대를 반환합니다. 이 응답보다 frame이 앞서면 안 됩니다. |
| S→C | binary `0x62`, opcode `1` | 한 장의 완전한 JPEG/PNG입니다. JSON base64가 아닙니다. |
| C→S | `browserFrameAck` | 실제 표시한 streamId·seq를 확인합니다. 제어권을 부여하지 않습니다. |
| C→S / S→C | `browserHeartbeat` / `browserPong` | 구독 생존을 확인하고 현재 소유자의 유효 lease만 갱신합니다. |
| C→S | `browserDriverClaim` | 유효한 구독과 Control 권한으로 단일 제어권을 요청합니다. |
| S→C | `browserDriverClaimed` / `browserDriverChanged` | leaseEpoch·만료 또는 viewer 상태 전환을 알립니다. |
| C→S / S→C | `browserDriverRelease` / `browserDriverReleased` | 자신의 lease를 해제합니다. 반복 해제도 안전합니다. |
| C→S | `browserCommand` | command·params·requestId·requestSeq·leaseEpoch·세대를 전달합니다. |
| S→C | `browserResult` / `browserError` | 실행 결과 또는 구조화된 code·message·retryable을 반환합니다. 수신 확인과 실행 완료를 혼동하지 않습니다. |
| S→C | `browserState` | 정제한 URL/제목·문서 세대·geometry 세대·loading·paused 원인을 전달합니다. |
| C→S / S→C | `browserUnsubscribe` / `browserUnsubscribed` | 취소 경계 설치 후 종료를 확인합니다. 이전 구독 frame은 더 이상 표시하지 않습니다. |

## 2. Orca 참조 설계 요약

### 2.1 실제 소스와 대조한 canonical file:line 인용

아래 O01–O21은 참조본에서 직접 확인한 서로 다른 코드 구간입니다. 코드의 관찰 사실과 Ferryx에서 추가할 정책을 분리합니다.

| 근거 | 확인한 Orca 설계 | Ferryx 적용 |
|---|---|---|
| O01 `mobile/src/transport/browser-screencast-protocol.ts:1-20` | kind 0x62, version 1, header 16B, Frame=1, JPEG/PNG입니다. | 동일 기본 image envelope를 사용합니다. |
| O02 `mobile/src/transport/browser-screencast-protocol.ts:22-39` | metadata에 offsetTop, pageScaleFactor, device/image 크기, scrollOffsetX/Y, timestamp가 있습니다. | 같은 이름을 유지하되 WK snapshot의 좌표 출처를 명시합니다. |
| O03 `mobile/src/transport/browser-screencast-protocol.ts:80-118` | format은 byte 3, seq는 offset 4, metadataLength는 offset 8의 u32LE이고 offset 12는 예약값 0입니다. | §4.2에서 실제 배치를 사용하고 크기·픽셀 검증을 추가합니다. |
| O04 `src/main/runtime/runtime-browser-screencast-controller.ts:76-85` | 같은 connection의 이전 stream을 cancel한 뒤 done을 기다립니다. | 교체/재접속 경쟁을 직렬화합니다. |
| O05 `src/main/runtime/runtime-browser-screencast-controller.ts:116-126` | ready 이전 binary는 false로 거부하고 시작 중에도 connection에 등록합니다. | 준비 전 frame 공개를 차단합니다. |
| O06 `src/main/runtime/runtime-browser-screencast-controller.ts:138-159` | page subscriber 등록, mobile driver 설정, cleanup 등록, ready emit 후 pending frame flush 순서입니다. | ready 순서를 유지하되 driver는 별도 claim으로 분리합니다. |
| O07 `src/main/runtime/runtime-browser-screencast-controller.ts:160-191` | finally에서 abort listener·connection/page subscriber를 정리하고 현재 mobile 소유자의 해제 후 driver를 재계산합니다. | 모든 종료 경로가 하나의 cleanup으로 모이도록 합니다. |
| O08 `src/main/runtime/remote-browser-screencast-frame-admission.ts:3-14` | 크기 초과 frame을 handled=true로 폐기하여 동일 oversized frame의 무한 재시도를 피합니다. | permanent drop과 transient backpressure를 별도 결과로 구분합니다. |
| O09 `src/main/runtime/browser-screencast-driver-scope.ts:12-34` | clientKind가 mobile일 때만 drivesAsMobile이며 남은 mobile subscriber 또는 idle로 복귀합니다. | 단순 구독을 제어 권한으로 간주하지 않는 원칙을 사용합니다. |
| O10 `src/main/runtime/browser-screencast-ghost-subscriber-eviction.ts:12-42` | 한 번 이상 전달된 subscriber에서 연속 90회 거부가 누적되면 ghost로 판정합니다. 한 번도 전달되지 않은 pre-ready 상태는 heartbeat에 맡깁니다. | 저속 polling에 90회 상수를 그대로 쓰지 않고 시간 기반 stall과 준비 timeout을 분리합니다. |
| O11 `src/main/runtime/runtime-browser-commands-browser-screencast.ts:55-96` | page별 단일 producer를 공유하고 느린 viewer마다 최신 pendingFrame을 유지하여 다른 viewer를 막지 않습니다. | browser별 한 capture/encode와 구독자별 latest-only 슬롯을 둡니다. |
| O12 `src/main/runtime/runtime-browser-commands-browser-screencast.ts:126-164` | 새 subscriber 등록 후 동일 pairedDeviceId의 기존 subscriber를 제거하여 교체 중 page stream이 비지 않게 합니다. | 인증된 deviceId+viewerInstanceId로 교체하되 lease를 자동 승계하지 않습니다. |
| O13 `src/main/runtime/runtime-browser-commands-browser-click.ts:142-174` | explicit stop·ghost eviction·same-device replacement가 같은 leave 경로를 사용하고 마지막 subscriber에서 session을 정지합니다. | 취소 경계·queue·lease·마지막 producer 종료를 공통화합니다. |
| O14 `src/main/runtime/runtime-browser-commands-browser-command-target-params.ts:98-121` | viewport/budget를 정규화하며 기본 quality 70, 1440×1200 cap, everyNthFrame 2 등을 사용합니다. | quality 개념만 참고하고 polling·해상도 기본값은 WKWebView용으로 따로 정합니다. |
| O15 `src/main/runtime/runtime-rpc/runtime-rpc-mobile-method-allowlist.ts:15-31` | mobile에는 back/dialog/goto/keyboard/mouse/reload/screencast/unsubscribe/tabCreate/viewport가 명시적으로 허용됩니다. | runtime 명령 전체를 remote에 노출하지 않습니다. eval은 Ferryx 별도 요구이며 이 allowlist에 포함되어 있다고 해석하지 않습니다. |
| O16 `src/main/runtime/runtime-browser-commands-browser-click.ts:24-67` | click/goto/fill은 공통 target 해석 후 bridge로 보내고 navigation 이후 URL/제목을 갱신합니다. | 명령 helper를 재사용하고 프레임과 상태의 문서 세대를 일치시킵니다. |
| O17 `src/main/runtime/runtime-browser-commands-active-screencasts-by-page-id.ts:103-156` | 명시적인 page ID를 우선하며 등록된 살아 있는 guest인지 검증합니다. | browserId의 실제 세션 생존을 검사하되 upstream의 활성화/탭 모델은 이식하지 않습니다. |
| O18 `src/main/runtime/runtime-browser-commands-state.ts:1-6` | activeScreencastsByPageId는 command state의 공유 Map입니다. | frame 생산 상태를 WS마다 중복 생성하지 않습니다. |
| O19 `src/main/runtime/runtime-browser-commands-browser-screencast.ts:195-206` | browserEval은 target 해석 후 bridge evaluate를 호출합니다. | Ferryx의 기존 eval helper와 64KiB 제한을 사용합니다. |
| O20 `src/main/runtime/runtime-browser-driver-controller.ts:17-43` | driver state는 page별이며 desktop reclaim 시 desktop 상태 설정과 screencast 취소가 연결됩니다. | desktop 회수 우선권을 유지하면서 Ferryx의 서비스 전체 단일 remote lease로 강화합니다. |
| O21 `src/main/runtime/runtime-edge-command-controller.ts:30-81` | runtime surface에는 snapshot/eval/파일·탭·입력 등 mobile보다 넓은 명령 집합이 존재합니다. | 넓은 runtime surface와 mobile allowlist를 동일시하지 않습니다. |

### 2.2 그대로 가져오지 않을 부분

Orca의 **page별 mobile presence**, 모바일 구독 시 driver 설정, 남은 mobile subscriber로의 fallback을 Ferryx의 요구와 혼동하지 않습니다. Ferryx는 GUI browser service 전체에서 **한 remote driver만** 허용하고, 보기 구독과 명시적인 claim/release를 분리하며 다른 viewer에게 자동 승계하지 않습니다. 이 차이는 O06·O09·O20에 근거한 의도적인 강화입니다.

Orca의 viewport emulation/탭 관리, CDP 캡처 방식은 이식하지 않습니다. 위에서 읽은 transport/controller 코드만으로 WKWebView가 CDP를 지원한다고 추론할 수 없습니다. 제공된 CLI 보조 문서의 snapshot→act→snapshot 루프와 tab/capture 명령도 검토했지만, 문서의 원래 저장소 경로가 별도로 주어지지 않아 코드 인용 수에는 포함하지 않았습니다. tab switch --index의 의미는 Ferryx 설계 범위에 추가하지 않습니다.

기본 16B envelope는 O01–O03과 맞추되, Ferryx identity·lease·ACK·RPC 계약은 별도 확장입니다. **Orca mobile 클라이언트와 전체 상호운용성이 검증되었다고 주장하지 않습니다.**

## 3. 단계별 구현

### 3.1 파일 소유권 원칙

아래 write scope는 **향후 구현의 파일별 배타적 소유권**입니다. 동일 파일은 정확히 한 단계만 소유합니다. 다른 단계는 read-only로 사용하며 공통 등록 파일은 Phase 4만 수정합니다. 이후 오류가 발견되어도 다른 단계가 해당 파일을 덮어쓰지 않고 원래 소유 단계의 작업으로 돌립니다. 등록·통합은 필요한 모듈/테스트가 준비된 뒤 Phase 4에서 마무리하므로 선행 파일 작성만으로 빌드가 완료되었다고 보지 않습니다. 이번 작업의 실제 write scope는 Phase 0의 문서 하나입니다.

### Phase 0 — 문서 및 계약 확정

**Write scope:** `docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md`.

Orca 인용, binary 배치, capability/권한, 소유권 경계를 확정합니다. §8의 선택에는 초기 기본안을 제시하며 제품 선택을 참조 접근 blocker로 취급하지 않습니다. **완료 조건:** 9개 섹션, 12건 이상 canonical 인용, 서로 겹치지 않는 파일 소유권입니다.

### Phase 1 — backend: 메모리 snapshot primitive

**Write scope:** `src-tauri/src/browser/screenshot.rs`, `src-tauri/src/browser/snapshot_source.rs`.

BrowserSnapshotSource를 정의하고 기존 takeSnapshot/PNG 경로에서 메모리 캡처를 분리합니다. 기존 take_browser_screenshot 함수는 같은 primitive를 사용하되 PNG 파일 저장·반환 경로 계약을 유지합니다. wrapper의 blocking 파일 I/O는 기존 run_blocking 규칙을 따릅니다. 원격 streaming은 wrapper를 호출하지 않습니다.

native 객체 접근은 main thread에 한정하고 worker로 넘기는 것은 소유권이 명확한 Rust 바이트/픽셀입니다. Objective-C 객체를 임의로 Send 처리하지 않습니다. 기존 요청별 OS thread 생성도 폴링 루프에 복제하지 않습니다 (`src-tauri/src/browser/screenshot.rs:149-190`). 타 플랫폼 공급자는 Unsupported를 반환합니다. 기존 의존성으로 가능한 경로를 우선하며 새 Cargo 의존성이 필요하면 이 계획의 범위 확대를 먼저 승인받습니다.

**완료 조건:** native 오류·빈 이미지·timeout·late callback·webview 소멸을 fake source로 검증할 수 있고 기존 PNG 저장 계약이 유지됩니다. frontend 파일은 수정하지 않습니다.

### Phase 2 — backend: GUI browser 서비스·조작·driver

**Write scope:** `src-tauri/src/browser/remote_service.rs`, `src-tauri/src/browser/remote_driver.rs`, `src-tauri/src/browser/remote_input.rs`, `src-tauri/src/browser/remote_bridge_protocol.rs`, `src-tauri/src/browser/manager.rs`, `src-tauri/src/browser/model.rs`, `src-tauri/src/browser/guest.rs`.

실제 BrowserManager를 사용하는 BrowserRemoteService, browser별 공유 producer, 전역 단일 remote lease, 로컬 framed IPC 계약, 제한된 native 입력 adapter를 구현합니다. guest.rs 변경은 필요한 viewport 관측 부분만 허용하고 nonce/isTrusted 보호는 유지합니다. desktop 공유 inventory/epoch는 GUI 실제 가시성·workspace/worktree에서 만들며 원격 클라이언트가 선언하지 못합니다.

local bridge는 service epoch·connection·browser instance·권한 철회 수명을 소유합니다. snapshot의 기존 target map은 같은 document generation에서도 교체될 수 있으므로 remote snapshotId를 별도로 두고 immutable target map 또는 map revision을 검증합니다 (`src-tauri/src/browser/manager.rs:331-380`). 기존 helper의 공개·Tauri 등록 변경은 Phase 4에 요청합니다.

**완료 조건:** 구독 0개이면 캡처 0개, 서비스 전체 driver 1명, 오래된 lease/instance/snapshot/viewport 입력 거부, point 입력과 reference 입력의 명시적 구분입니다. frontend 파일은 수정하지 않습니다.

### Phase 3 — backend: gateway transport·admission·security

**Write scope:** `src-tauri/src/remote/browser_backend.rs`, `src-tauri/src/remote/browser_protocol.rs`, `src-tauri/src/remote/browser_ws.rs`, `src-tauri/src/remote/browser_admission.rs`, `src-tauri/src/remote/browser_security.rs`.

RemoteBrowserBackend의 local IPC 및 Unavailable 공급자, public DTO/codec, HTTP/WS handler, rate limit·ticket 연계·권한 검사, latest-only writer를 구현합니다. in-process 테스트 공급자를 둘 수 있지만 실제 daemon→GUI 경계를 생략하지 않습니다. shared protocol은 §4를 따르고 내부 타입을 raw 직렬화하지 않습니다.

WS read, control writer, frame writer, revocation, heartbeat는 하나의 연결 수명에 속합니다. 긴 eval/wait는 bounded dispatcher에서 실행하여 release/heartbeat/철회 처리를 막지 않습니다. 크기 초과 frame은 drop, 일시적 queue 포화는 backpressured, 닫힌 연결은 closed로 구분합니다. terminal 코덱·writer는 건드리지 않습니다.

**완료 조건:** fake backend와 socket으로 인증/세대/크기/종료 계약을 검증할 수 있고 느린 viewer 하나가 다른 viewer를 막지 않습니다. 기존 등록 파일과 frontend는 수정하지 않습니다.

### Phase 4 — backend: 실제 프로세스·gateway·relay 등록 통합

**Write scope:** `src-tauri/src/browser/mod.rs`, `src-tauri/src/remote/mod.rs`, `src-tauri/src/remote/state.rs`, `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/relay_server.rs`, `src-tauri/src/ipc/browser.rs`, `src-tauri/src/ipc/browser_cli.rs`, `src-tauri/src/lib.rs`.

기존 CLI endpoint에 로컬 credential로 인증하는 remoteAttach handshake를 추가합니다. 기존 JSON 한 줄 요청/응답은 유지하고 **새 handshake가 성공한 연결만** `u32LE payloadLength + 1B contentType + payload` framed 모드로 전환합니다. 길이는 contentType 뒤 payload의 길이로 정의하며 JSON은 512KiB, image는 2MiB 상한을 적용합니다. endpoint/credential 발견과 파일 읽기는 blocking adapter를 사용합니다. remote token과 내부 경로는 로컬 서비스의 public 결과에 포함하지 않습니다.

GUI bootstrap·owner revoke 명령·기존 helper 재사용을 연결하고 gateway state에 backend를 주입합니다. GUI 재시작 시 service epoch를 변경하여 예전 connection/lease를 폐기합니다. gateway/relay 각각의 ticket target allowlist와 browser WS route를 등록합니다. relay의 기존 text/binary 전달은 재사용하되 전역 크기 제한을 무조건 상향하지 않습니다 (`src-tauri/src/remote/relay_server.rs:1276-1318`).

이 단계만 production/test 모듈 등록을 소유합니다. 신규 모듈을 다시 작성하지 않습니다. **완료 조건:** direct 및 relay 양쪽에서 GUI 소유 browser까지 도달하고, GUI 종료 시 browser만 unavailable이 되며, 기존 CLI/terminal/PIN 인증을 보존합니다. frontend 파일은 수정하지 않습니다.

### Phase 5 — frontend: transport·viewer

**Write scope:** `ui/src/remote/browserProtocol.ts`, `ui/src/remote/browserClient.ts`, `ui/src/remote/useRemoteBrowser.ts`, `ui/src/remote/RemoteBrowser.tsx`.

codec·typed request client·구독 수명·ready gate·decode/draw를 구현합니다. remoteSocketUrl은 기존 함수를 read-only로 재사용합니다. RemoteBrowser는 화면과 상태를 렌더링하는 컴포넌트이며 입력 callback/controls slot을 받아 Phase 6에서 조합할 수 있게 합니다. 저수준 client는 명세 전체를 표현하되 자동 claim을 하지 않습니다.

decode 완료 순서가 바뀌면 이전 seq를 폐기합니다. bitmap/object URL은 교체·unmount 때 해제하고 background·host 변경 시 frame buffer를 비웁니다. service worker나 persistent cache에 저장하지 않습니다. **완료 조건:** 보기 전용 연결·재접속·paused/stale/unsupported UI와 메모리 해제를 검증할 수 있습니다. backend와 기존 앱 통합 파일은 수정하지 않습니다.

### Phase 6 — frontend: 제어 UX·기존 앱 통합·owner 회수

**Write scope:** `ui/src/remote/useRemoteBrowserDriver.ts`, `ui/src/remote/RemoteBrowserControls.tsx`, `ui/src/remote/RemoteBrowserWorkspace.tsx`, `ui/src/remote/RemoteApp.tsx`, `ui/src/components/RemoteBrowserSharingIndicator.tsx`, `ui/src/App.tsx`, `ui/src/lib/tauri.ts`.

Phase 5의 client/viewer를 composition으로 연결하고 View/Control/다른 driver 점유 상태를 구분합니다. 사용자의 명시적 claim 이후만 입력을 전송하며 주소 입력·reference 선택/fill·허용 keypress·고급 eval UI를 제공합니다. point click은 해당 capability가 있을 때만 제공합니다. 모바일 IME는 composition 중간값을 키와 텍스트로 중복 전송하지 않고 확정 문자열을 제한된 fill 경로로 보냅니다.

desktop에는 공유 표시와 즉시 회수 버튼을 연결합니다. remote native input을 local 사용자 입력으로 오인하지 않도록 출처를 구별합니다. terminal swipe/tab 처리와 browserId targeting을 섞지 않습니다. **완료 조건:** 전체 보기→claim→조작→release 흐름, owner 회수, 재접속 후 mutation 자동 replay 금지입니다. Phase 5 소스와 backend는 수정하지 않습니다.

### Phase 7A — backend 검증 코드

**Write scope:** `src-tauri/src/browser/snapshot_source_tests.rs`, `src-tauri/src/browser/remote_service_tests.rs`, `src-tauri/src/remote/browser_protocol_tests.rs`, `src-tauri/src/remote/browser_security_tests.rs`, `src-tauri/src/remote/browser_lifecycle_tests.rs`.

§7의 mock/codec/IPC/driver/race 검증을 작성합니다. 테스트 모듈 등록은 Phase 4 소유입니다. 기존 ipc/tests.rs 또는 다른 세션의 테스트 파일을 수정하지 않습니다. **완료 조건:** 명세 경계와 오류 경로를 구현 단계에서 실제 실행하여 확인합니다. frontend 파일은 수정하지 않습니다.

### Phase 7B — frontend 검증 및 사용자 QA

**Write scope:** `ui/src/remote/browserProtocol.test.ts`, `ui/src/remote/RemoteBrowser.test.tsx`, `ui/src/remote/RemoteBrowser.mobile.test.tsx`, `ui/src/remote/RemoteBrowser.reconnect.test.tsx`.

§7의 UI/모바일/재접속 검증과 사용자 QA를 수행하고 §5 초기값의 실측 근거를 수집합니다. backend 및 문서를 이 단계에서 편의상 수정하지 않습니다. **완료 조건:** 지원 환경에서 두 기능의 end-to-end 흐름이 확인되고 미지원 capability가 명확합니다. 문서 갱신은 Phase 0 소유 작업으로만 처리합니다.

## 4. 와이어 프로토콜 명세

### 4.1 camelCase 및 identity

신규 JSON은 type 태그, command 이름, variant 필드까지 camelCase이며 deny_unknown_fields와 enum별 validation을 적용합니다. legacy CLI의 snake_case alias를 public DTO에 상속하지 않습니다. WS path의 browserId는 연결 동안 고정하며 payload와 다르면 거부합니다.

public endpoint는 workspaceId/worktreeSlug/browserId만 사용합니다. 내부 path·cwd·worktreePath·outPath·filePath·webviewLabel 필드는 없습니다. ID 길이/문자 집합을 server-issued catalog에 맞추고 slash·backslash·제어문자·dot-segment·중복 decoding을 거부합니다. worktreeSlug는 서버의 WorktreeIdentity/registry로 해석합니다.

browserInstanceId는 실제 browser 재생성, browserServiceEpoch는 GUI 재시작, desktopEpoch는 공유 권한 수명, documentGeneration은 문서, viewportRevision은 좌표 변환, snapshotId는 DOM target map을 구분합니다. u64 값은 JS 정밀도 손실을 막기 위해 10진 문자열로 보냅니다. deviceId/permission은 인증 결과에서 얻으며 클라이언트 주장으로 갱신하지 않습니다.

```json
{"type":"browserSubscribe","requestId":"r1","viewerInstanceId":"v1","options":{"format":"jpeg","quality":70,"intervalMs":250,"maxEdge":1280}}
```

```json
{"type":"browserSubscribed","requestId":"r1","subscriptionId":"s1","streamId":1,"browserId":"b1","browserInstanceId":"bi1","browserServiceEpoch":"3","desktopEpoch":"8","documentGeneration":"12","options":{"format":"jpeg","quality":70,"intervalMs":250,"maxEdge":1280}}
```

```json
{"type":"browserCommand","requestId":"r2","requestSeq":"1","browserId":"b1","leaseEpoch":"31","browserInstanceId":"bi1","desktopEpoch":"8","documentGeneration":"12","command":"fill","params":{"snapshotId":"snap1","reference":"e3","value":"검색어"}}
```

ID 예시는 credential이 아닙니다. browserHello에서 protocolVersion=1 및 지원 명령을 알립니다. 메시지 성공 응답은 requestId를 되돌리며 browserError는 고정 code/message, retryable, 필요한 경우 retryAfterMs를 제공합니다. 내부 error 문자열을 그대로 직렬화하지 않습니다.

### 4.2 실제 Orca 16B envelope와 Ferryx 확장

기본 배치는 `mobile/src/transport/browser-screencast-protocol.ts:80-118`에 맞춥니다. 이전 초안의 streamId/sequence/metadataLength를 각각 offset 4/8/12에 넣는 배치는 사용하지 않습니다.

| offset | 길이 | 값 |
|---|---:|---|
| 0 | 1 | kind=0x62 |
| 1 | 1 | envelopeVersion=1 |
| 2 | 1 | opcode=1, Frame |
| 3 | 1 | format: 1=jpeg, 2=png |
| 4 | 4 | seq: u32 little-endian |
| 8 | 4 | metadataByteLength: u32 little-endian |
| 12 | 4 | reserved=0 |
| 16 | metadataByteLength | UTF-8 JSON metadata |
| 16+metadataByteLength | 나머지 | 완전한 JPEG/PNG image |

한 binary WS message가 한 frame입니다. delta/video codec/base64는 v1에 없습니다. format은 헤더가 authoritative하며 metadata에 중복 format을 넣지 않습니다. **streamId는 헤더가 아니라 metadata 확장 필드**입니다. reserved 비트에 Ferryx 의미를 몰래 넣지 않습니다.

기본 metadata 이름은 offsetTop, pageScaleFactor, deviceWidth, deviceHeight, imageWidth, imageHeight, scrollOffsetX, scrollOffsetY, timestamp입니다 (`mobile/src/transport/browser-screencast-protocol.ts:22-39`). Ferryx 확장은 streamId, browserInstanceId, browserServiceEpoch, desktopEpoch, documentGeneration, viewportRevision, captureRect, geometrySource="wkSnapshot"입니다. 필수 확장 필드가 없는 frame은 Ferryx client에서 거부합니다. upstream decoder가 확장 필드를 읽지 않는 것과 전체 RPC 상호운용성은 별개입니다.

imageWidth/Height는 인코딩 이미지의 실제 픽셀입니다. deviceWidth/Height와 captureRect는 이 프로파일에서 native view logical 좌표로 정의합니다. offsetTop은 browser 내용만 캡처하면 0입니다. pageScaleFactor/scrollOffsetX/Y는 관측 가능할 때만 제공하며 CDP에서 받은 값이라고 표시하지 않습니다. page JS가 반환한 수치는 비신뢰 입력으로 검증하고 권한 판정에 사용하지 않습니다. timestamp는 host Unix seconds이고 TTL은 별도 monotonic clock으로 계산합니다.

최소 16B → kind/version/opcode/format/reserved → checked length → metadata≤4KiB 및 JSON 타입/finite 값 → frame≤2MiB → 비어 있지 않은 image 및 이미지 header의 크기 일치/4MP 이하 → decode 순서로 검증합니다. 각 변은 2048px 이하입니다. decoder 앞에서 검사하되 native capture 할당 이전에도 제한합니다. client→server binary는 거부하며 terminal parser로 image를 해석하지 않습니다.

seq는 stream별 증가하며 건너뛸 수 있지만 역전/중복은 표시하지 않습니다. u32 wrap 전 기존 구독을 종료하고 새 stream을 만듭니다. streamId는 해당 socket 내에서 재사용하지 않습니다. socket 자체의 connection epoch도 검사하여 다른 연결에서 다시 사용된 숫자와 혼동하지 않습니다.

### 4.3 subscribe/unsubscribe·ACK·stale eviction

수명은 `opening → ready → streaming/paused → closing → closed`입니다. subscribe 처리 전에 auth·지원 플랫폼·desktop 공유 범위·자원 예산을 검사하고, subscriber를 등록한 뒤 browserSubscribed를 같은 writer에서 전송한 후 frame gate를 엽니다. 시작 중 취소와 ready 이전 pending frame을 처리하는 순서는 O04–O07을 따릅니다.

browser별 native producer는 하나입니다. v1은 browser별 단일 협상 프로파일이며 첫 구독의 허용 프로파일을 고정하고 후속 viewer에게 적용값을 알립니다. 더 높은 화질 요구를 무조건 반영하지 않습니다. 옵션 변경은 재협상/새 stream 경계로 처리합니다. 캡처/encode는 공유하고 subscriber별 identity envelope만 별도로 붙일 수 있습니다.

동일한 `(인증된 deviceId, viewerInstanceId, browserId)`의 새 구독은 이전 viewer를 교체합니다. 새 subscriber를 예약한 뒤 기존 것을 정리하되 frame 공개 전에 교체를 확정합니다. viewerInstanceId는 device 내에서만 효력이 있고 ID/개수 상한이 있습니다. Orca의 pairedDeviceId 단독 교체(O12)와 달리 동일 기기의 서로 다른 창을 구분합니다. 이전 driver lease는 승계하지 않습니다.

viewer별 미확인 전송 1개와 최신 pending 1개만 유지합니다. browserFrameAck는 현재 stream에서 **서버가 실제로 보낸 seq**의 표시 완료만 인정합니다. 미래 seq·다른 stream ACK는 credit이나 lease를 갱신하지 않습니다. decode 실패는 성공 ACK 대신 오류/재구독으로 처리합니다. 프레임이 생성되지 않은 idle 상태는 ACK stall이 아닙니다.

초기 heartbeat 간격은 5초, 구독 TTL은 마지막 유효 heartbeat 이후 15초, sweeper는 2초입니다. ready가 10초 안에 완료되지 않으면 시작을 취소합니다. **미확인 frame이 실제 존재할 때만** 전송 시점부터 10초 ACK-progress deadline을 적용합니다. send false 자체나 ready 전 거부를 ghost로 계산하지 않습니다. Orca의 delivered-once/90회 원칙(O10)은 참고하되 polling 속도에 종속된 90회 기준을 복사하지 않습니다. 명령 실행 시에는 sweeper를 기다리지 않고 TTL을 직접 검사합니다.

unsubscribe, socket close/error, auth 철회, desktop 공유 범위 이탈, GUI/bridge 종료, browser 소멸, TTL/stall은 동일한 idempotent cleanup을 호출합니다. 먼저 admission/lease epoch를 무효화하고 대기 명령을 취소한 뒤 queue·subscriber를 제거합니다. 마지막 subscriber가 없으면 즉시 폴링을 멈춥니다. browserUnsubscribed 뒤 이전 frame을 enqueue하지 않고 client도 이미 전송 중이던 이전 frame을 폐기합니다.

늦은 native callback은 구독/instance/권한 epoch가 유효할 때만 공개합니다. background/unmount에서 client는 unsubscribe 및 buffer 해제를 수행하며 전달 실패는 TTL이 정리합니다. 재연결은 새 ticket·subscription·connection epoch이며 mutation과 제어권을 자동 복원하지 않습니다.

### 4.4 driver-claim/release 및 명령 의미

GUI 서비스 전체에 하나인 lease는 `(deviceId, connectionId, subscriptionId, browserId, leaseEpoch, expiresAt)`에 바인딩됩니다. Control+유효 구독으로만 claim 가능하며 빈 lease의 원자적 획득만 허용합니다. 점유 중에는 BROWSER_DRIVER_BUSY입니다. TTL은 15초이고 현재 owner의 인증된 heartbeat만 연장합니다. frame ACK·View·Machine scope는 조작 권한이 아닙니다. release/unsubscribe/만료/철회/desktop 회수 시 해제하고 다른 viewer를 자동 driver로 만들지 않습니다.

| command | 매핑/의미 | 필요한 추가 조건 |
|---|---|---|
| getState | 정제한 원격 상태 DTO | View 및 공유 범위 |
| snapshot | DOM automation snapshot, image와 별개 | View 이상, remote snapshotId·크기 상한 |
| navigate | navigate_browser_session | Control+lease, URL allowlist·현재 세대 |
| back/forward/reload | 기존 navigation helper | Control+lease, 고정 browserId |
| click | reference 또는 명시적인 point variant | Control+lease, snapshotId 또는 frame/viewport fencing |
| fill | 기존 DOM target fill | Control+lease, snapshotId·입력 크기 |
| keypress | 기존 parser/플랫폼 helper | Control+lease, page-scoped 키 allowlist |
| eval | eval_browser_session | Control+lease+owner 동의, 결과 64KiB |
| wait | 기존 condition polling | Control+lease, function condition에는 eval 동의도 필요 |

Open/Close/Focus, 파일 screenshot 저장, cookies/storage, upload/download, arbitrary CLI passthrough, app chrome shortcut은 v1 remote allowlist에 없습니다. CLI 지원 목록을 그대로 remote에 노출하지 않습니다. 화면 직접 click이 비지원이어도 reference click/fill/keypress는 해당 capability로 제공하되 지원 범위를 명확히 표시합니다.

requestSeq는 연결 내 증가하는 10진 정수이며 requestId와 함께 중복을 추적합니다. 실행 중 중복은 합치고 결과 캐시는 16건·총 1MiB·30초 중 먼저 도달한 상한을 적용합니다. 캐시가 사라진 이전 requestSeq는 재실행하지 않고 BROWSER_OUTCOME_UNKNOWN을 반환합니다. 새 연결에서 mutation을 자동 재전송하지 않습니다. queue 안에서는 실행 직전 lease를 재확인하며, 이미 시작한 native/JS 부작용을 disconnect나 timeout으로 되돌릴 수 있다고 보장하지 않습니다.

### 4.5 좌표·문서·키 입력

client는 letterbox를 제외한 실제 이미지 영역에서 normalized point `(u,v)`를 구하고 표시한 streamId/seq·documentGeneration·viewportRevision을 함께 보냅니다. 서버는 해당 전송 frame의 captureRect에서 `x=rect.x+u*rect.width`, `y=rect.y+v*rect.height`를 계산하여 native 원점/배율을 한 번만 적용합니다. native viewport 좌표에 scrollOffset을 다시 더하지 않습니다. 범위 밖/NaN/Infinity, 2초보다 오래된 frame, resize/zoom/scroll로 바뀐 viewportRevision은 거부하고 새 frame을 요구합니다.

geometry 측정과 capture 사이의 navigation/geometry 변경은 결과 폐기로 처리합니다. 동적인 DOM animation까지 완전히 원자적인 hit-test를 보장하는 것은 아닙니다. frame별 geometry 기록은 보낸 outstanding/latest frame에 한정해 bounded하게 보관합니다.

reference형은 documentGeneration과 immutable snapshotId/map revision을 함께 검사합니다. reference와 point를 몰래 대체하지 않습니다. 단순 element.click()이 native trusted click과 같다고 주장하지 않고, native 입력도 webview 내부로 제한하며 OS 전체 입력 주입을 사용하지 않습니다. cross-origin iframe/canvas/native dialog는 capability와 실제 QA 결과에 따라 제한합니다. remote keypress로 탭 관리·팔레트·앱 종료·OS clipboard를 조작하지 못하게 합니다.

## 5. 성능 전략

아래 값은 측정 결과가 아닌 **초기 프로파일과 hard cap 제안**입니다. takeSnapshot polling 간격은 실제 FPS/입력 지연을 보장하지 않습니다. timer tick을 밀린 만큼 몰아서 처리하지 않습니다.

| 항목 | 초기안 | trade-off / 제한 |
|---|---|---|
| 기본 polling | 250ms, 최대 4회/초 목표 | 개발 페이지/폼의 반응성과 CPU·배터리 비용을 절충합니다. |
| 조작 직후 | 짧게 150ms, 절대 최소 125ms | 최대 8회/초 목표이며 지속 고속 캡처는 피합니다. |
| idle/background | idle 1초, background/구독 0개는 중지 | 변경 감지는 비용이 있으므로 생략한 캡처만 배터리 절감으로 계산합니다. |
| 기본 encoding | JPEG quality 70, 긴 변 1280px | 작은 frame을 우선하고 작은 글자의 손실은 QA로 판단합니다. |
| PNG | 명시적 lossless 모드 | 글자 선명도와 큰 frame/encode 비용을 비교합니다. 초과 시 해상도를 낮추며 JPEG로 몰래 바꾸지 않습니다. |
| image cap | 변별 2048px, 4MP, wire 2MiB | source view 크기와 scaled capture 가능 여부를 캡처 전에 검사합니다. 만족할 수 없으면 거부합니다. |
| JSON cap | 요청 wire 64KiB, script 32KiB, fill 16KiB | parse/native 호출 전에 제한합니다. |
| 응답 JSON cap | wire 512KiB | 논리 64KiB eval 문자열의 JSON escaping 증가를 고려합니다. |
| capture 동시성 | browser별 1개, 서비스 전체 native capture 1개 | webview callback/worker 누적을 막습니다. |
| 구독 예산 | 서비스 전체 viewer 3개, capture browser 2개 | driver는 browser 개수와 무관하게 전체 1명입니다. |
| frame queue | viewer별 미확인 1개+최신 대기 1개 | 오래된 화면을 큐에 쌓지 않습니다. |
| control queue | 16개 또는 총 1MiB | control 우선순위와 무한 queue 금지를 함께 적용합니다. |
| 출력 budget | client별 평균 1MiB/초, burst 2MiB | 느린 연결에서 interval/해상도를 낮추고 생성 자체를 감속합니다. |

PNG 파일 저장→재읽기→base64 반복을 사용하지 않습니다. 첫 primitive 분리에서는 기존 PNG 변환을 재사용할 수 있지만 JPEG는 가능한 한 소유 픽셀/native 이미지에서 직접 인코딩하여 PNG decode→JPEG encode의 이중 비용을 없앱니다. native 객체 접근은 main thread, 허용되는 순수 압축·리사이즈는 제한된 blocking worker로 보냅니다.

snapshot timeout과 실제 native 취소는 다릅니다. callback이 살아 있으면 새 native capture를 겹쳐 시작하지 않고 해당 작업의 permit을 유지하며 producer를 paused/quarantined로 둡니다. 늦은 callback 또는 webview 소멸이 소유권을 회수해야 합니다. callback이 끝내 오지 않아 자동 복구할 수 없는 경우 구조화된 오류를 표시하고 명시적 browser 재생성/GUI 복구가 필요함을 알립니다. timeout마다 새 thread/native 작업을 만드는 방식은 금지합니다. 기존 screenshot의 10초 대기와 eval callback의 5초 제한은 서로 별도입니다 (`src-tauri/src/browser/screenshot.rs:149-172`, `src-tauri/src/ipc/browser.rs:774-790`).

admission은 전송뿐 아니라 **캡처 전**에도 적용합니다. 모든 viewer가 credit을 잃었으면 capture를 멈추거나 감속합니다. oversized는 영구 drop으로 처리하고 더 작은 다음 frame을 생성합니다(O08). 최신 frame 보존과 무제한 transport buffer는 다른 문제이므로 write deadline·byte budget을 적용합니다. control 우선 처리는 frame 경계에서 가능하며 이미 전송 중인 큰 frame을 선점한다고 가정하지 않습니다.

계산 예시는 100KiB×4장/초=400KiB/초, 약 3.28Mbps입니다. 500KiB×8장/초는 약 3.91MiB/초, 32.8Mbps이고 TLS/relay overhead는 별도입니다. 이를 실측 frame 크기로 주장하지 않습니다. viewer 수가 늘면 native capture는 공유해도 전송량과 decoder 비용은 늘어납니다.

capture/encode/decode 시간, p50/p95 frame age, 입력→다음 frame 지연, delivered/dropped 수, CPU·메모리·전송량·배터리/발열을 전원/배터리 환경으로 나누어 측정합니다. upstream quality 70은 참고값일 뿐 WKWebView에서 30/60FPS나 특정 배터리 절감을 약속하지 않습니다.

## 6. 보안 모델

### 6.1 PIN·capability·ticket·desktop lock

기존 6자리 PIN pairing과 device capability token을 필수로 사용합니다. 별도 공개 인증 체계나 무인증 browser 포트를 만들지 않습니다. Bearer는 HTTP Authorization에만, browser WS는 기존 single-use ticket에만 둡니다. target/host 바인딩, 만료, 재사용, 발급 후 device 철회를 모두 검사합니다. direct trusted-overlay/transport 및 relay의 기존 정책을 약화하지 않고 Origin 검사를 적용합니다. CORS는 인증이 아닙니다.

upgrade 전, subscribe/claim/dispatch 직전, native 결과와 frame 공개 직전에 auth·revocation·admission epoch를 확인합니다. retained revocation watch를 작업 시작 전부터 감시하고 연결 수명 전체를 취소하는 기존 패턴을 재사용합니다 (`src-tauri/src/remote/server.rs:1248-1259`). 취소 이후 callback 결과는 원격에 공개하지 않습니다.

terminal의 active-session 잠금은 유지합니다 (`src-tauri/src/remote/server.rs:1356-1413`, `src-tauri/src/remote/server.rs:2225-2248`). browser는 같은 **authoritative desktop 공유 workspace/worktree와 현재 공개된 visible browser inventory**에 묶습니다. terminal sessionId와 browserId를 비교하지 않습니다. backend는 GUI가 만든 inventory/epoch를 사용하고 remote가 arbitrary workspace/path로 범위를 확대할 수 없습니다. Machine 권한만으로 이 잠금을 우회하지 않습니다.

공유 scope/가시성 변경, 화면 잠금, owner 회수 시 lease와 구독을 무효화합니다. 일시적인 selection None이면 다른 browser로 권한을 넘기지 않고 새 frame/drive를 보류합니다. hidden/minimized/private는 기본 공유에서 제외하고 원격 요청으로 visible/focus/tab을 바꾸지 않습니다. GUI 부재 시 headless terminal의 예외를 browser에 그대로 적용하지 않습니다.

### 6.2 단일 remote driver·rate limiting

global driver broker는 GUI 서비스에 하나만 있고 direct/relay가 같은 broker를 사용합니다. gateway가 전달하는 device/permission/admission 정보는 인증된 로컬 bridge가 만든 값이며 public JSON에서 받지 않습니다. GUI는 현재 browser instance·공유 epoch·lease를 native 실행 직전에 확인합니다. local owner 회수는 remote보다 우선하고 자동 takeover나 남은 viewer에게 lease fallback은 없습니다.

초기 device별 command 한도는 20회/초, burst 40회, claim/release 2회/초, eval 1회/초입니다. 브라우저별 동시 eval/wait는 각각 1개, 실행 대기 command는 16개 이하입니다. 연결별뿐 아니라 device/서비스 집계 제한을 두어 다중 socket으로 우회하지 못하게 합니다. frame·snapshot·result의 byte budget도 별도입니다. 가벼운 rate-limit 오류는 retryAfterMs, 지속 초과는 연결 종료로 처리합니다.

철회/lease 만료가 대기 중 발생한 mutation은 실행하지 않습니다. 이미 실행된 부작용을 취소로 롤백할 수 없다는 한계는 명시합니다. pending task가 heartbeat/read loop를 소유하지 않도록 하여 wait 중에도 즉시 release를 처리합니다.

### 6.3 로컬 절대 경로와 credential 비공개

**public endpoint에는 workspace IDs/worktree slugs/browserId만 허용하며 로컬 절대 경로를 받거나 반환하지 않습니다.** raw BrowserState/BrowserCliResponse/IpcError를 원격에 직렬화하지 않습니다. 특히 screenshot 반환 경로/파일 오류는 remote command surface에 연결하지 않습니다 (`src-tauri/src/browser/screenshot.rs:174-190`, `src-tauri/src/ipc/browser_cli.rs:211-246`).

allowlist DTO와 고정 공개 오류 메시지를 사용하고 내부 error ID만 보냅니다. local credential, 홈/프로젝트 root, UDS/port 파일, profile 저장 위치, webviewLabel을 query·metadata·결과·원격 로그에 포함하지 않습니다. URL/제목/DOM 결과/eval 공개 문자열에도 앱이 생성한 내부 경로가 섞이면 해당 필드를 차단하거나 정제합니다. Unix/Windows/UNC·escaped URL fixture로 검증합니다. 사용자 제공 workspace 이름이나 JSON field alias를 경로 우회 채널로 쓰지 못하게 합니다.

navigation은 허용된 http/https에 한정하고 file/javascript/data/Tauri/internal scheme, credential 포함 URL, guest control host를 거부합니다. localhost 개발 페이지는 owner가 승인한 세션의 web content로 취급하되 파일 API나 임의 내부 endpoint 권한을 주지 않습니다. redirect/페이지 변경 후에도 공유 정책을 재평가하여 금지된 destination이 보이면 capture/drive를 중단합니다.

앱 내부 경로 비공개는 고정 요구입니다. 페이지 자체가 민감정보나 경로를 시각적으로 표시하는 경우까지 범용 OCR/regex가 완전히 제거한다고 주장하지 않습니다. 그러한 페이지는 공유 승인/대상 제한 단계에서 제외하거나 공유를 중단해야 합니다. 임의 eval 또한 owner 동의와 출력 검사 대상입니다. 경로 없는 DTO만으로 픽셀 콘텐츠까지 자동 비식별화되었다고 간주하지 않습니다.

### 6.4 guest bridge·eval·wait 계약 보존

guest nonce 검증, 시작 시 캡처한 내장 함수, isTrusted 보호는 유지합니다 (`src-tauri/src/browser/guest.rs:19-29`, `src-tauri/src/browser/guest.rs:103-127`). console ring 500개 제한은 gateway 인증을 대신하지 않습니다 (`src-tauri/src/browser/guest.rs:56-68`). nonce나 capability를 frame metadata 또는 remote JS로 보내지 않습니다.

native keypress는 trusted event로 app chrome shortcut을 유발할 수 있으므로 page-scoped allowlist를 먼저 적용합니다 (`src-tauri/src/browser/guest.rs:179-255`). Windows의 기존 trusted keypress Unsupported를 capability에 그대로 반영합니다 (`src-tauri/src/ipc/browser.rs:1669-1691`). isTrusted 보호를 끄거나 OS 전체 이벤트로 우회하지 않습니다.

eval은 로그인된 페이지의 데이터와 부작용에 접근할 수 있으므로 Control+lease와 owner의 명시적 승인이 필요합니다. wait의 function condition도 같은 승인 대상입니다. `truncate_eval_result`의 **65,536 UTF-8 bytes**, 문자 경계, truncated boolean을 그대로 보존합니다. undefined는 기존 Option/JSON null 의미이며 잘린 문자열을 완전한 JSON 객체로 다시 파싱하지 않습니다 (`src-tauri/src/ipc/browser.rs:2151-2162`, `src-tauri/src/ipc/browser.rs:2278-2295`). wire 512KiB cap은 이 논리 제한을 늘리는 것이 아니라 escaping overhead를 수용하는 상한입니다.

`BROWSER_WAIT_TIMEOUT`은 기존 **15초 예산·250ms polling·일시적 eval 실패를 재검사하는 의미**를 유지합니다 (`src-tauri/src/ipc/browser.rs:2298-2326`). 한 eval callback 대기 때문에 wall time이 정확히 15초에서 끊기지 않을 수 있습니다. gateway의 더 짧은 일괄 timeout으로 해당 code를 덮어쓰지 않습니다. auth 철회/실제 browser 소멸에 의한 취소는 별도 code이며 wait timeout과 구분합니다.

callback timeout은 이미 실행 중인 JavaScript의 CPU 강제 종료가 아닙니다. 결과 64KiB 제한도 실행 비용 sandbox가 아닙니다. 입력 크기·동시성·동의·권한 제한을 별도로 적용하고 desktop 복구 경로를 남깁니다.

## 7. 테스트 전략

**이번 문서 변경에는 빌드·제품 테스트를 실행하지 않습니다.** 다음은 구현 단계의 검증 계획이며 mock 통과로 native 동작/성능을 확인했다고 처리하지 않습니다.

### 7.1 unit / mock webview / protocol

| 범주 | 필수 검증 |
|---|---|
| SnapshotSource | JPEG/PNG 성공, webview 없음, native error/null image, bitmap/encode 실패, callback 없음·중복·늦은 도착, close 후 callback, 플랫폼 Unsupported |
| native 수명 | main-thread 접근, worker 객체 소유권, timeout 후 permit 유지, native in-flight 중복 금지, 구독 0개에서 capture 0회 |
| binary | 0–15B, kind/version/opcode/format/reserved 오류, LE golden bytes, metadata/frame 경계 ±1, checked length overflow, 빈 image, size/header 불일치, 초과 pixel, 필수 확장 누락 |
| 세대/좌표 | instance/service/desktop/document/viewport/snapshot 변경, seq 역전/wrap, 오래된 frame, letterbox/Retina/zoom/scroll, NaN/Infinity, target map 교체 |
| ready/admission | ready 이전 frame 차단, 취소 중 시작 완료, oversized 영구 drop, transient backpressure, 한 viewer 정체와 다른 viewer 진행, idle에 ACK stall 미적용 |
| 구독/ghost | 동일 viewer reconnect, 새 subscriber 예약 후 기존 정리, 다른 device ID 위장, 마지막 unsubscribe, heartbeat/ready/ACK deadline, late callback 폐기 |
| driver | 여러 device/browser/direct/relay 동시 claim, release/dispatch race, lease 만료, desktop 회수, owner 교체 후 이전 명령 거부, 자동 fallback 금지 |
| auth | 미인증 PIN/token, View-only drive, ticket 만료·replay·target 변경, 발급/upgrade/send 사이 revoke, Origin/transport, Machine scope의 browser lock 우회 거부 |
| 로컬 IPC | 잘못된 credential, fragmented frame, 크기 제한, raw CLI passthrough 거부, GUI 재시작/epoch 변경, daemon-only unavailable, 기존 CLI 호환 |
| eval/wait | 65,535/65,536/65,537 UTF-8 bytes, 한글/emoji, JSON escaping, undefined/error/timeout, 즉시/지연/불만족 condition, 15초 soft budget, heartbeat 병행, BrowserWaitTimeout 구별 |
| 정보 공개 | 금지 path 필드/alias, Unix/Windows/UNC, 내부 오류·screenshot path, URL/제목/eval 문자열, local nonce/token 유출 fixture |
| 요청/자원 | requestSeq high-watermark, 중복 병합, 캐시 퇴거 뒤 재실행 금지, queue count/bytes, device 집계 rate limit, mount/unmount 후 timer/bitmap/subscriber 회수 |
| frontend/relay | decoder 역전 완료, host 변경, visibilitychange/IME, 새 ticket 발급, mutation replay 금지, direct/relay의 binary/text/close/revoke 동일 결과 |

Rust와 TypeScript가 같은 16B golden fixture를 상호 decode하도록 합니다. upstream 기본 decoder가 받아들이는 정상 기본 envelope를 참고하되 Ferryx의 추가 크기/identity 검증은 별도 기대값입니다. fake monotonic clock·snapshot source·local IPC·writer credit·revocation을 주입할 수 있게 합니다.

### 7.2 사용자 QA 체크리스트

- [ ] macOS desktop과 iOS Safari/Android Chrome/desktop web에서 navigate→frame 갱신→reference click/fill→keypress→wait→eval을 확인하고, native point click은 지원 capability에 맞게 별도 확인합니다.
- [ ] 두 디바이스가 동시에 보기/claim을 수행해 한 명만 조작하며, 다른 browser를 대상으로 claim해도 global lease가 하나인지 확인합니다. owner 회수 직후 입력/화면 공개가 중단되어야 합니다.
- [ ] 모바일 회전·keyboard 표시·IME·Retina·zoom·scroll·letterbox를 확인합니다. canvas/iframe/dialog는 실제 지원 또는 명확한 Unsupported여야 합니다. desktop 탭이 자동 전환되면 실패입니다.
- [ ] 화면 잠금·background·browser close·GUI 종료·네트워크 단절·direct→relay 복구를 확인합니다. 새 ticket/구독을 사용하고 제어권을 다시 요청하며 이전 host/browser 이미지와 미확인 mutation을 복원하지 않아야 합니다.
- [ ] 같은 장면에서 1/4/8회 목표 polling, JPEG/PNG, 해상도별 CPU·메모리·전송량·입력 지연을 전원/배터리별로 비교합니다. 미지원 Windows/Linux에서는 capability off와 terminal 유지도 확인합니다.

## 8. TypeSafe System One (Jev) 실측 판정 기반 확정 사항

TypeSafe API 연동(`jev-1.13.0`, 259ms, 7개 질문 병렬 Fan-out 실측 추론)을 통해 미결정 사항 7건에 대한 정량적 확률 분포와 신뢰도(Confidence) 검증을 완료하고 최종 설계를 확정했습니다.

| 항목 | TypeSafe Jev 판정 | 확신도 (Confidence) | 채택 근거 및 구현 정책 |
|---|---|---|---|
| **1. 화질·반응성 기본값** | `jpeg70_with_ondemand_png` | 100.0% (P=1.00) | **스트림 JPEG 70 (1280px / 250ms / 4fps) + On-demand 고해상도 PNG 스냅샷 버튼.** 터미널/브라우저 작은 글씨 판독성을 보장하면서도 상시 PNG 대비 대역폭과 배터리 소모를 3~5배 절감. |
| **2. eval 승인 지속 범위** | `per_claim_ephemeral` | 100.0% (P=1.00) | **드라이버 claim 단위 승인 (Lease 종료/회수 시 즉시 만료).** eval은 임의 JS 실행이 가능하므로 세션 단위 유지를 배제하고 최소 권한 원칙(Principle of Least Privilege) 적용. |
| **3. hidden/minimized 지원** | `visible_only_with_auto_pause` | 100.0% (P=1.00) | **v1은 visible 일반 창만 수용, hidden 시 스트림 'paused' 전환.** macOS WKWebView의 백그라운드 스로틀링에 의한 품질 저하 방지. 화면 잠금 시 스트림 즉시 중단. |
| **4. 플랫폼 순서** | `macos_first_then_windows` | 100.0% (P=1.00) | **macOS 1순위(takeSnapshot), Windows 2순위(WebView2 CapturePreview), Linux 3순위.** 포터블 인터페이스는 1일차부터 유지하되 미지원 타겟은 typed `Unsupported` 반환. |
| **5. 화면 point 입력 범위** | `ref_required_main_frame_only` | 93.0% (P=0.96) | **semantic reference 조작 필수 + native point는 메인 프레임만.** 좌표 매핑이 불안정한 canvas/iframe/dialog는 v1에서 명시적 `unsupported` 오류 응답 처리. |
| **6. Orca 호환 범위** | `envelope_compat_custom_rpc` | 99.0% (P=1.00) | **0x62 16B 이진 엔벨로프 및 프레임 메타데이터 호환.** Ferryx 전용 단일 드라이버 의미론을 보존하여 불필요한 결합도 배제. |
| **7. 동시 예산** | `budget_3viewers_2browsers_1driver` | 42.0% (P=0.62 / 1:1=0.38) | **viewer 3개 / capture browser 2개 / driver 1명 고정.** M4 기준 안정권이며, v1 초기 리소스 안전성(0.38 경합)을 고려해 실측 후 단계적 증설. |

PIN/capability 인증, 단일 remote driver, browserId targeting, 원격 절대 경로 비공개, eval 64KiB, 기존 wait 의미, 탭 전환 설계 제외는 선택 사항이 아닙니다.

## 9. 파일 변경 예상 목록

**이번 실제 변경:** `docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md` 한 파일입니다. 아래는 향후 구현 예상 목록이며 각 파일의 소유 Phase는 하나뿐입니다.

| 소유 Phase | 파일 또는 동일 소유권의 파일 묶음 | 역할 |
|---|---|---|
| 0 | `docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md` | 계획/제품 결정/후속 근거 갱신 |
| 1 | `src-tauri/src/browser/screenshot.rs`, `src-tauri/src/browser/snapshot_source.rs` | memory capture·기존 PNG wrapper |
| 2 | `src-tauri/src/browser/remote_service.rs`, `src-tauri/src/browser/remote_driver.rs` | producer·명령 수명·전역 driver |
| 2 | `src-tauri/src/browser/remote_input.rs`, `src-tauri/src/browser/remote_bridge_protocol.rs` | point/key adapter·local IPC 계약 |
| 2 | `src-tauri/src/browser/manager.rs`, `src-tauri/src/browser/model.rs`, `src-tauri/src/browser/guest.rs` | identity/snapshot map/geometry 관측 |
| 3 | `src-tauri/src/remote/browser_backend.rs`, `src-tauri/src/remote/browser_protocol.rs` | daemon→GUI 공급자·public codec |
| 3 | `src-tauri/src/remote/browser_ws.rs`, `src-tauri/src/remote/browser_admission.rs`, `src-tauri/src/remote/browser_security.rs` | WS 수명·예산·권한 |
| 4 | `src-tauri/src/browser/mod.rs`, `src-tauri/src/remote/mod.rs` | production/test 모듈 등록 |
| 4 | `src-tauri/src/remote/state.rs`, `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/relay_server.rs` | backend 주입·route·capability·ticket |
| 4 | `src-tauri/src/ipc/browser.rs`, `src-tauri/src/ipc/browser_cli.rs`, `src-tauri/src/lib.rs` | helper·remoteAttach·GUI/owner revoke 연결 |
| 5 | `ui/src/remote/browserProtocol.ts`, `ui/src/remote/browserClient.ts`, `ui/src/remote/useRemoteBrowser.ts`, `ui/src/remote/RemoteBrowser.tsx` | typed transport·구독·viewer |
| 6 | `ui/src/remote/useRemoteBrowserDriver.ts`, `ui/src/remote/RemoteBrowserControls.tsx`, `ui/src/remote/RemoteBrowserWorkspace.tsx`, `ui/src/remote/RemoteApp.tsx` | 제어 UX·composition·remote 앱 통합 |
| 6 | `ui/src/components/RemoteBrowserSharingIndicator.tsx`, `ui/src/App.tsx`, `ui/src/lib/tauri.ts` | desktop 표시·owner 회수 |
| 7A | `src-tauri/src/browser/snapshot_source_tests.rs`, `src-tauri/src/browser/remote_service_tests.rs` | native/서비스 mock |
| 7A | `src-tauri/src/remote/browser_protocol_tests.rs`, `src-tauri/src/remote/browser_security_tests.rs`, `src-tauri/src/remote/browser_lifecycle_tests.rs` | codec/auth/수명 검증 |
| 7B | `ui/src/remote/browserProtocol.test.ts`, `ui/src/remote/RemoteBrowser.test.tsx`, `ui/src/remote/RemoteBrowser.mobile.test.tsx`, `ui/src/remote/RemoteBrowser.reconnect.test.tsx` | frontend/모바일/복구 검증 |

`ui/src/main.tsx`, 기존 `ui/src/remote/remoteClient.ts`, terminal binary protocol, `src-tauri/src/ipc/terminal.rs`, `src-tauri/src/ipc/tests.rs`, `paired_host/*`, 다른 세션의 suspended-wake 테스트·SSH 보고서는 write scope 밖입니다. `.omo/upstream-orca/`도 읽기 전용입니다. Cargo manifest/lockfile 변경은 현재 예상 write scope가 아니며 신규 의존성이 필요하면 별도 범위 승인을 받아야 합니다. 이번 작업은 커밋하지 않습니다.


### §8.1 재판정 확정 (2026-09-17, typesafe-judge jev-1.13.0 2차 실측 — 결정 게이트)

- **D5 point 입력**: `ref_plus_point_mainframe` (P=0.91, conf 0.87) — §8 표와 동일 최종 확정. v1 native point 입력은 메인 프레임 한정, iframe/canvas/dialog는 명시적 unsupported. reference 조작은 필수.
- **D7 동시 예산**: `viewer2_capture1` (P=1.00, conf 0.99) — **§5 구독 예산 표(viewer 3/capture 2)와 §8 표(3/2)를 v1 상수로서 viewer 2/capture 1로 우선 적용.** 실측 후 3/2로 완화하며 이는 상수/설정 변경만으로 가능. 리드 1차 판정(0.60/0.40 근소차)과 2차 고신뢰 판정(0.99) 모두 보수측이므로 확정.
- 구현 레인에 주입하는 바인딩 상수: viewer per browser = 2, concurrently captured browsers = 1, global driver = 1.
