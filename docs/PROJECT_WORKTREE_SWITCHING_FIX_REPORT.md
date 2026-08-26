# 프로젝트/워크트리 전환 결함 분석 및 수정 보고서

작성일: 2026-08-26

## 결론

프로젝트 또는 워크트리를 바꾸는 동안 나타나던 증상은 세 개의 수명주기 결함에서 비롯됐다.

1. 전환 순간 한 렌더 동안 새 프로젝트 ID와 이전 프로젝트의 워크트리/레이아웃/세션 상태가 함께 노출됐다.
2. React가 기존 네이티브 터미널을 먼저 unmount한 뒤 새 터미널을 attach/render해, 두 surface 사이에 빈 compositor 프레임이 생겼다.
3. 저장된 다른 프로젝트의 workspace 상태를 전환 후 effect에서 비동기로 읽어, 그 전까지 빈 `layout.tabs`가 먼저 커밋됐다.

수정 후에는 앱 부팅 단계에서 모든 등록 프로젝트의 저장된 workspace snapshot을 먼저 메모리에 적재한다. 프로젝트 전환은 첫 렌더부터 기존 탭을 포함한 상태를 동기 교체하며, 프로젝트 ID와 상태 소유자가 일치한다. 새 네이티브 터미널은 최종 bounds로 실제 렌더된 뒤에만 이전 surface를 detach한다.

## 증상별 원인

### 전환 플리커

- `useWorkspaceStore`는 프로젝트 변경을 감지하면 내부 ref를 즉시 바꾸고 reducer update를 예약했다.
- 그러나 해당 렌더의 지역 `state` 값은 이전 프로젝트를 계속 가리켰다.
- 결과적으로 앱은 새 프로젝트 chrome과 이전 프로젝트 터미널 상태를 한 프레임 조합한 뒤 다시 렌더했다.

수정 위치: `ui/src/state/workspaceStore.ts:148`

### “워크트리 못 찾음”

- 위의 혼합 렌더에서는 새 workspace ID와 이전 workspace의 worktree identity가 동시에 하위 effect/callback에 노출될 수 있었다.
- 이 잘못된 ID 조합이 터미널 생성 또는 워크트리 조회 IPC까지 도달하면 backend는 해당 workspace에서 identity를 찾을 수 없다.
- store가 동기 전환한 상태를 즉시 반환하도록 바꿔 잘못된 조합 자체를 만들지 않는다.

회귀 테스트: `ui/src/state/workspaceStore.test.tsx`

### 전환 후 기본 화면, New Terminal 뒤 기존 탭 출현

- 이전 구현은 현재 프로젝트만 복원하고, 다른 프로젝트의 저장된 탭은 해당 프로젝트를 선택한 뒤 `useEffect`에서 읽었다.
- effect가 끝나기 전 store에는 빈 레이아웃이 있어 기본 화면이 먼저 표시됐다.
- 이 사이 New Terminal을 누르면 새 상태 변경이 발생하고, 뒤늦게 복원된 기존 탭이 함께 나타났다.
- 앱 부팅을 완료하기 전에 모든 등록 프로젝트의 persisted session과 live backend mapping을 한 번 읽어 snapshot cache를 채우도록 변경했다.
- disk deserialize 결과에도 `workspaceId`를 명시해 복원 상태의 소유권을 항상 유지한다.
- preload된 상태는 후속 restore coordinator가 다시 디스크를 읽지 않고 사용하며, 기존 backend recovery 절차는 그대로 실행한다.

수정 위치:

- `ui/src/App.tsx`
- `ui/src/lib/sessionPersistence.ts`
- `ui/src/state/workspaceRestore.ts`

### 터미널 미표시

- 기존 터미널 effect cleanup의 detach가 microtask에서 실행되고, replacement의 attach 및 bounds render는 그 뒤에 완료됐다.
- 따라서 두 IPC 사이에 네이티브 child surface가 없는 구간이 존재했다.
- detach를 임의 시간만큼 늦추지 않고, replacement의 성공적인 bounds render라는 정확한 신호까지 보류하도록 변경했다.

수정 위치:

