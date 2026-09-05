# 알림 / 배지 / 하이라이트(pane · worktree · workspace) 전체 코드 리뷰

**작성일:** 2026-09-04
**범위:** 에이전트 활동 상태(activity) 파이프라인 전체 — 상태 산출(store) → 집계(selector) → 표시(pane / tab / worktree / project) → OS 알림·사운드·Dock 배지 → 원격(remote) 미러링
**성격:** 리뷰 + 개선 제안 문서. 이 문서 작성 과정에서 소스 변경은 하지 않았다.

---

## 1. 리뷰 대상 코드 지도

활동 신호가 흐르는 경로를 실제 파일 기준으로 정리하면 다음과 같다.

**신호 생성 (Rust)**
- `src-tauri/src/agent_detect/` — 화면(screen) 기반 에이전트 상태 판정 (`working` / `blocked` / `idle`)
- `src-tauri/src/terminal/pty.rs`, `src-tauri/src/daemon/server.rs` — 터미널 타이틀 변경 및 bell 이벤트 방출

**상태 산출 (프론트엔드 리듀서)**
- `ui/src/state/workspaceStore.ts`
  - `SESSION_SCREEN_ACTIVITY` (L1791~) — 화면 판정 → `working` / `waiting` / `done` 매핑
  - `SESSION_TITLE_ACTIVITY` (L1869~) — 타이틀 파싱 기반 활동 분류
  - `SESSION_LIFECYCLE` (L1706~) — 프로세스 종료 시 강제 `done`
  - `MARK_SESSION_ACTIVITY_SEEN` (L1854~) — 사용자 확인 처리
  - `applySessionActivity` (L1980~) — 저장 + unread 승격의 단일 관문
  - `acknowledgeTabCompletions` (L1955~), `clearWorktreeUnreadWhenRead` (L2097~)

**집계 (selector)**
- `ui/src/lib/activity.ts` — `summarizeActivities`, `combineActivitySummaries`, `resolveActivityIndicator`
- `ui/src/state/workspaceStore.ts` — `selectTabActivitySummaries` (L1158), `selectWorktreeActivitySummaries` (L1175), `selectWorktreeActivitySummariesAcrossWorkspaces` (L1225), `selectActivityNotificationTargets` (L1250)

**표시**
- pane: `ui/src/components/TerminalSplitView.tsx` `PaneLeafView` (L928~) + `ui/src/components/NativeTerminalPane.tsx` (L1604~, 네이티브 attention frame IPC)
- tab: `ui/src/components/tab-dnd/SortableTab.tsx` (L52~54, L107~)
- worktree: `ui/src/components/WorktreeList.tsx` `WorktreeRow`
- project(workspace): `ui/src/components/Sidebar.tsx` `ProjectHeader` (L470~), `summarizeProjectActivity` (L547~)
- 공통 도트: `ui/src/components/ui/StatusDot.tsx`

**알림 / 사운드 / 배지**
- `ui/src/lib/notificationCoordinator.ts` — 벨/상태변화 → 알림 결정
- `ui/src/lib/notificationSettings.ts` — 설정 로드/저장 + `useAttentionFrameEnabled`
- `ui/src/App.tsx` L533~550 (상태 변화 감시), L1000~1007 (배지 카운트)
- `src-tauri/src/notification/{mod,model,service,badge,audio,permission}.rs`, `src-tauri/src/ipc/notifications.rs`

**원격 미러**
- `ui/src/App.tsx` L300~324 (원격 페이로드 구성), `ui/src/remote/RemoteSessionList.tsx`

---

## 2. 전반 평가

구조 자체는 건강하다. 특히 잘 되어 있는 점:

- **단일 관문 설계.** 모든 활동 저장과 unread 승격이 `applySessionActivity` 하나를 통과한다. "보이는 탭에서 끝난 작업은 태어날 때부터 seen" 규칙(L2002~2004)이 한 곳에 있어 추론이 쉽다.
- **책임 분리가 명시적.** `src-tauri/src/notification/mod.rs` 주석대로 Rust는 "OS와 안전하게 대화하는 법"만 알고, "알릴지 말지"는 프론트엔드가 결정한다. 이 경계 덕분에 포커스/활성탭 지식이 Rust로 새지 않았다.
- **복원 시 in-flight 주장 폐기.** `sessionPersistence.ts` L654~666에서 재시작 후 `working`/`waiting`을 `done` + `seen: true`로 강등한다. 죽은 에이전트가 영구 스피너를 남기지 않는 올바른 판단이다.
- **parked workspace 처리.** `dispatchToParkedWorkspace` (workspaceStore L290~306)로 마운트되지 않은 프로젝트의 세션도 상태가 얼어붙지 않는다.
- **크로스플랫폼 배지.** `SetBadgeCountResult::unsupported`로 Windows/Linux에서 실패 대신 구조화된 미지원 결과를 반환한다.

