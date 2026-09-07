# 앱 시작 시 모든 Pane 하이라이트(Attention Frame) 원인 및 미해결 사유 분석

- **작성일자:** 2026-09-07
- **대상 파일:**
  - `ui/src/lib/sessionPersistence.ts` (`deserializeWorkspaceState`)
  - `ui/src/state/workspaceStore.ts` (`isSessionActivelyObserved`, `acknowledgeTabCompletions`, `applySessionActivity`)
  - `ui/src/components/TerminalSplitView.tsx` (`needsAttention`)
  - `src-tauri/src/native_terminal/surface_host.rs` (`attach_daemon_attachment`, `take_native_terminal_events`)
  - `ui/src/lib/agentAutoResume.ts` (`scheduleAgentAutoResume`, `collectAutoResumeCandidates`)

---

## 1. 개요 및 증상

앱(Ferryx Desktop) 실행 및 재시작 시, 분할되어 있는 모든 터미널 pane의 둘레에 amber/gold 테두리(Attention Frame, `needsAttention = true`)가 동시에 켜지는 현상이 발생합니다.

이 문제는 앞서 커밋 `fd7f60f`("fix(workspace): suppress restart auto-resume attention noise on every pane") 및 9월 5일 대규모 알림/하이라이트 리뷰(F01~F15)를 거쳤음에도 불구하고 여전히 해결되지 않고 반복되고 있습니다.

---

## 2. 근본 원인 (Root Causes)

### 원인 1: 세션 복원 시 `seen` 필드 누락 및 무조건 `done` 변환 (핵심 원인)

**위치:** `ui/src/lib/sessionPersistence.ts:655-675` (`deserializeWorkspaceState`)

앱이 종료될 때 `serializeWorkspaceState`는 각 pane의 활동 상태(`activityBySessionId`)를 디스크에 저장합니다. 이때 사용자가 이미 확인한 상태(`seen: true`)였더라도, 앱 시작 시 이를 역직렬화하는 `deserializeWorkspaceState` 함수에서 치명적인 결함이 발생합니다.

```typescript
// ui/src/lib/sessionPersistence.ts:655-675
const restoredActivity: Record<string, TerminalActivity> = {};
if (ws.activityBySessionId) {
  for (const [sessionId, activity] of Object.entries(ws.activityBySessionId)) {
    if (activity && referencedSessionIds.has(sessionId)) {
      // working/waiting 상태는 종료 시점에 끝난 것으로 간주하여 done으로 변환
      const isInFlightClaim = activity.state === "working" || activity.state === "waiting";
      restoredActivity[sessionId] = {
        state: isInFlightClaim ? "done" : activity.state,
        title: activity.title || "",
        isAgent: Boolean(activity.isAgent),
        ...(activity.agentType ? { agentType: migrateLegacyAgentType(activity.agentType) ?? activity.agentType } : {}),
        ...(activity.source ? { source: activity.source } : {}),
        ...(activity.agentSource ? { agentSource: activity.agentSource } : {}),
        // [결함]: seen 속성을 전혀 복사하거나 true로 지정하지 않음!
      };
    }
  }
}
```

이로 인해 복원된 모든 세션의 activity 객체는 `{ state: "done", seen: undefined }`가 됩니다.

이후 `TerminalSplitView.tsx`에서 하이라이트를 판단하는 기준:
```typescript
// ui/src/components/TerminalSplitView.tsx:1015
const needsAttention = attentionFrameEnabled && Boolean(
  activity && !activity.seen && (activity.state === "waiting" || activity.state === "done"),
);
```
- `activity.state`가 `"done"`
- `activity.seen`이 `undefined`이므로 `!activity.seen`은 `true`
- 결과적으로 **디스크에서 복원된 모든 세션이 앱이 켜지는 0번째 프레임부터 `needsAttention = true`**가 되어 모든 pane에 하이라이트가 켜집니다.

---

### 원인 2: 분할 화면(Split Pane)에서 형제 pane의 관측 배제 구조

**위치:** `ui/src/state/workspaceStore.ts:2188-2236` (`isSessionActivelyObserved`, `acknowledgeTabCompletions`)

