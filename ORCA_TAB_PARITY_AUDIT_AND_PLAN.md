# Orca-lite Tab Parity Audit & Implementation Plan

> 작성일: 2026-08-22  
> 대상: 현재 `orca-lite` working tree의 탭/분할/드래그/터미널 pane/복원 구현  
> canonical 비교 기준: 저장소에 포함된 `ui/original-dist/`의 원본 Orca renderer bundle  
> 주의: 감사 시점 working tree에는 기존 미커밋 변경이 다수 존재하므로, 본 작업은 기존 구현을 덮어쓰지 않고 이 리포트만 추가한다.

## 1. 결론

현재 Orca-lite의 탭 구현은 **기초 상태 모델은 Orca 방향으로 상당히 진전되었지만, 핵심 parity는 아직 미완성**이다.

특히 다음 세 가지는 릴리스 차단급으로 보는 것이 맞다.

1. **터미널 split은 상태 tree만 분리되고 실제 독립 terminal pane이 만들어지지 않는다.**
   - `ui/src/state/workspaceStore.ts:191-221`의 `splitPane()`은 새 leaf에 기존 `TerminalSession`을 그대로 재사용한다.
   - `ui/src/lib/terminalHostManager.ts:33-62`는 `session.id`당 xterm/DOM instance 하나만 캐시한다.
   - `ui/src/components/TerminalPane.tsx:68-73`은 그 단일 DOM element를 새 container에 `replaceChildren()`으로 붙인다.
   - 따라서 같은 session id를 두 leaf에 연결하면 xterm element가 두 pane에 동시에 존재하는 것이 아니라 한 pane에서 다른 pane으로 **re-parent**된다.
   - 원본 Orca는 `PaneManager.splitPane()`에서 새 pane을 생성하고 새 terminal을 열며, pane-created lifecycle을 통해 독립 PTY를 연결한다 (`ui/original-dist/assets/OnboardingInlineCommandTerminal-B5SNC4Sp.js:14496-14532`).

2. **tab split은 만들 수 있지만 기존 split group으로 다시 합치는 merge 경로가 없다.**
   - Orca-lite는 `MOVE_TAB_TO_SPLIT`으로 항상 새 single-tab group을 만든다 (`ui/src/state/layout.ts:218-280`).
   - 반면 원본 Orca의 drag end는 동일 group이면 reorder, 다른 group의 tab/index 또는 pane body 위에 놓으면 그 group으로 `dropUnifiedTab(... { groupId, index? })`, edge면 새 split로 처리한다 (`ui/original-dist/assets/rename-file-JaIUF221.js:4180-4630`).
   - 현재 Orca-lite에는 `MOVE_TAB_TO_GROUP`/`DROP_TAB`에 해당하는 reducer/action이 없다.

3. **독립 터미널 split을 구현하려 해도 현재 백엔드의 1-writer-1-worktree lease가 두 번째 PTY spawn을 막는다.**
   - `src-tauri/src/terminal/pty.rs:56-87`의 `spawn_in_worktree()`는 worktree별 writer lease를 획득하고, 기존 active owner가 있으면 spawn을 거부한다.
   - `src-tauri/src/ipc/terminal.rs:16-21, 85-125`의 spawn request는 worktree만 받고 현재 pane의 실제 CWD를 지정할 방법도 없다.
   - 원본 Orca의 split은 현재 pane CWD를 구한 뒤 새로운 pane/PTY를 생성한다 (`OnboardingInlineCommandTerminal-B5SNC4Sp.js:1285-1330`).
   - 따라서 UI만 고쳐서는 true split parity를 달성할 수 없고, **백엔드 concurrency contract를 먼저 결정해야 한다.**

종합 판정은 다음과 같다.

- 탭 추가/닫기, group-local 기본 reorder, tab-group binary split tree, divider resize, per-tab terminal pane tree, 기본 pane focus: **부분 구현/작동**
- tab edge drag → 새 split group: **부분 구현, transaction/focus/merge가 불완전**
- terminal pane split: **구조는 있으나 런타임 의미론은 깨짐**
- cross-group tab merge: **미구현**
- terminal pane → 새 tab detach + PTY ownership transfer: **미구현**
- group-aware close left/right/others: **UI는 group-aware처럼 보이지만 action은 global이라 잘못 동작 가능**
- session/layout persistence: **group tree는 저장되지만 terminal/browser/session continuity에 중요한 결함 존재**
- OS native window로의 tab tear-off: **현재 Lite에는 없으며, bundled original Orca에서도 canonical parity 기능이라는 근거를 찾지 못함. 별도 제품 요구로 취급해야 한다.**

---

## 2. 용어를 먼저 분리해야 하는 이유

이 코드베이스에는 서로 다른 네 종류의 “분할/분리”가 섞여 있다.

| 용어 | 의미 | 원본 Orca | Orca-lite 현재 |
|---|---|---|---|
| Tab reorder | 같은 tab group 내부 순서 변경 | 지원 | 부분 지원 |
| Tab-group split / merge | 탭 하나를 새 split column/row로 빼거나 기존 group에 다시 넣기 | 지원 | split만 지원, merge 없음 |
| Terminal pane split | 하나의 terminal tab 안에서 독립 terminal pane을 좌/우 또는 상/하로 생성 | 지원 | 상태 tree만 분리, 실제 독립 xterm/PTY 아님 |
| Terminal pane → tab detach | split된 terminal pane을 tab strip에 드롭하여 새 tab으로 승격, 기존 PTY 유지 | 지원 | 미구현 |
| OS native window tear-off | tab을 별도 top-level Tauri/Electron window로 분리 | canonical 증거 미확인 | 미구현 |