문제는 구조가 아니라 **의미론의 일관성**이다. 같은 개념("주목이 필요하다")이 네 개의 표시 계층에서 서로 다른 규칙으로 계산되고, `seen`과 `unread`라는 두 개의 병렬 "읽음" 개념이 부분적으로만 동기화된다.

---

## 2.1 사용자 보고 기반 실동작 재점검 (2026-09-04 추가)

사용자 보고: **데스크톱 알림, 기본 사운드, pane highlight가 모두 동작하지 않는다.** 최초 리뷰는 상태 의미론 중심이었고, 실제 발화 경로의 전면 무동작을 포함하지 못했다. 재점검 결과 아래 세 결함을 확인했다.

### A. 데스크톱 알림: Tauri IPC 인자명이 틀려 dispatch 자체가 실패한다

프론트엔드 `ui/src/lib/tauri.ts` L625~627:

```ts
invokeCommand("cmd_notification_dispatch", { req });
```

Rust `src-tauri/src/ipc/notifications.rs` L70~74:

```rust
pub async fn cmd_notification_dispatch<R: Runtime>(
    app: AppHandle<R>,
    request: DispatchNotificationRequest,
)
```

Tauri command 인자는 이름으로 매칭된다. Rust는 `request`를 요구하지만 프론트엔드는 `req`를 보내므로 command 호출이 payload 역직렬화 전에 실패한다. `NotificationCoordinator`가 반환 Promise를 `void dispatchNotification(...)`으로 버려 이 rejection은 사용자에게 드러나지 않는다. 따라서 **에이전트 완료를 감지해도 네이티브 알림 제출 단계까지 도달하지 않는다.**

수정은 단순하다.

```ts
return invokeCommand<DispatchNotificationResult>("cmd_notification_dispatch", { request: req });
```

그리고 TS wrapper를 mock한 기존 단위 테스트만으로는 Rust command signature 불일치를 잡을 수 없으므로, Tauri command boundary contract test 또는 공용 typed command adapter가 필요하다.

### B. 기본 사운드: 현재 구현상 독립적으로 재생되는 사운드가 전혀 없다

기본 설정은 `customSoundId: "system"`, `customSoundPath: null`이다(`notificationSettings.ts` L18~25). 그런데 `playNotificationSound`은 custom path가 없으면 즉시 `{ played: false }`를 반환한다(`tauri.ts` L651~656). 주석상 `system` 사운드는 OS banner가 담당한다.

즉 현재 기본 동작은:

1. 자체 사운드 재생 안 함
2. OS 알림 dispatch가 A의 인자명 버그로 실패
3. 결과적으로 **아무 소리도 나지 않음**

여기서 `soundId`는 함수 인자로 받지만 실행 분기에는 사용되지 않는다. `none`, `system`, `custom`의 의미가 타입에만 있고 동작으로 모델링되지 않았다.

권장 수정:

- `none`: 즉시 `played: false` 반환
- `custom`: 선택한 파일을 Rust audio player로 재생
- `system`: OS 알림 자체의 기본 사운드를 명시적으로 요청하거나, 번들된 짧은 기본 음원을 Rust player로 재생
- 호출 결과의 `played/reason`을 최소한 debug log 또는 설정 UI의 test 결과에 노출
- notification dispatch와 sound playback rejection을 `void`로 버리지 말고, coordinator가 `Promise.allSettled` 결과를 관측 가능한 오류 경로로 전달

### C. pane highlight: 읽음 판정이 pane이 아니라 tab 단위라 완료 시 즉시 소거된다

`workspaceStore.ts` L1970~1977의 `isTabVisible`은 tab group의 active tab이면 true다. `applySessionActivity` L2002~2004는 완료 상태가 이 visible tab에 있으면 무조건 `seen: true`로 저장한다.

```ts
const acknowledged =
  activity.state === "done" && (activity.seen === true || isTabVisible(state, tabId));
```

반면 pane frame은 `TerminalSplitView.tsx` L932~935에서 `!activity.seen`일 때만 표시한다.

```ts
const needsAttention = attentionFrameEnabled && Boolean(
  activity && (activity.state === "waiting" || activity.state === "done") && !activity.seen,
);
```

따라서 활성 tab 안의 split pane은 실제 포커스 pane이 아니어도 "이미 봄"으로 처리된다. 완료 이벤트가 들어오는 순간 `seen: true`가 되어 frame 조건을 만족할 수 없다. 단일 pane도 사용자가 다른 창을 보고 있거나 앱이 백그라운드여도 tab이 active라는 이유만으로 동일 문제가 생긴다. DOM visibility와 사용자 확인을 혼동한 모델 문제다.

