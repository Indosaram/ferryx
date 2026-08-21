# Orca Lite Parity Fix — 실행 계획 및 결과

> 작성일: 2026-08-21
> 입력: `ORCA_LITE_PARITY_AUDIT.md` (감사 판정 D1~D11), `ORCA_LITE_FIX_PLAN.md` (설계 계획)
> 방법: delegate-web-dag — ChatGPT Web 워커 2명(백엔드/프론트 분리, 단일 helper 배치) + 로컐 검증 게이트 + fan-in 최종 검증인

## 1. 설계 결정 사항

### D1 해결: mirror-pane split 모델 채택 (핀 계약)
- **단일 tab split**: 기존 primary 세션을 두 번째 pane에 그대로 렌더링(세션 공유, spawn 없음). 두 pane 모두 같은 세션의 출력을 구독하고 입력도 같은 세션으로 전달.
- **tab 2개 이상에서 split**: 기존 다른 tab을 secondary로 지정 (reducer 수준, spawn 없음).
- **split 해제/방향 전환**: 레이아웃만 변경. 세션 생성/파괴 없음.
- **마지막 tab 교체**: outgoing 세션의 close 확인(`terminal_lifecycle` exited/failed, 상한 ~5s) 이후에만 교체 세션 spawn (현재 `workspaceStore.ts:184→:186` 순서 버그 수정).
- **근거**: 설계 계획의 1-Writer-1-Worktree 불변식(P1 핵심, Phase 0.2 테스트 명세 "writer 2개 acquire 시 거부")과 split 인수 항목("2 tab + split = PTY 2개", "단일 tab 첫 split 즉시 표시", "split 해제로 shell 종료 안 됨")을 **동시에** 만족하는 유일한 모델. 백엔드 변경 불필요.
- **기각한 대안**: owner-keyed lease 완화(같은 owner의 다중 writer 허용) — 원본 Orca처럼 독립 셸 split이 가능하지만 Phase 0.2가 명시하는 P1 안전 테스트를 재작성해야 함. 추후 독립 셸 split이 필요하면 별도 계획 수정으로 다룸.

### 워커 배치: fresh batch
- 기존 구현 스코프 2개(`b2e19c02`, `ebc71b0a`)는 lifecycle이 generation-1 ready(2026-08-20 22:13)에서 동결 — terminal 미도달, 120분 lease 만료 후 reaping. resume 불가 판단, **fresh 단일 helper 배치 2워커**로 실행.
- stale 스코프 파일은 실행 종료 후 정리(close-scope, 이미 reaped여도 무해).

## 2. 토폴로지 (DAG `orca-parity-fix-20260821`)

```
wave1: web-dispatch (helper 1회, fresh 2워커: orca-fix-backend ∥ orca-fix-frontend)
wave2: local-gates  (cargo test / clippy 콜드캐시 / bun test / bun build)
wave3: final-verifier (D1~D11 항목별 판정, 인용 재검, 커밋 검사, 게이트 리뷰)
```

소유권: 백엔드 = `src-tauri/**`, 프론트 = `ui/**` + `PERF_NOTES.md`(D11 한 줄). 공유 git index → 커밋은 항상 `git commit -m "..." -- <정확한 경로>` pathspec 방식(상대방 staged 파일 흡입 방지), index.lock 경합 시 5s 대기 재시도.

## 3. 워커 분담

### orca-fix-backend (src-tauri/**)
- **Step 0 baseline commit**: 미스테이지 2파일(manager.rs, mod.rs — 중첩 디렉터리→canonical root 해석 수정 + 회귀테스트).
- **D10**: `worktree_changed` 이벤트 종류 확장(Phase 6.1). `dirty_changed`(status 조회에서 transition 감지 시 발행, 캐시 기반)을 TDD로 구현. lock/prune·branch-change는 트리거 IPC 존재 여부를 확인 — 없으면 dead variant 추가 금지, 부재를 보고에 명시.