사용자가 말하는 “탭 창 분리/합치기”는 최소한 **tab-group split/merge**를 포함한다. 원본 bundle에서 확실히 입증되는 parity 범위는 tab-group split/merge와 terminal-pane-to-tab detach까지이다.

---

## 3. 현재 Orca-lite 아키텍처

### 3.1 두 개의 독립 binary tree

현재 상태 모델은 좋은 방향으로 분리되어 있다.

#### A. workspace-level tab-group layout

`ui/src/lib/types.ts:160-190`

- `TabGroup { id, tabIds, activeTabId }`
- `TabGroupLayoutNode`
  - leaf: `{ type: "group", groupId }`
  - split: `{ type: "split", direction, first, second, ratio }`
- `LayoutState.tabGroups`
- `LayoutState.tabGroupLayout`
- `LayoutState.focusedGroupId`

이 구조는 원본 Orca의 `groupsByWorktree` + `layoutByWorktree` 개념과 잘 대응한다.

#### B. tab-level terminal pane layout

`ui/src/state/paneTree.ts`와 `LayoutState.layoutsByTabId`

- 각 terminal tab이 별도의 `PaneNode` binary tree를 소유한다.
- leaf는 `leafId`, split은 `direction + first + second + ratio`를 가진다.
- `TabPaneLayout.sessionIdsByLeafId`가 pane leaf와 terminal session을 연결한다.

이 역시 원본 Orca의 `terminalLayoutsByTabId`와 개념적으로 일치한다.

### 3.2 현재 normalization의 장점

`ui/src/state/layout.ts:394-545`의 `normalizeLayout()`은 다음 invariant를 어느 정도 자동 복구한다.

- tab 중복 제거
- 모든 tab에 pane layout 생성
- 모든 tab을 정확히 한 group에 배정
- 빈/유효하지 않은 group pruning
- group split tree에 없는 group 자동 append
- focused group/active tab 유효성 복구
- legacy state에 `group-default` 자동 생성

이 normalization layer는 유지할 가치가 있다. 다만 migration과 runtime mutation의 책임을 전부 normalization에 맡기지 말고, 아래 계획처럼 명시적 reducer invariant 테스트를 추가해야 한다.

---

## 4. 원본 Orca에서 확인된 canonical 동작

### 4.1 Tab drag는 dnd-kit 계열 primitive를 사용한다

원본 bundle에는 `DndContext`, `SortableContext`, `useSortable`, `useDroppable`, `DragOverlay`, `pointerWithin`, `closestCenter`가 포함되어 있고, terminal tab surface가 실제로 이를 사용한다.

- primitive bundle: `ui/original-dist/assets/rename-file-JaIUF221.js`
- 실제 `DndContext` 사용: `ui/original-dist/assets/Terminal-qm6WvB4Q.js:2440-2760`
- drag orchestration: `rename-file-JaIUF221.js:4180-4630`

원본 drag lifecycle은 단순 reorder가 아니다.

1. 12px activation distance
2. drag 시작 시 split geometry/활성 group snapshot
3. pointer + collision 기반 target 계산
4. tab insertion target과 group body/edge target을 구분
5. hover 중 필요한 active tab preview
6. drop 시 한 transaction으로 다음 중 하나 수행
   - same group: reorder
   - other group tab/index: move into existing group
   - other group body: move/merge into existing group
   - group edge: new split group
7. pointerup/cancel/blur 같은 missed-end fallback 처리
8. cross-group drop 뒤 source/target activation 정리

### 4.2 “Move Tab to Split”과 “Split terminal”은 다른 기능이다

원본 tab context menu는 둘을 명확히 분리한다.

`ui/original-dist/assets/rename-file-JaIUF221.js:3300-3730`

- **Move Tab to Split**
  - right / left / down / up
  - whole tab이 새 tab group으로 이동
  - source group에 tab이 둘 이상일 때 활성
- **Split terminal**
  - right / down
  - 현재 terminal tab 안에서 새 terminal pane 생성

현재 Orca-lite `ui/src/components/TabBar.tsx:500-560`에는 “Split terminal right/down”만 있고 whole-tab “Move Tab to Split” 메뉴가 없다. 현재 whole-tab split은 drag edge 경로로만 노출된다.

### 4.3 Terminal split은 독립 pane + 독립 PTY이다

원본 `splitTerminalPaneWithInheritedCwd()`는 source pane의 현재 CWD를 먼저 구하고 새 pane을 생성한다.

- CWD resolution: `OnboardingInlineCommandTerminal-B5SNC4Sp.js:1285-1330`
- `PaneManager.splitPane()`: `:14680-14720`
- actual split/new terminal: `:14496-14532`

즉 원본의 split은 “한 PTY를 두 xterm view로 미러링”하는 기능이 아니다.

### 4.4 Terminal pane을 새 tab으로 detach할 수 있다

원본 `detachTerminalPaneToTab()`은 다음을 수행한다.

`OnboardingInlineCommandTerminal-B5SNC4Sp.js:28195-28240`

1. source tab layout에서 leaf를 detach
2. leaf가 소유한 `ptyId`를 보존
3. source PaneManager에서 renderer를 external move 용도로 detach
4. target group에 새 tab 생성
5. detached layout을 새 tab에 연결
6. `syncPaneDetachPtyOwnership()`으로 PTY ownership 이동
7. 새 tab 활성화

외부 pane drop target은 terminal tab strip에서 계산된다 (`:30020-30160`).

이 기능은 backend PTY를 새로 spawn하지 않으므로, 정확한 ownership transfer만 구현하면 현재 writer lease와 충돌하지 않는다.

---

## 5. 기능별 parity matrix

상태 정의:

- **OK**: 핵심 의미론이 원본과 대체로 일치
- **PARTIAL**: 경로는 있으나 중요한 parity 차이/bug 존재
- **BROKEN**: UI/state는 존재하지만 실제 동작 의미론이 성립하지 않음
- **MISSING**: canonical 기능 자체가 없음
- **UNCONFIRMED**: 원본 canonical 기능인지 입증되지 않음