권장 수정:

- `isTabVisible` 대신 `isSessionActivelyObserved(state, tabId, sessionId)` 사용
- 최소 조건: 앱 window가 foreground/focused이고, 해당 tab group이 focused이고, 해당 tab이 active이며, 해당 session의 leaf가 `activeLeafId`인 경우에만 born-acknowledged
- 더 안전한 조건: 완료 순간에는 무조건 attention을 생성하고, 정확한 native pane focus / 입력 이벤트가 해당 session에 들어왔을 때만 consume
- `waiting`도 동일 모델로 처리하고, pane 클릭 시 frame·tab dot·worktree dot·Dock badge를 하나의 acknowledge action으로 함께 해제

이 세 항목은 서로 완전히 동일한 한 원인은 아니다. **알림과 기본 사운드는 IPC dispatch 계약 파손으로 함께 죽었고, highlight는 tab 단위 seen 판정 때문에 별도로 억제된다.** 다만 세 기능 모두 실제 발화 결과를 관측하지 않는 테스트/오류 처리 때문에 회귀가 사용자 실행 전까지 숨었다는 공통 문제가 있다.

---

## 3. 발견된 문제

심각도 순으로 정리한다. 각 항목은 파일:라인 근거를 가진다.

### P1-1. Dock 배지가 worktree unread를 세지 않는다 — 배지 카운트 정의 불일치

`ui/src/App.tsx` L1000~1003:

```ts
const unreadBadgeCount = useMemo(
  () => Object.values(state.unreadTabIds ?? {}).filter(Boolean).length,
  [state.unreadTabIds],
);
```

세 가지 결함이 겹친다.

1. **현재 마운트된 workspace의 탭만 센다.** 다른 프로젝트(parked workspace)의 미확인 완료는 사이드바에는 표시되지만(`selectWorktreeActivitySummariesAcrossWorkspaces`) Dock 배지에는 절대 반영되지 않는다. 사용자가 앱을 백그라운드에 두고 여러 프로젝트를 돌리는 것이 Ferryx의 핵심 시나리오인데, 배지가 그중 하나만 센다.
2. **`unreadTabIds`만 세고 `activityBySessionId`의 미확인 `done`을 세지 않는다.** `applySessionActivity` L2011은 보이지 않는 탭의 `done`에서만 unread를 세우지만, `waiting`(사용자 입력 대기)은 unread를 세우지 않는다. 즉 **에이전트가 사용자 응답을 기다리는 상태는 Dock 배지에 전혀 나타나지 않는다.** 이건 가장 알림 가치가 높은 상태다.
3. **탭 단위 카운트라 의미가 모호하다.** 한 탭에 3개 pane이 각각 완료돼도 배지는 1이다. 반대로 배지 숫자가 "몇 개의 에이전트가 나를 기다리는가"를 뜻하지도, "몇 개의 탭에 새 소식이 있는가"를 정확히 뜻하지도 않는 중간 상태다.

### P1-2. `waiting`은 unread를 세우지 않아 알림 계층 간 신호가 끊긴다

`applySessionActivity` L2011:

```ts
if (activity.state === "done" && previous?.state !== "done" && !isTabVisible(state, tabId)) {
```

`done`만 unread를 승격한다. 반면:
- `resolveActivityIndicator` (activity.ts L124)는 `waiting`을 최우선 순위로 표시한다.
- `NotificationCoordinator.handleAgentStateChange`는 `waiting`과 `done` 모두를 completion edge로 보고 OS 알림을 쏜다 (notificationCoordinator.ts L124~127).
- `projectAttentionState` (Sidebar L542~545)는 `hasWaiting`을 최우선으로 본다.

즉 `waiting`은 "OS 알림도 오고, 도트도 뜨고, 프로젝트 헤더도 반응하지만, unread 플래그와 Dock 배지에는 없는" 상태다. 계층마다 다르게 취급되는 것 자체가 버그의 온상이며, 실제 증상은 P1-1의 (2)번이다.

### P1-3. `seen`과 `unread`가 이중 진실 소스로 갈라진다

같은 "사용자가 봤다"를 두 자료구조가 표현한다.

- `activity.seen` — 세션 단위, `MARK_SESSION_ACTIVITY_SEEN` / `acknowledgeTabCompletions` / `applySessionActivity`의 born-acknowledged 규칙이 건드린다.
- `unreadTabIds` / `unreadWorktreePaths` — 탭·워크트리 단위, `MARK_TAB_UNREAD` / `CLEAR_TAB_UNREAD` / `clearWorktreeUnreadWhenRead` 및 `NotificationCoordinator` 콜백이 건드린다.