- `ui/src/lib/nativeTerminalLifecycle.ts:89`
- `ui/src/lib/nativeTerminalLifecycle.ts:125`
- `ui/src/components/NativeTerminalPane.tsx:120`

## 회귀 방지

실패 우선으로 다음 세 결함을 재현했다.

1. 프로젝트 전환 렌더 기록에 `{ requestedWorkspaceId: "beta", stateWorkspaceId: "alpha" }`가 포함되는 테스트
2. replacement의 `cmd_native_terminal_set_bounds`보다 outgoing `cmd_native_terminal_detach`가 먼저 호출되는 테스트
3. persisted session preload가 끝나기 전에 native app shell이 빈 workspace를 렌더하는 테스트

수정 후 빠른 A -> B -> C 전환 순서와, 프로젝트 전환의 첫 렌더부터 기존 탭이 존재하며 새 terminal을 spawn하지 않는 조건을 추가 검증했다.

## 자동 검증

- 변경 파일 TypeScript LSP 오류: 없음
- 전환 핵심 테스트: 59/59 통과
- 네이티브 터미널 관련 테스트: 35/35 통과
- 프로젝트/복원 관련 테스트: 102/102 통과
- 전체 프론트엔드 테스트: 80 files, 656 tests 통과
- 마지막 추가한 first-render 전환 테스트: 1/1 통과
- `git diff --check`: 통과
- 프로덕션 빌드: `tsc && vite build` 성공

## 데스크톱 수동 확인

첫 번째 수정은 실제 Ferryx 확인에서 FAIL이었다. 당시 관찰된 정확한 증상은 전환 후 기본 화면이 보이고, New Terminal을 누르면 기존 탭이 뒤늦게 나타나는 것이었다. 이 결과로 비동기 workspace hydration 레이스를 추가 수정했다.

데스크톱 UI 자동 조작은 수행하지 않았다. preload는 앱 부팅 시 실행되므로 HMR 상태를 그대로 둔 채 확인하지 말고 Ferryx dev 앱을 완전히 재시작한 뒤 다음 항목을 확인해야 한다.

1. 프로젝트 A와 B에 각각 터미널을 열고 구분 가능한 출력/CWD를 만든다.
2. 사이드바로 A -> B -> A를 10회 반복한다.
3. 같은 프로젝트에서 worktree A -> B -> C를 빠르게 반복한다.
4. 각 전환에서 빈 프레임, 이전 프로젝트 터미널의 순간 노출, workspace/worktree 오류 toast가 없는지 확인한다.
5. 각 프로젝트에 열린 터미널을 둔 채 앱을 재시작하고, 다시 프로젝트를 오가며 올바른 터미널이 즉시 표시되는지 확인한다.

재시작 후 수동 확인 결과를 받으면 이 섹션에 최종 PASS/FAIL을 기록한다.

## 전환 디버그 로그

데스크톱 실패가 계속되어 development build에 구조화 전환 추적을 추가했다. 각 레코드는 동일 프론트엔드 실행을 묶는 `runId`, 단조 증가하는 `sequence`, `wallTimeMs`, event 이름, 세부 상태를 포함한다.

기록 범위:

- 프로젝트 선택, 등록, canonical ID 채택, runtime gate
- inactive project 등록과 worktree list 결과
- workspace snapshot source(HMR/snapshot/empty), swap 전후 탭·세션 수
- session preload와 restore source, active/parked tab 수
- worktree refresh/list/sync/ensure 분기
- New Terminal 시작·완료·전환 중 폐기
- native terminal attach, bounds, present, replacement hold, detach

development WebView console에는 `[ferryx:switch]` prefix로 출력되며, 같은 데이터가 다음 파일에 JSONL로 저장된다.

```text
/tmp/ferryx-switch-debug.jsonl
```

재현 절차:

1. Ferryx dev 앱을 완전히 재시작한다.
2. 기존 터미널 탭이 있는 프로젝트 A에서 프로젝트 B 또는 다른 worktree를 선택한다.
3. 빈 기본 화면이 나오면 New Terminal을 누르기 전에 잠시 멈춘다.
4. 동일 전환을 한 번만 더 수행한다.
5. `/tmp/ferryx-switch-debug.jsonl`을 전달한다. `runId`별 `sequence`가 실제 처리 순서를 보존한다.