| 기능 | 상태 | Orca-lite 진단 | 핵심 근거 |
|---|---|---|---|
| 새 terminal tab | OK/PARTIAL | focused group에 추가되는 기본 구조는 존재 | `workspaceStore.ts:createSpawnedTab/openTab`, `normalizeLayout` |
| tab close | PARTIAL | 기본 close는 동작. pinned/running-process UX는 원본보다 단순 | `workspaceStore.ts:223-275` |
| last tab close 후 valid state | PARTIAL | close→wait→spawn 순서로 과거 lease conflict는 개선 | `workspaceStore.ts:239-260` |
| same-group tab reorder | PARTIAL | custom pointer reorder 구현. cross-group 요소를 구분하지 않는 document-wide selector bug 가능 | `TabBar.tsx:135-171` |
| drag activation threshold | OK | 12px | `TabBar.tsx:210-217` |
| tab edge drop → new split group | PARTIAL | 4 edge 판정/preview 있음. 원본처럼 단일 DnD transaction은 아님 | `TerminalSplitView.tsx:105-184` |
| tab merge into existing group | MISSING | reducer/action/drop target 없음 | `layout.ts:REORDER_TAB`, `MOVE_TAB_TO_SPLIT`만 존재 |
| group body/strip center drop | MISSING | edge가 아니면 null | `TerminalSplitView.tsx:476-503` |
| tab split context menu 4방향 | MISSING | whole-tab 메뉴 없음; terminal split right/down만 있음 | `TabBar.tsx:500-560` |
| terminal split right/down | BROKEN | same `TerminalSession.id` 재사용 → single xterm DOM reparent | `workspaceStore.ts:207-220`, `TerminalHostManager`, `TerminalPane` |
| terminal split CWD inheritance | MISSING | backend spawn에 explicit cwd/get-cwd 계약 없음 | `src-tauri/src/ipc/terminal.rs:16-21` |
| pane divider resize | OK/PARTIAL | pointer resize + cleanup 있음 | `TerminalSplitView.tsx:810-875` |
| tab-group divider resize | OK/PARTIAL | 같은 divider primitive 사용, ratio reducer 존재 | `TerminalSplitView.tsx:240-292`, `layout.ts:282-288` |
| pane focus click | OK | `FOCUS_PANE`이 active tab/group도 동기화 | `layout.ts:320-342` |
| focus next/previous pane | PARTIAL | current tab leaf 순환 구현. 독립 split이 깨져 실제 UX 검증 필요 | `App.tsx:382-398` |
| pane drag reorder | PARTIAL | same-tab leaf swap만 지원; HTML5 DnD | `TerminalSplitView.tsx:650-730` |
| pane → new tab detach | MISSING | tab strip external pane target/PTY ownership transfer 없음 | 원본 대비 gap |
| browser unified tab split/merge | MISSING | `workspaceStore.splitPane()`이 browser source/target 모두 거부 | `workspaceStore.ts:195-205` |
| close others/left/right | BROKEN after group split | menu는 group tabs 기준인데 store action은 flattened global `layout.tabs` 기준 | `TabBar.tsx` + `workspaceStore.ts:301-332` |
| keyboard next/prev tab | PARTIAL | flattened global `layout.tabs` 순환; group-aware UX contract가 명시되지 않음 | `App.tsx:353-363` |
| drag hover activation/restore | MISSING | 원본의 pre-drag activation snapshot/preview/rollback 없음 | custom pointer state only |
| group split state persistence | OK/PARTIAL | `tabGroups/tabGroupLayout/focusedGroupId` 저장/복원 | `sessionPersistence.ts:42-61, 157-180` |
| terminal PTY continuity restore | BROKEN/PARTIAL | local `session.id`를 persisted `sessionId`로 기록해 backend live id와 비교가 어긋남 | `sessionPersistence.ts:63-76, 103-118` |
| browser tab persistence | BROKEN | persisted tab에 `kind`가 없고 deserialize는 `paneTree.type === "browser"`를 검사하나 `PaneNode`는 leaf/split뿐 | `types.ts:298-307`, `sessionPersistence.ts:121-144` |
| pinned state persistence | MISSING | `PersistedTab`에 pinned 없음 | `types.ts:298-307` |
| expanded pane persistence | MISSING | serialize/deserialize가 `expandedLeafId`를 보존하지 않음 | `sessionPersistence.ts` |
| native OS window tab tear-off | UNCONFIRMED | Lite production code에 `WebviewWindow` 생성 경로 없음. 원본에서도 tab parity 기능 근거 미발견 | 별도 제품 요구로 분리 |

---

## 6. 현재 구현의 구체적 문제와 root cause

### P0-1. Terminal split이 같은 xterm을 두 leaf에 연결한다

현재 `splitPane()`은 target leaf의 session을 찾은 뒤 그대로 `SPLIT_PANE` action에 넣는다.

결과적으로 `sessionIdsByLeafId`는 다음 같은 상태가 될 수 있다.

```text
leaf-a -> session-1
leaf-b -> session-1
```

하지만 `TerminalHostManager`의 instance key도 `session-1`이다.

```text
session-1 -> { xterm instance, one HTMLDivElement }
```

DOM node는 동시에 두 부모를 가질 수 없으므로 두 번째 `TerminalPane`이 mount되면 첫 pane에서 element가 빠진다.

#### 수정 원칙

true terminal split에서는 반드시 다음 invariant가 성립해야 한다.

```text
terminal leaf 1개 -> local TerminalSession 1개 -> live backend PTY 1개
```

예외적으로 pane-to-tab detach 시에는 **동일 session 자체의 ownership을 이동**해야 하며 복제하지 않는다.