두 값이 어긋나는 경로가 실재한다.

- `NotificationCoordinator`는 포커스가 없을 때 `onMarkTabUnread` / `onMarkWorktreeUnread`를 직접 호출한다(notificationCoordinator.ts L88~96, L136~144). 이 경로는 `activityBySessionId`를 전혀 건드리지 않는다. 반대로 `applySessionActivity`는 unread를 세우면서 `seen`도 관리한다. 같은 이벤트가 두 경로로 들어와 한쪽만 갱신될 수 있다.
- `MARK_SESSION_ACTIVITY_SEEN` (L1854~)은 `activity.seen`만 true로 만들고 `unreadTabIds`는 손대지 않는다. 즉 pane 클릭으로 attention frame은 꺼지지만 탭의 unread 도트와 Dock 배지는 그대로 남는다.
- `WorktreeRow`는 이 불일치를 UI 레이어에서 봉합한다(WorktreeList.tsx L54~58):
  ```ts
  const displaySummary = activitySummary
    ? activitySummary.hasUnread === unread ? activitySummary : { ...activitySummary, hasUnread: unread }
    : undefined;
  ```
  selector가 계산한 `hasUnread`를 컴포넌트가 prop으로 덮어쓴다는 것은 selector 결과를 신뢰할 수 없다는 자백이다.

### P1-4. `SESSION_TITLE_ACTIVITY`의 분기 폭발 — 실질적으로 검증 불가능한 상태 머신

workspaceStore L1869~1943, 약 75줄에 다음이 얽혀 있다: `isBareAgentTitle` / `isScreenSource` / `isScreenAgent` / `parsed?.isAgent` / `classified` / `inFlight` / `previous` 존재 여부. `isAgent`, `agentType`, `agentSource` 각각이 3항 연산자 3중첩으로 계산된다(L1919~1933).

이 로직은 실제 버그를 고치며 자란 흔적(L1905~1908 주석)이지만, 현재 상태로는 "화면 소스가 우선인가 타이틀 소스가 우선인가"를 코드에서 읽어낼 수 없다. 우선순위 규칙이 데이터가 아니라 제어 흐름에 인코딩되어 있다.

### P2-1. `WorkspaceTerminalActivity.agentSource`가 타입 계약 밖에 산다

`workspaceStore.ts` L40~42에서 `TerminalActivity`를 로컬 확장해 `agentSource`를 붙인다. 그런데:

- `WorkspaceState.activityBySessionId`의 타입은 여전히 `Record<string, TerminalActivity>` (L54) — 확장 타입이 아니다. 리듀서는 저장할 때 확장 타입을 쓰고 읽을 때 `as WorkspaceTerminalActivity`로 캐스팅한다(L1810, L1871).
- `applySessionActivity`의 동일성 비교(L1987~1997)는 `agentSource`를 비교하지 않는다. 따라서 **`agentSource`만 바뀐 업데이트는 조용히 버려진다.** `state`/`title`/`isAgent`/`agentType`/`source`/`seen`이 모두 같고 `agentSource`만 `"title"` → `"screen"`으로 바뀌는 전이는 반영되지 않는다.
- `sessionPersistence.ts` L658~666의 복원은 `agentSource`를 복사하지 않는다. 재시작 후 화면 유래 에이전트 신원이 소실되어 `isScreenAgent` 판정이 무조건 false가 된다.

### P2-2. 원격(remote) 표시가 데스크톱과 다른 규칙을 쓴다

`App.tsx` L312~318은 원격에 **세션의 원시 `activity.state`만** 보낸다. `seen`도, unread도 보내지 않는다. 그 결과 `RemoteSessionList.tsx`는:

- 자체 `attentionRank`(L105~110)를 재구현한다 — `activity.ts`의 `stateRank`/`summaryRank`와 중복이며 `unread` 개념이 없다.
- `StatusDot`을 쓰지 않고 인디케이터 마크업을 손으로 재작성한다(L446~463, L600~617). 크기(`size-3.5` vs `size-2` vs `size-1.5`), 링, 색 토큰이 데스크톱과 미묘하게 다르다.
- 이미 사용자가 데스크톱에서 확인한 완료가 모바일에서는 계속 초록 도트로 남는다.

### P2-3. 알림 상태 추적이 세 곳에서 중복된다

같은 "이전 상태" 추적이 세 벌 존재한다.

- `App.tsx` L531 `lastAgentStatesRef: Map<string, TerminalActivityState>`
- `NotificationCoordinator` 내부 `lastAgentState: Map<string, string>` (L47)
- 리듀서의 `previous = state.activityBySessionId?.[sessionId]`