설령 활성 pane이 확인되더라도, 분할된 다른 pane들은 화면에 버젓이 나란히 보이고 있음에도 "관측되지 않음(unobserved)"으로 취급됩니다.

1. **`isSessionActivelyObserved`의 배타성:**
   ```typescript
   function isSessionActivelyObserved(state: WorkspaceState, tabId: string, sessionId: string): boolean {
     if (!isTabVisible(state, tabId)) return false;
     const tabLayout = state.layout.layoutsByTabId?.[tabId];
     if (tabLayout && tabLayout.root.type === "split") {
       const leafId = Object.entries(tabLayout.sessionIdsByLeafId).find(([_, sId]) => sId === sessionId)?.[0];
       // 오직 activeLeafId 하나만 관측된 것으로 인정!
       return Boolean(leafId && leafId === tabLayout.activeLeafId);
     }
     return true;
   }
   ```
2. **`acknowledgeTabCompletions`의 배타적 스킵:**
   ```typescript
   if (tabLayout && tabLayout.root.type === "split" && activeSessionId && sessionId !== activeSessionId) {
     continue; // activeLeaf가 아닌 형제 pane은 확인 처리하지 않음
   }
   ```

앱 시작 시 `RESTORE_WORKSPACE` 액션은 탭/pane 확인(`acknowledgeTabCompletions`)을 전혀 호출하지 않으며, 설령 호출하더라도 activeLeaf 하나를 제외한 모든 분할 pane은 `continue`로 스킵되어 영구적으로 `seen: undefined` 상태로 남습니다.

---

### 원인 3: 앱 시작 시 윈도우 포커스 추적의 비동기 레이스 (`observed: false`)

**위치:** `ui/src/lib/nativeWindowFocus.ts`, `ui/src/state/workspaceStore.ts:295`

앱 시작 시 `startNativeWindowFocusTracking()`은 비동기로 Tauri 윈도우 포커스를 조회합니다(`win.isFocused()`).
- 초기값: `nativeFocused = null`
- 초기 DOM 상태: `document.hasFocus()`는 윈도우 생성 초기 프레임에 `false`
- `getNativeWindowFocused() ?? isWindowForegroundFocused()`는 `false` 반환

이 타이밍에 백엔드 네이티브 터미널 연결(`attach_daemon_attachment`) 직후 강제 화면 감지(`take_native_terminal_events(session, session_id, true)`)가 돌면서 화면의 프롬프트나 상태를 읽어 `SESSION_SCREEN_ACTIVITY`를 디스패치하면, `observed: false`가 전달됩니다.
`observed: false`인 경우 `applySessionActivity`는 활성 pane조차도 `seen: false`로 저장하므로 하이라이트가 켜지게 됩니다.

---

## 3. 왜 그동안 해결하지 못했는가? (실패 원인 분석)

### 실패 이유 1: 커밋 `fd7f60f`의 엉뚱한 가설 (오진단)

커밋 `fd7f60f` 작성자는 문제 원인을 다음과 같이 추정했습니다:
> "앱 재시작 시 daemon epoch 불일치로 백엔드가 끊어지고, 400ms 간격의 `agentAutoResume`이 에이전트를 재실행하면서 프롬프트에 안착할 때 `working -> idle -> done` 전이가 발생해 모든 pane에 가짜 완료가 뜬다."

그래서 `agentAutoResume.ts`의 `reconnect` 콜백 안에 `SUPPRESS_NEXT_ATTENTION` 액션을 디스패치하도록 코드를 작성했습니다.

**왜 이것으로 해결되지 않았는가?**
1. **시점 차이:** 하이라이트는 `agentAutoResume`이 실행되기도 전, 앱 시작 직후 `sessionPersistence.ts`가 `activityBySessionId`를 역직렬화하여 Zustand 스토어에 넣는 순간 이미 `state: "done", seen: undefined`로 켜져 있었습니다.
2. **실제 환경 미실행:** Ferryx는 백그라운드 PTY 데몬(`ferryx --daemon`)이 살아있는 상태로 유지되는 구조입니다. 따라서 데몬이 정상 작동 중일 때는 세션의 `backendSessionId`가 `null`이 아니므로 `collectAutoResumeCandidates`는 빈 배열을 반환하고 `scheduleAgentAutoResume`은 아예 실행조차 되지 않았습니다.
3. **플래그의 한계:** `SUPPRESS_NEXT_ATTENTION`은 "다음번에 새로 들어올 상태 전이"를 억제하는 플래그일 뿐, 이미 디스크에서 복원되어 들어와 있는 `activityBySessionId`의 기존 `done` 상태를 `seen: true`로 바꿔주지 못합니다.