### P0-2. Backend writer lease와 Orca split parity가 구조적으로 충돌한다

현재 `spawn_in_worktree()`는 worktree canonical path를 key로 terminal session 하나만 writer로 허용한다.

따라서 UI에서 split할 때 독립 PTY를 spawn하도록 바꾸면 두 번째 split에서 `WriterAlreadyActive`가 발생한다.

이 문제는 workaround로 같은 PTY/xterm을 공유해서 해결할 수 없다. 그것은 Orca의 split 의미론과 다르다.

#### 권장 결정

**interactive terminal PTY는 worktree-wide exclusive writer lease 대상에서 제외하거나 lease scope를 재설계한다.**

권장 모델:

- interactive terminal session: 동일 worktree에 복수 허용
- destructive worktree operation: active terminal/session 존재 여부와 별도 safety gate로 보호
- agent/automation에 정말 exclusive writer invariant가 필요하다면 `writer lease`를 “agent writer ownership”으로 좁히고 terminal lifecycle과 분리

만약 “worktree당 terminal도 무조건 1개”라는 제품 invariant를 유지해야 한다면, **Orca terminal split parity는 달성 불가능**하다는 결정을 문서화해야 한다.

### P0-3. Cross-group merge가 없다

`MOVE_TAB_TO_SPLIT`은 항상 new group id를 만든다. source tab을 기존 group의 특정 index에 삽입하는 primitive가 없다.

필요 reducer primitive:

```ts
MOVE_TAB_TO_GROUP {
  tabId: string;
  targetGroupId: string;
  targetIndex?: number;
}
```

이 action은 다음을 한 transaction으로 해야 한다.

1. source group에서 tab 제거
2. source group이 비면 split tree에서 leaf 제거 및 parent collapse
3. target group에 requested index로 tab 삽입
4. target group active tab = moved tab
5. focused group = target group
6. global flattened order 재계산
7. tab 자체의 `layoutsByTabId[tabId]`와 sessions는 그대로 보존

### P1-1. Custom tab pointer drag는 cross-group target을 잘못 reorder target으로 해석할 수 있다

`TabBar.resolveTabDropTargetAtPoint()`는 `document.querySelectorAll("[data-tab-dnd-id]")`로 **모든 group의 tab element**를 검색한다.

하지만 반환한 `data-tab-index`를 source `TabBar`의 local `tabs.length`로만 검증한다. destination group의 index가 source group 길이 범위 안이면 다른 group tab 위에 있어도 source group reorder target으로 인정될 수 있다.

그 결과 cross-group center drag가 아직 merge되지 않는 것뿐 아니라, source group 내부 순서가 destination group의 local index를 기준으로 뜻밖에 바뀔 수 있다.

원본처럼 하나의 root DnD context가 `{tabId, sourceGroupId}`와 droppable `{type, groupId, index}`를 함께 소유해야 한다.

### P1-2. Reorder와 split이 별도 side-effect로 실행된다

현재 pointer up에서 `TabBar`가 먼저 `commitTabReorder()`를 실행하고, 이어 parent `onTabPointerDragEnd()`가 edge split을 실행할 수 있다.

원본은 drag end target을 먼저 한 번 결정하고 **reorder / merge / split 중 정확히 하나**를 수행한다.

따라서 Orca-lite도 drop resolution과 state mutation을 root orchestration 한 곳으로 올려야 한다.

### P1-3. group-local close UI와 global close action이 불일치한다

`TabContextMenuPopup`은 현재 group의 `tabs` 배열로 “Close Others/Left/Right” enabled 상태를 계산한다.

하지만 callback은 `workspaceStore.closeOtherTabs/closeTabsToRight/closeTabsToLeft`로 연결되고, 이 함수들은 `state.layout.tabs` 전체 flattened array를 대상으로 한다.

split group이 둘 이상일 때 “Close Others”가 다른 group tab까지 닫을 수 있다.

#### 수정

API를 group-aware하게 바꾼다.

```ts
closeOtherTabs(groupId, tabId)
closeTabsToRight(groupId, tabId)
closeTabsToLeft(groupId, tabId)
```

또는 store가 `getGroupForTab(tabId)`를 내부에서 구해 그 group의 `tabIds`만 사용한다.

### P1-4. Browser tab은 unified tab group operation에서 배제된다

whole-tab split/merge는 terminal content와 독립적인 workspace layout operation이어야 한다.

하지만 `workspaceStore.splitPane()`은 target 또는 source가 browser이면 즉시 return한다. 이는 “terminal pane split”과 “tab group split”을 하나의 API에 섞었기 때문에 생긴 문제다.

#### 수정

API를 분리한다.

```ts
splitTerminalPane(tabId, leafId, direction)
moveTabToSplit(tabId, targetGroupId, direction)
moveTabToGroup(tabId, targetGroupId, index?)
```

`moveTab*`는 `WorkspaceTab` 종류와 무관해야 한다.

### P1-5. Terminal pane → tab detach가 없다

현재 pane DnD는 same tab 내 `SWAP_PANES`만 지원한다.

원본 parity에는 다음 primitive가 필요하다.

```ts
DETACH_PANE_TO_TAB {
  sourceTabId: string;
  sourceLeafId: string;
  targetGroupId: string;
  targetIndex?: number;
  newTabId: string;
}
```

중요한 점은 **backend session을 닫거나 새로 spawn하지 않는 것**이다.

- source layout에서 leaf와 session mapping 제거
- source split collapse
- new terminal tab + single-leaf layout 생성
- 기존 local TerminalSession을 new tab layout에 그대로 연결
- `tab.sessionId` compatibility field가 필요하다면 moved session id로 설정
- `TerminalHostManager` instance도 같은 session id를 계속 사용하므로 React mount 대상만 새 tab으로 이동