`App.tsx`가 이미 edge를 판정해서(L537) coordinator를 호출하는데, coordinator가 또 `effectivePrev`로 edge를 판정한다(L120~127). `App.tsx`는 `previousState`를 넘기지 않으므로 coordinator의 내부 맵이 실제 판정에 쓰인다. 두 맵이 서로 다른 생명주기(ref는 컴포넌트, coordinator는 인스턴스)를 가져 워크스페이스 전환 시 어긋날 수 있다.

또한 `lastAgentStatesRef`는 **세션이 사라져도 정리되지 않는다.** 장기 실행 시 단조 증가하는 맵이다(경미한 누수).

### P2-4. `bellAgentSuppressionMs` 억제가 한 방향으로만 동작한다

`notificationCoordinator.ts` L74~85: 에이전트 완료 직후 1.5초 내 벨은 억제된다. 그러나 역방향 — 벨이 먼저 울리고 곧바로 에이전트 완료가 감지되는 경우 — 는 억제되지 않아 **같은 사건에 대해 알림이 두 번** 나갈 수 있다. 에이전트가 완료 시 벨을 울리는 흔한 패턴에서 순서는 이벤트 전송 경로 지연에 따라 뒤집힐 수 있다.

또한 벨 throttle(L69~73)이 억제 검사보다 먼저 타임스탬프를 기록하므로, 억제로 버려진 벨도 throttle 창을 소모한다.

### P2-5. `resolveActivityIndicator`의 `done` vs `unread` 구분이 사용자에게 전달되지 않는다

`activity.ts` L126: `if (summary.hasDone) return summary.hasUnread ? "unread" : "done";`

`unread`는 `bg-primary`(파랑), `done`은 `bg-status-success`(초록)으로 그려진다(StatusDot L44~59). 그런데 두 상태 모두 "완료됐고 아직 안 봤다"를 의미한다 — 차이는 벨이 함께 울렸는지 여부뿐이다. 사용자는 파랑/초록 도트의 의미 차이를 알 방법이 없고, 툴팁도 없다. `SortableTab` L53은 여기에 또 한 겹 덮어쓰기를 얹는다:

```ts
const activityIndicator = unread && (resolvedActivity === null || resolvedActivity === "done") ? "unread" : resolvedActivity;
```

### P3-1. 배지 IPC 스로틀 없음

`App.tsx` L1005~1007은 `unreadBadgeCount`가 바뀔 때마다 IPC를 쏜다. `cmd_notification_set_badge_count`는 `run_on_main_thread` + oneshot 채널을 사용한다(ipc/notifications.rs L182~196). 여러 에이전트가 동시에 완료/확인을 반복하면 메인 스레드에 짧은 폭주가 생긴다. 실측 문제로 관측되진 않았으나 배지는 지연 허용 신호이므로 coalesce가 자연스럽다.

### P3-2. attention frame 정리 이펙트의 의존성 누락

`NativeTerminalPane.tsx` L1611~1618의 cleanup은 `targetSessionId`만 의존성으로 갖고 내부에서 `needsAttention`을 참조하지 않는다. 언마운트 시 무조건 false를 보내므로 동작은 맞지만, 의도가 "언마운트 정리"라면 값 참조 없이 명시하는 편이 안전하다. 현재는 세션 ID가 바뀔 때마다 이전 세션의 프레임을 끄는 부수효과가 겹친다.

### P3-3. `StatusDot`의 fallback이 "알 수 없음"을 "실패"로 표시한다

StatusDot L63~71: 명시된 상태에 해당하지 않는 모든 값이 `bg-destructive`(빨강) + `data-status-state="failed"`로 떨어진다. `StatusDotState`는 `AgentState | TerminalActivityState | "unread"` 유니온이므로 향후 상태가 추가되면 조용히 빨간 실패 도트가 된다. 명시적 exhaustive 처리가 필요하다.

### P3-4. Sidebar에서 동일 집계를 3회 반복 계산

`Sidebar.tsx` L421~432: 드래그 오버레이 렌더 시 `summarizeProjectActivity`를 두 번(활동용, attention용) 호출하고, 그 안에서 `projectAttentionState`가 다시 호출된다. 결과는 같으므로 낭비다. 본 목록 경로(L338~343)는 올바르게 한 번만 계산한다.

### P3-5. worktree 인디케이터 fallback 의미가 불명확

`WorktreeList.tsx` L59~61:

```ts
const indicator: StatusDotState | null =
  aggregateIndicator ?? (activitySummary === undefined && agent ? agent.state : null);
```