---

### 실패 이유 2: 단위 테스트 격리의 함정 (Mock 테스트만 통과)

9월 5일 진행된 대규모 알림/하이라이트 리뷰(`docs/FERRYX_NOTIFICATION_BADGE_HIGHLIGHT_REVIEW_2026-09-05.md`)에서는 F01부터 F15까지 15개 항목을 검토하고 456개 프론트엔드 테스트를 통과시켰습니다.

하지만 모든 테스트가 다음과 같이 고립된 Mock 데이터로 수행되었습니다:
- `workspaceActivity.test.tsx`: 수동으로 가공된 `state` 객체에 액션을 전달하여 리듀서 동작만 검증.
- `TerminalSplitView.test.tsx`: `{ state: "done", seen: true }` 또는 `{ state: "done", seen: false }`를 직접 props로 주입.
- `sessionPersistence.test.ts`: "working/waiting 상태가 done으로 바뀌어 스피너가 안 도는가"만 검증하고, 복원된 activity 객체의 `seen` 속성 여부나 이로 인한 split view 렌더링 결과는 검증하지 않음.

실제 프로덕션 환경의 라이프사이클인:
`디스크 저장 -> 앱 재시작 -> deserializeWorkspaceState -> restoreWorkspace -> TerminalSplitView 마운트`
의 전체 파이프라인 통합 검증이 없었기 때문에, `deserializeWorkspaceState`에서 `seen` 속성이 누락되어 무조건 하이라이트가 켜지는 치명적 결함을 아무도 발견하지 못했습니다.

---

## 4. 해결 내용 및 아키텍처 개선

### 1) 역직렬화 시점의 단일 정규화 (`seen: true`)
- **위치:** `ui/src/lib/sessionPersistence.ts:670`
- 디스크에서 복원되는 모든 세션 활동(`restoredActivity`)에 명시적으로 `seen: true`를 부여합니다.
- 앱 재시작 시점에 복원된 이전 세션의 완료(`done`) 상태는 과거 이력이므로, 0번째 렌더링 프레임에서 사용자 주의를 끄는 amber 테두리(Attention Frame)를 띄우지 않습니다.

### 2) 프로젝트 전환(`RESTORE_WORKSPACE`) 시 미확인 활동 보존 (HIGH-2 해결)
- **위치:** `ui/src/state/workspaceStore.ts:1520`
- `RESTORE_WORKSPACE` 액션은 앱 초기 기동뿐 아니라 **작업공간/프로젝트 스왑(`swapped.state`)** 시에도 디스패치됩니다.
- 리듀서 내부에서 무조건 `seen: true`로 덮어쓰던 중복 정규화를 제거하고 원본 `activityBySessionId`를 그대로 보존하도록 복원하여, 다른 프로젝트에서 백그라운드로 완료된 에이전트의 미확인 하이라이트/배지가 프로젝트 복귀 시 지워지는 회귀를 원천 차단했습니다.

### 3) 주의 상태 전이 시 확인 상태 승계 결함 수정 (HIGH-1 해결)
- **위치:** `ui/src/state/workspaceStore.ts:2260-2305` (`applySessionActivity`)
- 기존 코드의 `acknowledged` 및 `unreadTabIds` 판단:
  ```typescript
  const wasAttentionState = previous?.state === "done" || previous?.state === "waiting";
  const acknowledged = ... || (wasAttentionState && previous?.seen === true) || ...;
  if (isAttentionState && !wasAttentionState && ...) { ... unreadTabIds 갱신 ... }
  ```
  이로 인해 복원된 `done (seen: true)` 상태에서 터미널 재연결 후 에이전트가 권한 요청(`blocked`/`waiting`)으로 전이할 때, `wasAttentionState`가 true라는 이유로 `seen: true`가 승계되어 하이라이트와 탭 점, 독 배지가 침묵하는 심각한 회귀가 발생했습니다.