### P1-6. Drag focus/activation transaction이 원본보다 약하다

원본은 drag 시작 전 group별 active tab snapshot을 만들고, cross-group hover preview와 drop/cancel 후 복원까지 처리한다.

현재 Lite는 `tabPointerDragId`/`tabPointerDropTarget`만 추적한다. 따라서 다음 UX가 빠져 있다.

- destination group tab/body hover 시 target preview activation
- source group active tab이 이동될 때 자연스러운 replacement activation
- cancelled drag 뒤 원래 activation 복원
- destination group focus 확정

이 로직은 reducer normalization에 암묵적으로 맡기지 말고 DnD controller의 명시적 transaction으로 다뤄야 한다.

### P1-7. Persistence schema가 layout tree는 저장하지만 session continuity를 정확히 보존하지 않는다

현재 좋은 부분:

- `tabGroups`
- `tabGroupLayout`
- `focusedGroupId`
- per-tab `paneTree`
- `sessionIdsByLeafId`
- `activeLeafId`

은 저장된다.

하지만 다음 문제가 있다.

1. `PersistedWorkspaceSession.version`은 항상 1이고 schema migration switch가 없다.
2. `PersistedTerminalSession.sessionId`에 `sess.id`(local id)를 저장한다. live session list는 backend id이므로 reattach 판정과 식별자가 다르다.
3. browser tab kind/url/browser state를 정식 field로 저장하지 않는다.
4. deserialize의 `paneTree.type === "browser"` 판정은 `PaneNode` union과 맞지 않는다.
5. `pinned`, `expandedLeafId`가 보존되지 않는다.
6. current terminal CWD/scrollback도 실제로 serialize하지 않는다.

따라서 “layout 모양이 돌아오는 것”과 “실행 중 terminal/browser 상태가 동일하게 살아나는 것”은 구분해서 봐야 한다.

---

## 7. 목표 상태 모델

기존 두-tree 모델은 버리지 말고 invariant를 강화하는 방향이 가장 비용이 낮다.

### 7.1 Workspace tabs/groups

```ts
type LayoutState = {
  tabs: WorkspaceTab[]; // compatibility + flattened rendering/indexing view
  activeTabId: string | null;

  tabGroups: Record<string, TabGroup>;
  tabGroupLayout: TabGroupLayoutNode | null;
  focusedGroupId: string | null;

  layoutsByTabId: Record<string, TabPaneLayout>;
};
```

필수 invariant:

1. 모든 tab id는 정확히 한 `TabGroup.tabIds`에 존재한다.
2. 모든 non-empty group은 `tabGroupLayout` leaf에 정확히 한 번 존재한다.
3. 빈 group은 즉시 제거하고 parent split을 collapse한다.
4. `group.activeTabId`는 반드시 해당 `group.tabIds` 안에 있다.
5. `focusedGroupId`는 존재하는 group이다.
6. `LayoutState.activeTabId === focusedGroup.activeTabId`.
7. `tabs` global array는 authoritative ordering이 아니라 group tree를 flatten한 projection으로 취급한다.

가능하면 장기적으로 `tabs`도 `tabsById + group.tabIds`로 정규화하고 global array 의존을 줄이는 것이 안전하다.

### 7.2 Terminal pane ownership

```ts
type TabPaneLayout = {
  root: PaneNode;
  activeLeafId: string | null;
  expandedLeafId: string | null;
  sessionIdsByLeafId: Record<string, string>;
};
```

필수 invariant:

1. terminal leaf마다 local session id가 하나 존재한다.
2. 일반 split에서는 서로 다른 leaf가 같은 local session/xterm instance를 공유하지 않는다.
3. local session은 backend PTY id를 최대 하나 가진다.
4. pane-to-tab detach는 session을 복제하지 않고 ownership 위치만 이동한다.
5. session close는 전체 layout에서 reference가 0이 된 뒤에만 수행한다.
6. terminal tab의 legacy `tab.sessionId`는 canonical ownership source로 쓰지 말고 `activeLeafId` 또는 first leaf의 session으로부터 파생하거나 migration compatibility용으로 제한한다.

### 7.3 명시적 layout commands

현재 overloaded `splitPane()` 대신 최소 다음 command layer를 권장한다.

```ts
reorderTabWithinGroup(tabId, targetIndex)
moveTabToGroup(tabId, targetGroupId, targetIndex?)
moveTabToSplit(tabId, targetGroupId, direction, position?)

splitTerminalPane(tabId, sourceLeafId, direction)
closeTerminalPane(tabId, leafId)
moveTerminalPaneWithinTab(tabId, sourceLeafId, targetLeafId)
detachTerminalPaneToTab(sourceTabId, sourceLeafId, targetGroupId, targetIndex?)
```

이렇게 해야 tab group operation과 terminal pane operation의 backend side-effect가 섞이지 않는다.

---

## 8. Drag & Drop 전략: dnd-kit 채택 권장

### 8.1 왜 현재 custom pointer를 계속 확장하지 않는가

현재 구현도 12px threshold, pointer capture, cancel cleanup 등을 직접 잘 처리하고 있다. 하지만 parity에 필요한 범위는 이제 단순 pointer gesture를 넘어선다.

필요한 것:

- 여러 tab strip 사이 sortable
- group body droppable
- 4 edge split droppable/geometry
- insertion index
- drag overlay
- one-drop-one-transaction 보장
- hover activation
- cancel rollback
- keyboard/accessibility sensor 선택 가능
- nested split DOM에서 collision resolution
- Tauri native titlebar drag와 충돌 방지

원본 Orca 자체가 dnd-kit 계열 primitive로 이 문제를 풀고 있으므로, parity/유지보수 관점에서 같은 계열을 쓰는 것이 가장 낮은 위험이다.