`activitySummary`가 `undefined`일 때만 legacy `agent.state`로 폴백한다. 그런데 `selectWorktreeActivitySummaries`는 세션이 하나라도 있으면 빈 summary를 만들어내므로, 실제로 `undefined`가 되는 조건이 무엇인지 코드에서 명확하지 않다. 두 개의 활동 소스(`agents` prop과 `activityByWorktreePath`)가 공존하는 과도기 상태로 보인다.

---

## 4. 개선 방안

### 4.1 [P1] 활동 상태를 단일 도메인 모델로 통합

**핵심 원칙: `seen`을 유일한 읽음 진실 소스로 만들고, `unreadTabIds` / `unreadWorktreePaths`는 파생값으로 강등한다.**

현재 unread 맵이 별도로 존재하는 유일한 이유는 "벨은 세션 활동 엔트리를 만들지 않는다"는 점이다. 이를 활동 모델 안으로 흡수한다.

```ts
// ui/src/lib/activity.ts
export type AttentionReason = "completed" | "waiting" | "bell";

export type TerminalActivity = {
  state: TerminalActivityState;
  title: string;
  isAgent: boolean;
  agentType?: string;
  source?: "screen" | "title";
  agentSource?: "screen" | "title";   // 타입 계약 안으로 승격 (P2-1)
  /** 미확인 주목 요청. 없으면 사용자가 이미 읽은 상태. */
  attention?: { reason: AttentionReason; at: number };
};
```

- 벨은 `attention: { reason: "bell" }`을 세션 엔트리에 기록한다 → `unreadTabIds` 불필요.
- `waiting` 진입은 `attention: { reason: "waiting" }`을 기록한다 → **P1-2 해소**.
- 탭/워크트리/프로젝트 unread는 전부 `selectXxxActivitySummaries`가 하위 세션의 `attention` 존재 여부로 계산 → **P1-3 해소**, `WorktreeRow`의 prop 덮어쓰기(L54~58) 제거 가능.

마이그레이션: `unreadTabIds` / `unreadWorktreePaths`는 한 릴리스 동안 리듀서 내부에서 파생 계산해 기존 prop 시그니처를 유지하고, 소비자 전환 후 제거한다.

### 4.2 [P1] Dock 배지를 "나를 기다리는 세션 수"로 재정의

```ts
// 전 workspace 대상, 세션 단위
export function selectGlobalAttentionCount(): number {
  let count = 0;
  for (const [, snapshot] of listWorkspaceSnapshots()) {
    for (const activity of Object.values(snapshot.activityBySessionId ?? {})) {
      if (activity.attention) count += 1;
    }
  }
  return count;
}
```

- parked workspace 포함 → **P1-1 (1) 해소**
- `waiting` 포함 → **P1-1 (2) 해소**
- 세션 단위 → 숫자의 의미가 "N개의 에이전트가 당신을 기다립니다"로 명확해짐 → **P1-1 (3) 해소**

`App.tsx`에서는 이 값을 100~250ms coalesce 후 `setBadgeCount`에 넘긴다 → **P3-1 해소**.

### 4.3 [P1] `SESSION_TITLE_ACTIVITY`를 우선순위 테이블로 재작성

제어 흐름에 숨은 우선순위를 데이터로 끌어낸다.

```ts
type AgentIdentity = { isAgent: boolean; agentType?: string; agentSource?: "screen" | "title" };

/** 신원 확정 우선순위: 세션의 authoritative agentType > 타이틀 파싱 > 화면 판정 > 직전 값(단, in-flight일 때만) */
function resolveAgentIdentity(input: {
  authoritative: string | null;
  parsed: ParsedAgentTitle | null;
  screenManifest: string | null;
  previous: TerminalActivity | undefined;
  inFlight: boolean;
}): AgentIdentity { /* 순차 if, 3항 중첩 없음 */ }
```

`SESSION_SCREEN_ACTIVITY`와 `SESSION_TITLE_ACTIVITY`가 이 함수를 공유하면 두 리듀서 케이스의 신원 계산 중복(L1807~1821 vs L1919~1933)이 사라진다. 기존 동작을 고정할 회귀 테스트를 먼저 작성한 뒤 리팩터링할 것 — 이 로직은 실제 버그 수정의 누적이므로 무테스트 재작성은 금물이다.

### 4.4 [P2] `applySessionActivity` 동일성 비교 수정

`agentSource`(및 새 `attention`)를 비교 대상에 포함한다. 필드 추가 시 누락을 막으려면 명시적 키 목록을 쓴다.

```ts
const ACTIVITY_IDENTITY_KEYS = ["state","title","isAgent","agentType","source","agentSource"] as const;
const unchanged = previous && ACTIVITY_IDENTITY_KEYS.every((k) => previous[k] === activity[k])
  && attentionEquals(previous.attention, activity.attention);
```

