# Ferryx Attention Inbox — 구현 범위 확정서

**작성:** 2026-09-12 · **상태:** 범위 확정, 착수 대기 · **결정권자:** 사용자

멀티 프로젝트·멀티 호스트로 흩어진 에이전트를 찾아다니지 않도록, 전 호스트의 에이전트를 한 목록에 모으고 클릭 한 번으로 해당 세션에 도달하게 한다. 사용자 결정은 **원격 포함 완전판**.

---

## 1. 확정된 결정

- **원격 팬아웃 = WS 구독 + 지수 백오프 재연결.** 별도 폴링 경로를 만들지 않는다. 게이트웨이에 이미 WS 이벤트 엔드포인트가 있고, `useInventory.ts`가 revision 갭을 감지하면 전체 재부트스트랩을 수행하므로 재연결이 폴링의 역할을 대신한다. 연결이 없는 호스트는 감추지 않고 `unavailableHosts` + `completeness: "partial"`로 정직하게 표시한다.
- **표시 범위 = working 포함 전부.** 단 목록과 배지의 의미를 분리한다. 목록은 waiting → 안 읽은 done → working 순으로 전부 나열하고, **버튼 배지 카운트는 actionable(waiting + 안 읽은 done)만** 센다. working까지 세면 배지가 상시 점등돼 신호 가치를 잃는다.
- **원격 머신 항목 클릭 = 호스트 전환.** 해당 머신으로 연결을 전환하고 세션을 포커스한다.
- **OS 알림 발사함.** 단 **원격 머신 항목 한정**이다. 로컬·SSH 세션은 이미 `notificationCoordinator`의 completion edge가 알림을 쏘므로, 인박스가 같은 대상에 또 쏘면 이중 발사가 된다. 인박스는 "이 데스크톱이 직접 보고 있지 않은 머신"의 전이만 담당한다.

## 2. 이미 있는 것 (배선만 없음)

- `src-tauri/src/ferryx_scope/control/` 460줄 — `Inventory`(`register_host`/`insert`/`validate`/`report`/`snapshot`/`subscribe` broadcast), `router.rs`(Axum 라우트, base64url opaque target, `ScopeError`→HTTP 매핑), `service.rs`, `local.rs`, `lease.rs`, 테스트 3종. `lib.rs`·`ipc/`·`remote/server.rs` 참조 **0건**.
- `ui/src/features/ferryx/control/` — `AttentionInbox.tsx`, `useInventory.ts`(구독 후 부트스트랩, revision 델타, 갭 시 재부트스트랩), `client.ts`(`InventoryClientState`, `targetKey`, `opaqueTarget`). 마운트 **0건**.
- 로컬 크로스-프로젝트 attention 집계는 **이미 동작한다.** `ui/src/App.tsx:355` `deriveFocusedTerminal`이 workspace 스냅샷을 순회해 `attentionInventory`를 만들고, `src-tauri/src/remote/server.rs:484`가 병합해 `ui/src/remote/RemoteSessionList.tsx`가 소비한다. 없는 것은 **데스크톱 자신이 보는 화면**이다.
- `ui/src/lib/remoteClient.ts` 182줄 `RemoteClient` — 호스트별 토큰(`getRemoteAuthToken(hostId)`)으로 페어링된 머신 게이트웨이 호출 가능.
- 상태 판정 — `agent_detect` 11개 매니페스트, waiting/working/done, `engine.rs:90` HOLD 불변식.

## 3. 구조 제약

원격 게이트웨이(Axum)는 데스크톱 GUI가 아니라 **데몬 프로세스 안에서** 구동되고, 원격 이벤트는 UDS `SubscribeRemoteEvents` + `start_remote_event_bridge`를 거쳐 데스크톱에 전달된다. 데몬이 `agent_states`와 PTY 마스터를 소유하므로 **`Inventory`의 소유자는 데몬이고 데스크톱은 소비자다.** 이 제약이 아래 A·B의 형태를 결정한다.

## 4. 작업 분해

- **A. 데몬 인벤토리 상주** — `ferryx_scope::control::Inventory`를 데몬 상태에 두고, 로컬 PTY 세션과 SSH 호스트 세션의 상태 전이를 `report()`로 주입한다. 세션 종료 시 `removed` 전이(기존 `agent_states.remove()` 호출부 3곳과 동일 지점).
- **B. 데스크톱 노출** — UDS 프로토콜에 inventory list/subscribe 추가 → Tauri 커맨드 + 이벤트 → `ControlClient` 구현체. 기존 remote event bridge 패턴을 따른다.
- **C. 원격 팬아웃** — 페어링된 머신마다 `RemoteClient`로 부트스트랩 조회 + WS 구독, 스냅샷 병합, 실패 호스트는 `unavailableHosts`에 누적하고 `completeness`를 `partial`로 낮춘다. 재연결은 지수 백오프.
- **D. UI 마운트** — `ui/src/components/Sidebar.tsx:510` `NotificationCenterButton` 이웃에 Attention 버튼 + 팝오버. 정렬은 waiting → 안 읽은 done → working, 배지는 actionable만 카운트.
- **E. 점프 라우팅** — 로컬/SSH는 기존 세션 포커스 경로 재사용. 원격 머신 행은 호스트 전환 후 포커스하며, **돌아오는 경로**(이전 호스트로 복귀)를 함께 제공한다.
- **F. 인증·권한** — 호스트별 device token, `scopeControlV1` capability 게이팅. 토큰 만료 시 해당 호스트만 `unavailable` 처리하고 전체 목록을 죽이지 않는다.
- **G. 알림 연동** — 원격 머신 항목 전이에만 OS 알림. 로컬·SSH 대상은 기존 경로가 담당하며 인박스는 발사하지 않는다(이중 발사 방지).
- **H. 데몬 프로토콜 범프** — UDS 신규 메시지는 프로토콜 버전을 올린다. 구버전 데몬은 unknown 메시지에 graceful하게 응답해야 하고, 업그레이드는 **세션을 유지하는 기존 롤링 업그레이드 경로**를 따른다. 데몬을 죽여서 적용하지 않는다(PTY 마스터 fd 전량 손실).
- **I. 테스트** — revision 델타/갭 재부트스트랩, 팬아웃 부분 실패 시 `partial` 표기, 점프 라우팅(로컬/SSH/원격 3분기), 알림 이중 발사 방지, 구버전 데몬 호환.

## 5. 위험

- **호스트 전환은 화면 전체를 바꾼다.** 원격 머신 행을 누르면 데스크톱이 그 머신의 클라이언트가 되므로 보고 있던 로컬 화면이 교체된다. 복귀 경로가 없으면 UX 사고가 된다(E에 포함).
- **Active Desktop Lock과의 상호작용.** 게이트웨이 WS는 현재 데스크톱 세션에 바인딩되므로, 전환이 기존 원격 클라이언트 연결을 끊을 수 있다. 전환 전 경고 또는 무해함 확인이 필요하다.
- **working 포함으로 목록이 길어진다.** 호스트가 많으면 스크롤 목록이 되므로 호스트별 그룹핑 또는 접기가 필요해질 수 있다.

## 6. 범위 밖

OSC 133(2026-09-12 제외 결정), Design Mode, `features/ferryx/chat`·`push`. 모두 별건이다.