### 8.2 권장 dependency

`ui/package.json`

```json
{
  "@dnd-kit/core": "...",
  "@dnd-kit/sortable": "...",
  "@dnd-kit/utilities": "..."
}
```

정확한 version은 구현 시 현재 React/Vite 의존성과 lockfile 기준으로 선택한다.

### 8.3 권장 component 구조

새 파일 예시:

```text
ui/src/components/tab-dnd/
  TabDndProvider.tsx
  SortableTab.tsx
  TabGroupDropSurface.tsx
  tabDropResolver.ts
  tabDragTypes.ts
```

`TabDndProvider`를 `TerminalSplitView`의 전체 `tabGroupLayout` renderer 바깥에 둔다.

Drag payload:

```ts
{ type: "tab", tabId, sourceGroupId }
```

Droppable payload:

```ts
{ type: "tab", groupId, tabId, index }
{ type: "group-body", groupId }
{ type: "group-edge", groupId, edge: "left" | "right" | "top" | "bottom" }
```

Drop resolution priority:

1. explicit group edge
2. tab insertion
3. group body center
4. no-op/cancel

그 결과를 store command 하나로만 전달한다.

### 8.4 Tauri titlebar drag 처리

현재 `TabBar.startWindowDrag()`가 tab/button/input 밖의 empty strip에서만 `getCurrentWindow().startDragging()`을 호출하는 방식은 유지할 수 있다.

중요:

- actual sortable tab에 Tauri drag-region을 지정하지 않는다.
- DnD activation 전 empty strip만 native window drag를 시작한다.
- tab DnD와 OS window move gesture를 같은 DOM region에서 동시에 발동시키지 않는다.

---

## 9. 단계별 구현 계획

## Phase 0 — invariant와 regression test를 먼저 고정

### 변경 파일

- `ui/src/state/layout.ts`
- `ui/src/state/layout.test.ts`
- `ui/src/state/workspaceStore.ts`
- `ui/src/state/workspaceStore*.test.tsx`

### 작업

1. `MOVE_TAB_TO_GROUP` action 추가.
2. `MOVE_TAB_TO_SPLIT`의 target을 `targetTabId/targetLeafId`가 아니라 `targetGroupId` 중심으로 정리.
3. source empty group collapse 테스트.
4. target insertion index 테스트.
5. moved tab의 nested terminal pane tree/session mapping이 identity 그대로 보존되는지 테스트.
6. browser tab도 group move/split reducer에서 허용.
7. close others/left/right를 group-local로 수정하고 2-group regression test 추가.

### 완료 조건

- reducer property 수준에서 “tab은 정확히 한 group에 존재” invariant 위반 0.
- cross-group merge 후 source empty/non-empty 두 경우 모두 tree 정상 collapse.
- 기존 tab pane tree/session mapping object semantic 보존.

---

## Phase 1 — tab DnD를 단일 dnd-kit transaction으로 교체

### 변경 파일

- `ui/package.json`
- `ui/bun.lock`
- `ui/src/components/TabBar.tsx`
- `ui/src/components/TerminalSplitView.tsx`
- 신규 `ui/src/components/tab-dnd/*`
- `TabBar.test.tsx`
- `TerminalSplitView.pointerDrag.test.tsx` 또는 신규 dnd test

### 작업

1. `DndContext`를 모든 tab groups의 공통 parent에 둔다.
2. 각 group tab list를 `SortableContext`로 구성.
3. activation distance 12px 유지.
4. same-group reorder 구현.
5. destination tab/index → `moveTabToGroup`.
6. destination body center → append/move into group.
7. 4 edge → `moveTabToSplit`.
8. `DragOverlay`와 edge/insertion preview 구현.
9. drag start 시 group active snapshot 저장.
10. hover preview/drag cancel rollback/drop focus 확정.
11. 기존 `document.querySelectorAll` 기반 source-local reorder resolver 제거.
12. legacy `TAB_DRAG_MIME`/pointer-only dead path 정리.

### 완료 조건

E2E-like component test에서 한 gesture마다 정확히 하나의 command만 발생:

- reorder
- cross-group indexed merge
- cross-group body merge
- left/right/top/bottom split
- cancel no-op

---

## Phase 2 — true terminal pane split 구현

이 phase는 backend lease 결정을 포함하는 **P0 architecture phase**이다.

### 백엔드 변경 파일 후보

- `src-tauri/src/worktree/manager.rs`
- `src-tauri/src/worktree/model.rs`
- `src-tauri/src/terminal/pty.rs`
- `src-tauri/src/terminal/session.rs`
- `src-tauri/src/ipc/terminal.rs`
- 관련 Rust tests

### UI 변경 파일

- `ui/src/lib/tauri.ts`
- `ui/src/lib/types.ts`
- `ui/src/state/workspaceStore.ts`
- `ui/src/components/TerminalSplitView.tsx`
- `ui/src/lib/terminalHostManager.ts`는 unique session invariant 기준으로 검증

### 작업

1. **writer lease 정책 결정**
   - 권장: interactive terminal multi-session 허용.
2. source pane의 current CWD 조회 API 추가.
   - 예: `cmd_terminal_get_cwd(session_id)` 또는 spawn request의 validated `cwd` override.
3. `splitTerminalPane()`에서 새 backend PTY spawn.
4. 새 local `TerminalSession.id` 생성.
5. 새 leaf id ↔ 새 session id mapping.
6. independent `TerminalHostManager` xterm instance 생성.
7. split 실패 시 reducer state를 mutate하지 않는 two-phase command 처리.
8. pane close 시 해당 PTY만 close.
9. last pane close는 tab close semantics로 연결.

### 완료 조건

실 Tauri 환경에서:

- 하나의 tab을 right split → 두 pane에 동시에 서로 다른 prompt가 보임.
- 한 pane에서 `cd subdir`, 그 pane을 split → 새 pane이 `subdir`에서 시작.
- 각 pane에 서로 다른 command 실행 가능.
- 한 pane close가 sibling PTY를 죽이지 않음.
- 동일 worktree split이 writer lease 오류 없이 작동.

---

## Phase 3 — terminal pane → 새 tab detach

### 변경 파일

- `ui/src/state/paneTree.ts`
- `ui/src/state/layout.ts`
- `ui/src/state/workspaceStore.ts`
- `ui/src/components/TerminalSplitView.tsx`
- tab/pane DnD controller
- 관련 reducer/store/component tests

### 작업

1. `detachLeaf()` helper 추가: source tree, detached subtree/leaf, session id, next active leaf 반환.
2. source tab이 pane 2개 이상일 때만 detach 허용.
3. tab strip을 pane external drop target으로 노출.
4. drop 시 backend spawn/close 없이 existing session ownership 이동.
5. target group insertion index 지원.
6. source active leaf가 빠졌다면 sibling focus.
7. new tab을 active/focused로 설정.
8. source tab title/activity/session ownership 정리.

### 완료 조건

- split pane에서 한 pane을 tab strip으로 drag → 새 tab 생성.
- 기존 shell process/scrollback가 끊기지 않음.
- source tab에는 나머지 pane만 남고 tree collapse 정상.
- 새 tab close 시 moved PTY가 정확히 한 번 종료.

---

## Phase 4 — context menu, focus, browser unified tab parity

### 변경 파일

- `ui/src/components/TabBar.tsx`
- `ui/src/components/TerminalSplitView.tsx`
- `ui/src/state/workspaceStore.ts`
- `ui/src/App.tsx`
- shortcut 관련 파일/tests

### 작업

1. 원본과 같이 context menu에 **Move Tab to Split** 4방향 추가.
2. **Split terminal** right/down과 시각적으로 별도 section 유지.
3. browser tab도 `moveTabToSplit/moveTabToGroup` 허용.
4. bulk close를 group-local로 제한.
5. tab next/previous/select-by-index의 group semantics를 명시하고 focused group 기준으로 정리.
6. drag hover/cancel focus test 추가.
7. terminal focus next/previous는 unique pane session 구현 후 실제 xterm focus까지 검증.
8. optional: equalize/expand pane shortcut을 원본 keybinding surface와 맞춤.

### 완료 조건

- terminal/browser tab 모두 split group 간 이동/merge 가능.
- context menu와 drag가 동일 reducer command를 사용.
- focused group의 tab 단축키가 다른 group state를 잘못 변경하지 않음.

---

## Phase 5 — persistence schema v2 + migration

### 변경 파일

- `ui/src/lib/types.ts`
- `ui/src/lib/sessionPersistence.ts`
- `ui/src/lib/sessionPersistence.test.ts`
- `ui/src/App.tsx`

### `PersistedTab` 권장 필드

```ts
type PersistedTab = {
  id: string;
  kind: "terminal" | "browser";
  label: string;
  pinned?: boolean;

  terminal?: {
    primarySessionId?: string;
    paneTree: PaneNode;
    sessionIdsByLeafId: Record<string, string>;
    activeLeafId: string | null;
    expandedLeafId: string | null;
  };

  browser?: {
    browserId: string;
    url: string;
    title?: string;
  };
};
```

`PersistedTerminalSession`은 local id와 backend id를 분리한다.

```ts
{
  localSessionId: string;
  backendSessionId: string | null;
  worktreePath: string;
  cwd: string;
}
```

### 작업

1. `version: 2` schema 도입.
2. v1 → v2 migration 함수 작성.
3. browser kind/state 명시 저장.
4. pinned/expanded leaf 보존.
5. live backend list와 `backendSessionId`로 정확히 match.
6. dead PTY만 respawn.
7. split pane unique sessions를 restore 순서대로 respawn/reattach.
8. backend multi-session policy에 맞게 restore concurrency 처리.
9. corrupted group/pane tree는 `normalizeLayout()`으로 safe repair.

### 완료 조건

round-trip test에서 다음이 모두 동일:

- 2+ tab groups와 nested group split ratios
- 각 group active tab + focused group
- terminal tab nested pane tree + active/expanded pane
- unique pane sessions
- browser tabs
- pinned tabs

---

## Phase 6 — 실기 검증 및 release gate

### 자동 테스트

- layout reducer tests
- workspace store session ownership tests
- dnd target resolution tests
- persistence v1/v2 migration tests
- backend multi-PTY worktree tests
- pane detach ownership tests

### macOS Tauri smoke

필수 시나리오:

1. tab A/B/C 생성
2. B를 A/C 사이 reorder
3. B를 right edge로 drag → 새 group
4. B를 원 group body로 drag → merge
5. 같은 동작 left/up/down 반복
6. context menu Move Tab to Split 4방향
7. terminal split right/down
8. 각 pane에서 독립 command
9. pane focus next/previous
10. pane divider/group divider resize
11. pane를 tab strip으로 drag → new tab detach
12. browser tab을 group split/merge
13. app close/reopen 후 layout/session 복원
14. drag 도중 Escape/pointercancel/window blur
15. split/merge/close 반복 후 orphan PTY 0

### release invariant

```text
UI leaf count == mounted terminal renderer count == owned live PTY count
```

terminal pane에 대해서 이 invariant가 깨지는 순간 parity regression으로 간주한다.

---

## 10. 파일별 변경 요약