동시에 `WorkspaceState.activityBySessionId`의 타입을 `Record<string, WorkspaceTerminalActivity>`로 좁히고 캐스팅(L1810, L1871)을 제거한다. `sessionPersistence.ts` L658~666에 `agentSource` 복원을 추가한다 → **P2-1 전체 해소**.

### 4.5 [P2] 원격을 데스크톱 selector의 투영으로 만들기

`App.tsx`가 원격에 보내는 pane 정보에 `attention` 여부를 포함하고, `RemoteSessionList`가 자체 `attentionRank`를 버리고 `activity.ts`의 `resolveActivityIndicator`를 재사용하도록 한다. 인디케이터 마크업도 `StatusDot`으로 통일한다(remote 번들이 이미 `agentIcon`을 공유하므로 추가 비용 없음) → **P2-2 해소**.

원격 전용으로 남아야 할 것은 크기 조정뿐이며, `StatusDot`에 `className`으로 이미 주입 가능하다.

### 4.6 [P2] 알림 edge 판정을 coordinator 한 곳으로

`App.tsx` L531~550의 `lastAgentStatesRef`를 삭제하고, 모든 타깃 상태를 매 렌더마다 coordinator에 그대로 넘긴다. coordinator가 자신의 `lastAgentState` 맵으로만 edge를 판정한다. 대신 coordinator에 세션 정리 API를 추가한다.

```ts
/** 살아있는 세션 집합 밖의 추적 상태를 버린다. */
retainSessions(liveSessionIds: ReadonlySet<string>): void;
```

`App.tsx`는 `activityNotificationTargets` 변경 시 이 함수를 호출한다 → **P2-3 해소 + 맵 누수 제거**.

### 4.7 [P2] 벨/완료 중복 알림을 양방향 억제로

완료·벨을 하나의 "attention 이벤트" 큐로 합치고, 세션 키당 억제 창(1.5초) 안에서는 먼저 도착한 하나만 발화시킨다. throttle 타임스탬프는 실제로 발화한 경우에만 갱신한다 → **P2-4 해소**.

```ts
private lastAttentionAt = new Map<string, number>();

private shouldEmit(key: string, now: number): boolean {
  const last = this.lastAttentionAt.get(key) ?? 0;
  if (now - last < this.attentionCoalesceMs) return false;
  this.lastAttentionAt.set(key, now);
  return true;
}
```

### 4.8 [P2] 인디케이터 의미 정리 — 색이 아니라 이유를 보여주기

`unread`(파랑)와 `done`(초록)의 이중화를 없애고 `attention.reason` 기준으로 단일화한다.

- `waiting` → 앰버 링 도트 + `title="에이전트가 입력을 기다립니다"`
- `completed` / `bell` → 초록 도트 + `title="완료됨 (미확인)"`
- 확인 완료 → 도트 없음 (에이전트 아이콘은 유지)

`SortableTab` L53의 덮어쓰기와 `WorktreeRow` L54~58의 덮어쓰기가 모두 제거된다. `StatusDot`은 `aria-hidden`이므로 툴팁은 감싸는 `span`의 `title`에 붙인다 → **P2-5 해소**.

### 4.9 [P3] 잔여 정리

- **StatusDot exhaustive 처리**: 알려진 상태를 모두 처리한 뒤 `default`에서 `never` 체크를 강제하고, 미지의 값은 실패 빨강이 아니라 중립(`bg-status-idle`)으로 떨어뜨린다 → P3-3.
- **Sidebar 오버레이 중복 계산 제거**: `summarizeProjectActivity` 결과를 지역 변수로 1회 계산 후 재사용 → P3-4.
- **attention frame cleanup 명시화**: 언마운트 전용 정리라면 `useEffect(() => () => { ... }, [])` + 세션 ID를 ref로 읽는 형태로 의도를 드러낸다 → P3-2.
- **worktree legacy fallback 제거**: `agents` prop 기반 폴백이 아직 필요한 경로가 있는지 확인하고, 없다면 `WorktreeList.tsx` L59~61과 `agents` prop 자체를 제거 → P3-5.

---

## 5. 권장 실행 순서

각 단계는 독립적으로 배포 가능하고, 앞 단계가 뒤 단계의 전제가 된다.

**1단계 — 회귀 잠금 (변경 없음)**
현재 동작을 고정하는 테스트를 먼저 채운다. 특히 `SESSION_TITLE_ACTIVITY`의 화면/타이틀 우선순위 조합, `applySessionActivity`의 born-acknowledged 규칙, `clearWorktreeUnreadWhenRead`의 다중 탭 케이스. 기존 테스트 자산(`workspaceActivity.test.tsx`, `activityStatePersistence.test.ts`, `worktreeActivityAcrossSwitch.test.ts`)이 이미 상당 부분을 덮고 있으므로 빈 곳만 보강한다.