로그에는 터미널 출력, 실행 명령, 환경 변수, 인증 정보가 포함되지 않는다.

## 2026-08-26 재현 로그 확정 원인

사용자 재현 run `804b0076-de10-4e19-959d-8d37706d66b4`에서 이전 분석보다 더 직접적인 React StrictMode 전환 레이스가 확인됐다.

- sequence 129: `orca-lite` snapshot은 탭 4개와 세션 4개를 정상 보유했다.
- sequence 130: `workspace.store.swap`도 이 정상 snapshot을 incoming state로 선택했다.
- sequence 134: 바로 다음 commit은 `activeProjectId=orca-lite`인데도 이전 `superwiki-mail-otp`의 탭 3개를 렌더했다.
- sequence 156: 잘못 결합된 worktree sync가 이전 프로젝트 세션을 모두 제거해 탭/세션이 0개가 됐다.
- sequence 160 이후: 빈 상태를 채우기 위한 terminal spawn이 시작됐다.
- sequence 273 이후: 오염된 0-tab HMR 상태가 다음 프로젝트 전환에서 다시 로드됐고, sequence 298-364에서 여러 terminal이 연쇄 생성됐다.

원인은 `useWorkspaceStore`가 render 중 `mountedWorkspaceIdRef`를 새 ID로 먼저 바꾸고 reducer restore를 예약한 데 있었다. StrictMode가 commit 전에 render를 다시 실행하면 reducer state는 여전히 이전 프로젝트인데 ref만 새 프로젝트를 가리킨다. 두 번째 render는 ref가 이미 일치한다고 판단해 swap을 건너뛰고 이전 state를 새 프로젝트에 commit했다. 이후 effect가 이 잘못된 state를 새 프로젝트 HMR key에 저장해 다음 전환까지 오염시켰다.

수정은 mutable ref가 아니라 reducer state의 소유자(`state.workspaceId`)와 요청된 `workspaceId`를 비교해 swap 여부를 결정한다. React가 render를 재시도해 reducer state가 아직 이전 프로젝트라면 swap도 반드시 다시 수행된다. 또한 모든 HMR/snapshot write는 resulting state가 가진 실제 owner ID를 사용하고, 구버전 restore state에 `workspaceId`가 없으면 현재 reducer state의 owner를 보존한다.

회귀 테스트 `retains target workspace tabs and avoids spawning terminals across alpha -> beta -> alpha switch under render retries`는 `StrictMode`에서 alpha -> beta -> alpha를 수행한다. 수정 전에는 beta 전환 직후 expected `"beta"`, received `"alpha"`로 실패했으며, 수정 후 target 탭을 유지하고 `spawnTerminal` 호출이 0회임을 검증한다.

## 2026-08-26 전환 연속성 후속 수정

상태 소유권 레이스를 제거한 뒤에도 두 가지 짧은 시각적 단절이 남았다.

### Sidebar worktree 목록 소실

active 프로젝트가 바뀌는 첫 렌더에서 `Sidebar`는 해당 프로젝트가 inactive일 때 받아 둔 worktree cache를 즉시 버렸다. 동시에 새 active store의 authoritative worktree list가 아직 비어 있으면 `WorktreeList`가 `null`을 반환했다. 이전 active 프로젝트 역시 inactive cache에 들어간 적이 없으므로, 비동기 `listWorktrees`가 끝날 때까지 그 목록도 잠시 사라졌다.

수정 내용:

- `useInactiveProjectWorktrees`가 현재 active 프로젝트의 authoritative worktree rows도 cache에 기록한다.
- active ID 전환 후 비동기 inactive 결과를 기존 cache에 merge해 이전 프로젝트 rows를 유지한다.
- `Sidebar`는 authoritative active rows가 존재하면 항상 그것을 사용하고, 전환 중 비어 있을 때만 cached rows를 사용한다.
- `App.tsx`의 keyboard-visible worktree 계산도 동일한 fallback 규칙을 사용한다.

실패 우선 테스트:

- active store rows가 잠시 비어 있어도 newly-active 프로젝트의 cached list가 계속 렌더되는지 검증
- alpha -> beta 직후 alpha의 inactive listing이 아직 끝나지 않아도 alpha rows가 cache에 남는지 검증