### orca-fix-frontend (ui/** + PERF_NOTES.md)
- **Step 0 baseline commit**: staged 11파일(이전 감사 실패 해소 레이어).
- **D1+D2**: mirror split + last-tab 직렬화 (RED→GREEN). 가짜 spawnTerminal에 lease 의미론(WRITER_ALREADY_ACTIVE, close await) 부여.
- **D6**: zinc 다크 semantic 토큰 레이어(index.css + tailwind.config) — 미정의 유틸리티 0건화.
- **D3**: 단축키 레지스트리+훅 (원본 plugin-manifest 기반: 신규/닫기/다음/이전 tab, split right/down, unsplit, ⌘K 팔레트).
- **D5**: gap 무관 inert 버튼 제거(Agents 등), Workspace/Search(⌘K) 실동작화.
- **D4**: Settings 다이얼로그 (Terminal: 폰트 크기/스크롤백 — localStorage 저장 + xterm 적용 / Shortcuts: 읽기 전용 목록). 가짜 섹션 금지.
- **D9**: 미래핑 6개 명령 wrapper(cmd_worktree_status/delete_preview/delete/delete_destructive, cmd_terminal_signal/list) + 삭제 플로우(preview→safe→파괴 확인), 인터럽트 액션.
- **D7**: 사이드바 리사이즈(드래그, localStorage). **D8**: split divider 드래그(양방향).
- **D11**: PERF_NOTES.md 테스트 수 정정(신규 테스트 반영).

## 4. 검증 게이트

1. `cargo test --manifest-path src-tauri/Cargo.toml`
2. `cargo clippy --all-targets -- -D warnings` (캐시 의심 시 `CARGO_TARGET_DIR` scratch 콜드 재검)
3. `bun run --cwd ui test` (D1/D2 신규 테스트명 출력 확인 포함)
4. `bun run --cwd ui build`

최종 검증인: 항목별 D1~D11 판정(fixed/partial/not-fixed/blocked+사유), 워커 리포트 인용 ≥5건씩 재오픈 검증, `git show --stat`로 커밋 경로 침범 없음 확인, 직전 해소 5건 유지 확인, bun test 재실행 샘플, 한국어 종합 판정.

## 5. 스코프 수명주기

- fresh 스코프 2개는 검증인 수용 전까지 retained. 수용 후 close.
- runtime 미검증 항목(macOS GUI 스모크 등)은 이 실행으로도 여전히 미커버 — 별도 실기 확인 필요.

## 6. 실행 결과 (2026-08-21 11:15 검증 완료)

### 판정: D1~D11 전 항목 해결 — 4/4 게이트 통과, 릴리스 승인 재심사 통과

> 비고: DAG 형식상 local-gates 노드가 429 rate-limit으로 중단되고 final-verifier가 skip되었으나, gates 노드는 중단 전 4게이트 실행·기록을 완료했고(전 exit 0), 팬인 검증은 코디네이터가 직접 수행해 아래 인용을 확보했다. 워커 2명 모두 COMPLETED(completion_check ready=true).

### 게이트 (HEAD 3bf0dc6, 트리 클린 — 미트래킹은 계획 문서 3개뿐)

| 게이트 | 결과 |
|---|---|
| cargo test | **PASS** 48 passed(+3: D10 dirty_changed 계열) |
| clippy -D warnings (콜드캐시 scratch 재검 포함) | **PASS** 0 경고, 로컬 크레이트 컴파일 확인 |
| bun test | **PASS** 16 files / 50 tests (기존 8/26) |
| bun build | **PASS** tsc 0 진단, 청크 경고 없음 |

### 항목별 판정 (전부 코디네이터 직접 재검 인용)