| 파일 | 변경 방향 |
|---|---|
| `ui/src/state/layout.ts` | `MOVE_TAB_TO_GROUP`, group-targeted split, detach-to-tab reducer primitives, strict invariants |
| `ui/src/state/paneTree.ts` | detach leaf/subtree helper, ownership-preserving tree operation |
| `ui/src/state/workspaceStore.ts` | group commands와 terminal pane commands 분리, group-local bulk close, unique split session lifecycle |
| `ui/src/components/TabBar.tsx` | custom global pointer resolver 제거, sortable tab presentation, Move Tab to Split context menu |
| `ui/src/components/TerminalSplitView.tsx` | root DnD provider/drop surfaces, group body/edge/insertion targets, pane external drop target |
| `ui/src/components/tab-dnd/*` | 새 drag orchestration/typed target/overlay |
| `ui/src/components/TerminalPane.tsx` | unique session mount invariant 검증; ownership 이동 시 안정적인 reparent만 허용 |
| `ui/src/lib/terminalHostManager.ts` | session당 renderer 1개 invariant를 명시하고 duplicate leaf binding 방어 진단 추가 |
| `ui/src/lib/types.ts` | persisted schema v2, tab kind/session ids 명확화 |
| `ui/src/lib/sessionPersistence.ts` | v1→v2 migration, backend id continuity, browser/pin/expanded pane 복원 |
| `ui/src/App.tsx` | focused-group tab shortcuts, v2 restore orchestration |
| `ui/package.json`/`bun.lock` | dnd-kit 의존성 |
| `src-tauri/src/ipc/terminal.rs` | current cwd 조회/validated spawn cwd, multi-pane spawn contract |
| `src-tauri/src/terminal/pty.rs` | interactive multi-PTY 정책 반영 |
| `src-tauri/src/worktree/manager.rs` | writer lease scope 재설계 또는 terminal lease 분리 |

---

## 11. 구현 순서와 우선순위

권장 순서는 다음과 같다.

```text
P0-A state reducer merge primitives
  ↓
P0-B backend multi-PTY / current-CWD contract
  ↓
P0-C true terminal split
  ↓
P1-A root dnd-kit tab reorder/split/merge
  ↓
P1-B pane → tab detach/PTy transfer
  ↓
P1-C group-aware close/focus/browser unified moves
  ↓
P1-D persistence v2
  ↓
P2 macOS Tauri parity E2E
```

DnD UI부터 먼저 크게 교체하는 것보다 **state command와 terminal ownership invariant를 먼저 고정**해야 한다. 그래야 drag layer는 UI target을 reducer command로 변환하는 얇은 계층으로 유지할 수 있다.

---

## 12. Native OS window tear-off에 대한 별도 판단

현재 Orca-lite production UI에서 `WebviewWindow`를 생성해 tab을 별도 Tauri window로 옮기는 구현은 확인되지 않았다.

동시에 bundled original Orca의 tab context/drag 코드를 조사했지만 canonical action은 “Move Tab to Split”, cross-group merge, terminal pane-to-tab detach였고 **tab을 top-level OS window로 tear off하는 명확한 근거는 찾지 못했다.**

따라서 native window detach/reattach를 이번 parity의 필수 blocker로 넣으면 안 된다.

제품 요구로 추가한다면 별도 Phase W가 필요하다.

### Phase W 개략 설계

- window-scoped renderer store가 아니라 process/backend-scoped workspace model 필요
- `windowId -> visible group/layout subset` 또는 shared central state sync
- PTY/session ownership을 renderer lifecycle과 분리
- Tauri multi-window event/state synchronization
- window close 시 tabs를 다른 window로 re-home할지 PTY를 close할지 정책
- drag across native windows 좌표/IPC channel
- persistence에 window layout 추가

이 작업은 현재 in-window Orca parity보다 훨씬 큰 architecture 변경이므로 별도 RFC로 분리하는 것이 안전하다.

---

## 13. 최종 Gap List

### P0 — 반드시 먼저 해결

1. terminal split shared-session/shared-xterm runtime bug
2. backend 1-writer-1-worktree와 independent split PTY의 충돌
3. current pane CWD를 inherited split에 전달할 IPC 부재
4. cross-group tab merge state primitive 부재

### P1 — Orca core parity

5. root drag transaction 부재; source-local custom pointer DnD의 cross-group 오판 가능성
6. existing group tab/index/body drop 부재
7. Move Tab to Split context menu 4방향 부재
8. terminal pane → new tab detach/PTy transfer 부재
9. browser unified tab split/merge 차단
10. group-local close others/left/right action 오류
11. drag hover activation/cancel restore/focus transaction 부재
12. persistence의 backend/local session id 혼동
13. browser/pinned/expanded pane persistence 누락

### P2 — polish/robustness

14. focused-group 기준 tab cycle/index shortcut 의미론 정리
15. pane drag를 HTML5 DnD에서 typed pointer/DnD lifecycle로 통합할지 결정
16. equalize/expand pane 기능과 shortcut parity 보강
17. drag/resize 반복 시 listener/PTY/renderer leak E2E

### 별도 요구

18. native OS window tab tear-off/reattach — canonical parity 근거 미확인, 별도 RFC 권장

---

## 14. 최종 권고

현재 구현을 전부 폐기할 필요는 없다. **`tabGroups + tabGroupLayout`과 `layoutsByTabId + PaneNode`의 두-tree 분리 자체는 올바른 기반**이다.

가장 중요한 수정은 다음 세 가지 설계 원칙을 코드에 강제하는 것이다.

1. **Tab group mutation과 terminal pane mutation을 서로 다른 command로 분리한다.**
2. **Terminal leaf 하나는 독립 renderer/session/PTY 하나를 소유한다.**
3. **모든 drag는 root controller에서 target을 한 번 결정하고 정확히 하나의 atomic layout command를 실행한다.**

이 세 가지가 고정되면 reorder → split → merge → pane detach → persistence가 같은 상태 모델 위에서 자연스럽게 이어지고, 현재 발생하는 대부분의 edge-case가 제거된다.