수정 전 두 테스트는 각각 `Unable to find ... "alpha worktrees"`와 `expected undefined`로 실패했다. 수정 후 Sidebar/cache 관련 33/33 테스트가 통과했다.

### 새 worktree 첫 terminal 전환 플리커

탭이 아직 없는 worktree를 처음 선택할 때 `ensureTabForWorktree`가 terminal spawn을 시작하기 전에 `SELECT_WORKTREE`를 dispatch했다. 이 action은 target의 빈 layout을 active로 만들었고, `App`은 `TerminalSplitView`를 unmount한 뒤 `EmptyWorkspaceView`를 렌더했다. PTY spawn이 완료된 뒤 terminal tree가 다시 mount되면서 빈 terminal frame이 노출됐다.

수정은 선행 `SELECT_WORKTREE`를 제거했다. 기존 terminal/layout은 새 PTY가 준비되는 동안 계속 표시된다. `openTab`이 target worktree의 parked layout에 새 tab/session을 먼저 추가한 다음 `SELECT_WORKTREE`를 dispatch하므로, target 전환 시점에는 이미 non-empty layout이 존재한다.

실패 우선 테스트는 target spawn promise를 의도적으로 unresolved 상태로 유지하고, 그동안 source worktree의 active path와 tab/layout이 계속 남는지 확인한다. 수정 전에는 expected `"/repo/main"`, received `"/repo/feature"`로 실패했고, 수정 후 workspace store 30/30 테스트가 통과했다.

### 최신 자동 검증

- continuity 핵심 테스트: Sidebar/cache 33/33, workspace store 30/30
- project switch/native lifecycle/App 관련 테스트: 76/76
- 전체 frontend: 88 files, 773 tests 통과
- production build: `tsc && vite build` 성공
- `git diff --check`: 통과

데스크톱 자동 조작은 사용자 지시에 따라 수행하지 않는다. 최종 수동 확인은 두 프로젝트 accordion을 펼친 상태의 A -> B -> A 목록 연속성, 그리고 탭이 없는 worktree를 처음 선택할 때 기존 terminal이 새 terminal 준비 완료 전까지 유지되는지를 확인한다.

## 2026-08-26 prompt alignment 및 shortcut index 후속 수정

### 전환 후 prompt가 80-column 경계로 밀리는 현상

사용자 screenshot과 run `5aa6d187-8a36-4bfa-88f0-8fcb7e875616`의 geometry를 비교했다. native surface는 sidebar 뒤의 정확한 `x=236`, `width=1044`에 있었고 cursor도 `col=2`로 올바르게 렌더됐다. 잘못된 부분은 compositor 위치가 아니라 daemon history replay 시점의 VT column 수였다.

`cmd_native_terminal_attach`는 DOM geometry를 받지 않았고, `attach_daemon_attachment`는 매번 `NativeTerminal::new(80, 24)`를 만든 뒤 history를 먼저 replay했다. shell RPROMPT가 right-margin cursor sequence로 기록돼 있어도 80-column grid에서 old right edge에 clamp됐다. 나중에 130 columns로 resize하면 기존 cells는 80-column 경계에 남고 오른쪽에 빈 50 columns만 추가됐다. Screenshot에서도 stranded prompt 시작점은 terminal surface left에서 약 640px, 즉 8px cell 기준 정확히 80 columns였다.

수정 내용:

- `NativeTerminalPane`가 attach 전에 DOM bounds와 device scale factor를 측정한다.
- `cmd_native_terminal_attach`가 optional bounds/scale을 받는다.
- Rust surface host가 해당 geometry로 terminal session을 먼저 create/resize한 뒤 daemon history를 replay한다.
- 실제 compositor render/present는 기존 `cmd_native_terminal_set_bounds`가 계속 담당한다.
- geometry를 보내지 않는 legacy caller는 기존 80x24 경로를 유지한다.

실패 우선 검증:

- frontend attach payload가 bounds/scale을 보내지 않아 실패
- target width 130으로 요청해도 replay snapshot이 80 columns여서 실패