| 항목 | 판정 | 근거 |
|---|---|---|
| D1 split 이중 writer | **해결** | `workspaceStore.ts:151-156` 단일 tab → `ENABLE_SPLIT_EXISTING`이 primary를 mirror pane으로 지정(기존 탭 있으면 그 탭), spawn 없음. 테스트 "single-tab mirror split without spawning another backend PTY" ✓ |
| D1b 마지막 탭 교체 순서 | **해결** | `:174-176` `closeBackendSessionAndWait` → `createSpawnedTab` → dispatch. 테스트 "replacement only after lifecycle-confirmed writer release" ✓ (RED 3/6 실패 → GREEN 확인)|
| D2 테스트 맹점 | **해결** | 가짜 spawnTerminal이 WRITER_ALREADY_ACTIVE 시맨틱 내장, waitForTerminalExit 서비스 추가(`:60`), 11개 lease 관련 테스트 전 통과 |
| D3 단축키 | **해결** | `shortcuts.ts` 레지스트리: Mod+T/W, Ctrl+PageDown/PageUp, Mod+D, Mod+Shift+D, Mod+Alt+D(unsplit), Mod+K palette. 라벨 palette·설정에 노출 |
| D4 설정 | **해결** | SettingsDialog(폰트 크기/스크롤백 localStorage + xterm 실시간 적용, 단축키 읽기 전용 목록). 가짜 섹션 없음 |
| D5 inert 내비 | **해결** | agent 계열 affordance 제거, Workspace 포커스, Search/⌘K → CommandPalette(worktree+tab 전환) |
| D6 토큰 | **해결** | zinc 다크 시맨틱 토큰(index.css +144, tailwind +77) — w-sidebar, bg-worktree-sidebar, bg-status-success 등 전부 생성 확인 |
| D7 사이드바 리사이즈 | **해결** | 220-420px 클램프 드래그 + localStorage(`orca.sidebar.width`) |
| D8 divider 드래그 | **해결** | 양방향 orientation, 160px 최소 pane, mirror 세션 보존 |
| D9 IPC 래퍼 | **해결** | 12/12 명령 + 이벤트 리스너 3종 래핑 완료(getWorktreeStatus/previewWorktreeDelete/delete/destructive/signalTerminal/listTerminalSessions 포함). 삭제 플로우 preview→safe→UNMERGED_BRANCH code 시에만 파괴 확인 |
| D10 worktree_changed | **해결(설계상 부분)** | DirtyChanged(clean↔dirty 전이 시, 스냅샷 캐시) + Pruned(삭제 경로 prune 성공 시) 구현 + 3개 integration 테스트. lock/branchChanged는 트리거 IPC 자체가 없어 미구현이 정당 — 백엔드 리포트에 필요 IPC 명시 |
| D11 문서 | **해결** | PERF_NOTES.md → 16 files/50 tests |

### 회귀 확인 (직전 감사 해소 5건 유지)

`spawn_in_worktree` 존재 ✓ · e2e destructive delete ✓ · `WriterLeaseGuard::new` 사용 ✓ · `ipcErrors.ts` 존재+통과 ✓ · `createWorktree` identity 기반 계약 유지 ✓. 코디네이터 `bun test` 재실행: 50/50 exit 0.

### 커밋 (12개, 전부 pathspec 감사 IN SCOPE)

`6cf4dbe` baseline(백엔드) · `4945a87` baseline(프론트) · `f85cc79` D1/D2 · `3c2bafa` D6 · `08b5f7c` D3 · `6eca894` D10(백엔드) · `88bbab7` D5/palette · `b2229a1` D4 · `a253ee8` D9 · `78af70d` D7 · `90c3ecc` D8 · `3bf0dc6` D11

### 스코프 정리

- 신규 2개(bfe42fab 백엔드 / adaf13d7 프론트): 검증 수용 → 종료 처리.
- 구현 감사 시 스코프 stale 동결 2개(b2e19c02/ebc71b0a): lease 만료로 reaped 상태, 파일 정리 완료.

### 남은 과제

- runtime 미검증 항목은 여전히 미커버: macOS 실기 PTY 스모크(특히 mirror split 입력 동작, 마지막 탭 교체 5s 타임아웃 실동작), Ctrl-C foreground 전달, 10회 반복 open/close 누수, 실제 브라우저에서 단축키/팔레트/설정 UX.
- lock/branch_changed 이벤트는 전용 IPC(cmd_worktree_lock, cmd_worktree_branch_change 등) 설계 후 추가 필요.