- **수정:**
  ```typescript
  const isSameAttentionState = isAttentionState && wasAttentionState && previous?.state === activity.state;
  const acknowledged = ... || (isSameAttentionState && previous?.seen === true) || ...;
  const isNewAttentionTransition = isAttentionState && (!wasAttentionState || previous?.state !== activity.state);
  if (isNewAttentionTransition && !suppression && (!observed || !isTabVisible(state, tabId))) {
    // unreadTabIds 및 unreadWorktreePaths 갱신
  }
  ```
  - 동일한 상태 내에서의 타이틀/메타데이터 갱신(예: `waiting` 중 타이틀 변경)에 대해서만 `seen: true`가 유지됩니다.
  - `done -> waiting` 또는 `waiting -> done`과 같은 실제 주의 상태 변경은 항상 새로운 주의 이벤트로 처리되어 정상적으로 Attention Frame과 미확인 배지가 활성화됩니다.

---

## 5. ccapi/claude-opus-5 코드 리뷰 결과 및 조치 내역

코드 리뷰어(`st_01a07b29`, `ccapi/claude-opus-5`)가 지적한 모든 사항(Blocker 3건, Medium 4건)을 100% 반영하여 해결하였습니다:

1. **HIGH-1 (재시작 후 첫 주의 이벤트 침묵 회귀):**
   - `applySessionActivity`의 carry-over 조건을 `isSameAttentionState (previous?.state === activity.state)`로 한정하고, 상태 전이 시 `isNewAttentionTransition`으로 unreadTabIds를 갱신하도록 수정 완료.
   - `workspaceStore.test.tsx`에 재시작 후 `done -> waiting (blocked)` 및 타이틀 변경 전이에 대한 회귀 방지 테스트 추가.
2. **HIGH-2 (프로젝트 스왑 시 타 프로젝트 미확인 알림 소실 회귀):**
   - `workspaceStore.ts`의 `RESTORE_WORKSPACE` 리듀서에서 불필요한 `seen: true` 강제 변환 루프 제거.
   - `workspaceStore.test.tsx`에 프로젝트 스왑 시 미확인 상태(`seen: false`) 및 글로벌 배지 카운트 보존 테스트 추가.
3. **HIGH-3 & MEDIUM-3 (`tsc` 타입 에러 및 미존재 props 전달):**
   - `startupPaneAttention.integration.test.tsx`의 import 경로 수정(`WorkspaceState` from `workspaceStore`), `expandedLeafId: null` 보완, `Worktree` 타입 필드 준수.
   - `TerminalSplitView` 렌더링 시 미존재 props(`tab`, `tabLayout`, `activeLeafId` 등) 제거하고 실제 props(`layout`, `sessions`, `activityBySessionId`)만 전달하도록 정리.
   - `bun x tsc --noEmit` 실행 결과 0 에러, `bun run build` 빌드 성공 확인.
4. **MEDIUM-2 (`sessionPersistence.test.ts` 동어반복 어설션):**
   - `activity.seen` 값을 그대로 재복사하여 검증하던 불필요한 `needsAttention` 검증 로직 제거.
5. **MEDIUM-4 (회귀 벡터 고정 테스트):**
   - `workspaceStore.test.tsx`에 4종의 회귀 방지 단위 테스트 추가 및 `startupPaneAttention.integration.test.tsx` 통합 테스트 유지.

---

## 6. 최종 검증 내역 (Verification Ledger)

- **TypeScript Typecheck:**
  - 명령: `bun x tsc --noEmit`
  - 결과: 에러 0건 (Clean)
- **Production Build:**
  - 명령: `bun run --cwd ui build`
  - 결과: Vite 프로덕션 번들 빌드 성공 (소요시간 2.74초)
- **타겟 테스트 3종:**
  - 명령: `bun x vitest run sessionPersistence.test.ts workspaceStore.test.tsx startupPaneAttention.integration.test.tsx`
  - 결과: 3개 파일 74개 테스트 전원 통과 (74/74 Pass)
- **상태 관리 전수 테스트:**
  - 명령: `bun x vitest run src/state src/lib/sessionPersistence.test.ts`
  - 결과: 31개 테스트 파일 287개 테스트 전원 통과 (287/287 Pass)