**2단계 — P2-1 수정 (저위험, 실제 버그)**
`applySessionActivity` 동일성 비교에 `agentSource` 포함, 타입 계약 승격, 복원 경로 복사. 이건 명확한 결함이라 모델 통합 전에 단독으로 고칠 수 있다.

**3단계 — P1-1 / P1-2 배지 재정의**
`selectGlobalAttentionCount` 도입 + coalesce. `attention` 모델 도입 전이라면 `activityBySessionId`의 `!seen && (done|waiting)` 조건으로 먼저 구현해도 효과는 동일하다. 사용자 체감 개선이 가장 크다.

**4단계 — P1-3 모델 통합**
`attention` 필드 도입, unread 맵 파생화, UI 레이어 덮어쓰기 제거.

**5단계 — P1-4 / P2-3 / P2-4 리팩터링**
신원 해석 추출, edge 판정 단일화, 양방향 억제.

**6단계 — P2-2 / P2-5 / P3 표시 계층 통일**
원격 selector 재사용, 인디케이터 의미 정리, 잔여 항목.

---

## 6. 검증 계획

**자동 검증**
- `bun test --cwd ui` — 기존 activity/notification 관련 스위트가 전부 green이어야 한다. 대상: `workspaceActivity`, `workspaceNativeActivity`, `activityStatePersistence`, `activityRenderChain`, `worktreeActivityAcrossSwitch`, `screenActivity`, `notificationCoordinator`, `notificationSettings`, `notificationSoundWire`, `App.notifications`, `Sidebar.activity`, `RemoteAttention`, `TabBar`, `WorktreeList`.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` — `notification::badge`, `ipc::notifications` 스위트.

**수동 E2E (`bun tauri dev`로만 실행)**
데스크톱 확인이 필요한 항목은 자동화 불가이므로 아래를 사용자에게 요청해야 한다.

1. 프로젝트 A와 B를 등록하고 각각 에이전트를 띄운 뒤 앱을 백그라운드로 보낸다 → B 완료 시 Dock 배지가 증가하는가 (현재는 증가하지 않음).
2. 에이전트가 사용자 입력 대기(`waiting`)에 들어간 상태에서 앱 백그라운드 → Dock 배지에 반영되는가 (현재는 미반영).
3. 분할 pane 중 하나의 에이전트만 완료 → 해당 pane만 attention frame이 켜지고, 그 pane을 클릭하면 프레임·탭 도트·배지가 함께 꺼지는가 (현재는 프레임만 꺼짐).
4. 벨과 에이전트 완료가 거의 동시에 발생하는 에이전트로 알림이 한 번만 오는가.
5. 모바일 원격에서 데스크톱과 동일한 인디케이터 상태가 보이는가, 데스크톱에서 확인한 항목이 원격에서도 해제되는가 (현재는 해제되지 않음).

---

## 7. 요약

| 우선순위 | 항목 | 핵심 증상 |
|---|---|---|
| P1-1 | Dock 배지 정의 | 다른 프로젝트·`waiting` 미집계, 탭 단위라 의미 모호 |
| P1-2 | `waiting`이 unread 미승격 | 계층별 신호 불일치의 근원 |
| P1-3 | `seen` / `unread` 이중 진실 | UI가 selector 결과를 덮어써서 봉합 중 |
| P1-4 | 타이틀 활동 분기 폭발 | 우선순위가 제어 흐름에 은닉 |
| P2-1 | `agentSource` 계약 이탈 | 동일성 비교 누락 → 갱신 유실, 복원 시 소실 |
| P2-2 | 원격 규칙 분기 | 랭킹·마크업 재구현, 읽음 미반영 |
| P2-3 | 알림 상태 3중 추적 | 맵 누수 + 생명주기 불일치 |
| P2-4 | 벨/완료 단방향 억제 | 순서 역전 시 중복 알림 |
| P2-5 | `done` / `unread` 색 의미 불명 | 사용자에게 구분 근거 없음 |
| P3 | 배지 IPC 스로틀 / cleanup 의존성 / StatusDot fallback / Sidebar 중복 계산 / worktree legacy 폴백 | 견고성·성능·정리 |

가장 큰 구조적 승리는 **4.1의 `attention` 모델 단일화**다. P1-2, P1-3, P2-2, P2-5가 전부 그 하나에서 파생된 증상이며, 통합하면 UI 레이어의 덮어쓰기 두 곳이 함께 사라진다. 반면 사용자 체감이 가장 즉각적인 것은 **4.2의 Dock 배지 재정의**이므로, 모델 통합을 기다리지 말고 3단계에서 먼저 처리할 것을 권한다.