수정 후 `NativeTerminalPane` 38/38, native surface host contract 12/12가 통과했다. Rust contract는 `CSI 999C` 기반 right-margin replay를 사용해 marker가 80-column 경계가 아니라 130-column right edge에 배치되는지 확인한다.

### Cmd+digit worktree selection 불일치

Sidebar가 보여 주는 row와 shortcut이 계산하는 `listVisibleWorktrees`가 서로 다른 규칙을 사용했다.

- Sidebar는 non-Git folder project에 synthetic root row를 표시하지만 shortcut list는 누락했다.
- inactive project에서 Sidebar는 cached rows를 먼저 표시하지만 shortcut list는 extra owned rows를 먼저 넣었다.

따라서 Cmd+1..9의 index가 화면의 위에서 아래 row 순서와 달라져 다른 worktree를 선택하거나 아무 것도 선택하지 않았다.

수정 후 shortcut list는 Sidebar와 동일하게:

- active project의 authoritative rows를 우선 사용하고 없을 때만 cache fallback 사용
- inactive project는 cached rows 다음 extra owned rows 순서 사용
- visible non-Git project가 비어 있으면 repo root synthetic row 생성
- collapsed project 제외 및 path dedupe 유지

수정 전 regression은 non-Git root 선택 호출 0회, inactive ordering은 expected cached row 대신 extra-owned row 선택으로 실패했다. 수정 후 App 67/67 테스트가 통과했다.

### 최신 검증

- focused frontend: App, NativeTerminalPane, shortcuts 132/132
- native terminal surface host contract: 12/12
- 전체 frontend: 88 files, 784 tests 통과
- frontend production build 및 `tsc --noEmit`: 성공
- `cargo check`, `cargo fmt --check`, `git diff --check`: 성공
- Rust의 기존 dead-code warning 4개는 이번 변경과 무관하며 그대로 유지

이 수정은 frontend와 Rust attach command를 함께 변경하므로 HMR만으로는 적용되지 않는다. 전체 Tauri dev process를 재시작한 뒤 확인해야 한다.

## 2026-08-26 돌아온 workspace의 open tab 소실 수정

사용자 재현 run `0d58ae02-bece-4e32-b6c5-a564d0f3e535`에서 tab 소실은 worktree layout reducer가 아니라 restore coordinator에서 발생했다.

- sequence 227/258: `maho-workspace`의 최신 HMR state는 탭 2개와 세션 2개를 정상 보유했다.
- sequence 260: startup 때 preload한 disk snapshot이 탭 1개/세션 1개 상태로 다시 선택됐다.
- sequence 261: `restoreWorkspace`가 최신 2-tab state를 오래된 1-tab snapshot으로 덮어썼다.

`preloadedRestoreStateByWorkspace`는 첫 렌더에서 프로젝트별 disk snapshot을 즉시 제공하기 위한 cache지만, entry를 사용한 뒤 삭제하지 않았다. 또한 `useWorkspaceRestore`는 `recoveredFromHmr`와 restore status보다 preload cache를 먼저 검사했다. 따라서 A -> B -> A로 돌아올 때마다 startup snapshot을 재적용해 그 이후 생성된 탭을 제거했다.

수정 내용:

- preload entry를 읽는 즉시 map에서 삭제해 one-shot으로 소비한다.
- `recoveredFromHmr=true`이면 HMR state를 더 최신으로 간주하고 preload를 적용하지 않는다.
- 이 경로는 `workspace.restore.preloaded.skipped-hmr` 구조화 로그를 남기고 restore status를 `restored`로 확정한다.
- 첫 non-HMR mount에서는 기존처럼 preload snapshot을 적용하므로 초기 프로젝트 전환의 first-render tab 복원은 유지된다.

실패 우선 regression은 1-tab persisted snapshot을 preload한 뒤 newer 2-tab HMR state로 restore hook을 mount한다. 수정 전 stale `restoreWorkspace`가 1회 호출돼 실패했고, 수정 후 호출 0회 및 HMR tab count 2 유지가 검증됐다.

최신 검증:

- workspace restore: 16/16
- restore/store/project switch/App 관련: 120/120
- 전체 frontend: 88 files, 785 tests 통과
- `tsc --noEmit`, production build, `git diff --check`: 성공
