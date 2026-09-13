# 워크트리 자동 감지: Orca 패리티 분석 (2026-09-12)

질문: "에이전트가 워크트리를 생성하는 경우 페릭스 프로젝트 리스트에서 자동 감지돼야 하는데 지금은 안 된다. Orca는 이걸 어떻게 감지하는가?"

Upstream 기준: github.com/stablyai/orca, 로컬 클론 `/Users/indo/code/project/orca` @ `20ab9950` (2026-09-11).
핵심 설계 문서: upstream `docs/reference/worktree-scan-fingerprint.md` ("Steady-state worktree rescan: Git-admin fingerprint gate").

## 결론 3줄

1. **Orca는 fs.watch를 쓰지 않는다** — per-repo 파일시스템 감시는 명시적으로 거부됨 (watcher 핸들/플랫폼 차이/dormancy-rearm 부담). 대신 2계층: "앱 내부 변화 = 이벤트 push(즉시)", "외부 변화 = TTL 기반 재스캔(≤30초)".
2. 외부 변화(`git worktree add/remove/move/prune`, `rm -rf`)를 잡는 주체는 **메인 프로세스의 30초 TTL 재스캔**이며, 그 비용을 **git-admin fingerprint 게이트**(서브프로세스 없는 stat/readdir/readFile 프로브)로 90% 줄인다.
3. Ferryx는 **주기 재스캔이 전혀 없고**, `worktree_changed` 이벤트도 Ferryx 자체 CRUD에서만 발화하므로 에이전트가 PTY에서 만든 워크트리는 포커스 전환/프로젝트 전환/재시작 전까지 절대 보이지 않는다(비활성 프로젝트는 리마운트 전까지 영구 스테일).

## Orca 아키텍처 상세

### 계층 1 — push (in-Orca 변화, 즉시)

- 발화원: managed worktree create/remove/rename/activate, `orca` CLI 명령(client event로 유입), SSH 상태 변화, 브랜치 rename 감지.
- 경로: `notifyWorktreesChanged(repoId)` → (1) `runWorktreeChangeInvalidators(repoId)` — 5초 detected-scan 캐시를 먼저 무효화해 렌더러가 stale TTL 엔트리를 못 읽게 함 — (2) `worktrees:changed` IPC push.
  - `src/main/window/runtime-window-lifecycle.ts:39-44`
  - `src/main/ipc/worktree-change-invalidators.ts`
- 렌더러 수신: `src/renderer/src/hooks/ipc-events/project-catalog-ipc-bridge.ts` → `worktreeChangeRefreshQueue` → `handleWorktreesChanged` (`src/renderer/src/hooks/ipc-events/worktree-event-runtime.ts`) → `fetchWorktrees(repoId)` 재조회 + 삭제 diff 기반 purge (out-of-band 삭제로 좀비 PTY 상태 방지).

### 계층 2 — pull (외부 변화, ≤30초)

메인 프로세스 2단 캐시:

- `resolvedWorktreeCache` (전체 플릿 스냅샷): TTL **1초** (`RESOLVED_WORKTREE_CACHE_TTL_MS = 1000`, `src/main/runtime/orca-runtime-postlude.ts:31`)
- `worktreeScanCache` (레포별): TTL **30초** (`WORKTREE_SCAN_CACHE_TTL_MS = 30_000`, `src/main/runtime/runtime-worktree-scan-cache.ts:4`; agent-scratch 레포는 5분)

TTL이 만료된 후 **누구든** 캐시를 읽으면 재스캔을 지불한다. 고빈도 내부 호출자(`listTerminals`, `showTerminal`, `getWorktreePs`, `listManagedWorktrees`, `resolveWorktreeSelector`, orchestration authority refresh)가 사실상의 폴러 역할을 해서, 실전 트레이스에서 플릿 전체가 ~30.5초마다 한 번씩 스윕되었다. 즉 별도 타이머가 아니라 "기존 트래픽 + TTL"이 감지기를 만든다.

### Git-admin fingerprint 게이트 (핵심 최적화)

`src/main/runtime/repo-worktree-admin-fingerprint.ts` + `orca-runtime-refresh-repo-worktree-scan.ts`의 `refreshRepoWorktreeScan`.

Git common dir를 서브프로세스 없이 해석(`.git` → `gitdir:` → `commondir`)한 뒤 다음을 읽어 NUL-구분 문자열 fingerprint를 만든다:

| 입력 | 잡아내는 외부 변화 |
|---|---|
| `<commonDir>/worktrees` 엔트리명 정렬 목록 | `worktree add/remove/prune` |
| repoPath 존재 여부 | 메인 checkout 삭제 |
| `packed-refs` / `reftable` mtime+size | tip 이동 |
| 각 checkout의 `HEAD` 내용 | 브랜치 스위치/detach |
| HEAD가 가리키는 ref의 내용 | 일반 commit/reset/fetch로 tip 이동 |
| 각 워크트리 `gitdir` 내용 | `worktree move/repair` |
| `locked` 존재 | `worktree lock/unlock` |
| `gitdir` 목표 경로 존재 | `rm -rf` (prunable 전환) |

캐시 결정(TTL 만료 시): probe → 같으면 TTL 연장(단, 마지막 실스캔 < 5분일 때만; `WORKTREE_SCAN_ADMIN_RECONCILE_INTERVAL_MS = 5 * 60_000` — 5분마다 강제 재조정으로 bounded convergence) / 다르거나 null이면 실제 `git worktree list` / probe 실패는 **fail-open**(무조건 스캔). fingerprint는 스캔 **직전에** 캡처해 스캔 중 착지한 변화가 다음 probe에서 반드시 잡히게 한다. SSH/WSL 라우팅 레포는 probe 대상 제외.

효과(문서 측정): 10레포 1Hz 폴링 30분 기준 `git worktree list` 600회 → 60회(-90%). 외부 변화 발견 지연은 여전히 ≤30초.

### 렌더러 계층

- `worktrees:list` / `worktrees:listDetected` IPC 핸들러에 별도 **5초 TTL 캐시**(`DETECTED_WORKTREE_SCAN_CACHE_TTL_MS = 5_000`, `src/main/ipc/worktrees/listing/detected-worktree-scan-cache.ts`) — 렌더러 폴링 버스트를 흡수하고 외부 변화 노출 지연을 "짧은 갱신 윈도우 1회"로 제한.
- 사이드바 재조회 트리거: `worktrees:changed`/`repos:changed` push, startup hydration, 설정 변경, CRUD, unread activity 등 이벤트/액션형. 전용 주기 폴러는 없음 — 메인 캐시가 ≤30초로 신선해지므로 어떤 재조회가 와도 최신 카탈로그를 받는다.
- 명시 거부된 대안(문서 "Rejected alternatives"): (a) TTL을 5분으로 올리기 — 모든 외부 변화 지연이 5분으로 나빠짐, (b) 레포별 `fs.watch` — watcher 핸들/플랫폼 차이/dormancy-rearm 복잡도.

## Ferryx 현재 상태 (갭)

갱신 트리거를 전수 조사한 결과, **전부 이벤트형**이고 외부 변화를 잡는 경로가 없다:

- `ui/src/state/workspaceRuntime.ts:243-262` — 초기화(마운트/등록/프로젝트 스위치) 시 1회
- `ui/src/state/workspaceRuntime.ts:266-268` — `window focus` 시 (활성 프로젝트만)
- `onWorktreeChanged("worktree_changed")` 이벤트 — 그러나 백엔드 발화점은 Ferryx 자체 플로우뿐: `src-tauri/src/ipc/worktree.rs:136,172,183,251` (create/delete/status), `src-tauri/src/ipc/ssh.rs:433,470`. 에이전트가 PTY에서 `git worktree add`를 실행해도 이 이벤트는 절대 발생하지 않는다.
- `ui/src/App.tsx:2649,2682` — AddWorktreeDialog/WorktreeDeleteDialog 콜백
- 비활성 프로젝트: `ui/src/state/inactiveProjectWorktrees.ts:121-165` — 마운트 시 1회 목록 후 재폴링 없음
- UI 측 setInterval은 permissions 온보딩/원격 접속 틱뿐(`ui/src` 전수 grep). 데몬/GUI 어디에도 워크트리 watcher·주기 재스캔 코드 없음(`src-tauri/src/worktree/` 내 watch/notify 0건).

결과: 앱이 이미 포커스된 상태에서 에이전트가 워크트리를 생성하면 어떤 신호도 없음 → 활성 프로젝트는 포커스를 떠났다 돌아올 때야 보이고, 비활성 프로젝트는 훅이 리마운트되기 전까지 영구 스테일.

## 권장 설계 (Orca 모델 포팅)

워크트리 레지스트리는 GUI(Tauri) 프로세스에 있으므로(`WorkspaceRegistry`, 데몬 아님), 감지기도 GUI 프로세스에 둔다.

1. **Tauri setup에서 주기 재스캔 태스크**: 등록된 각 git 워크스페이스에 30초 TTL 인터벌. 발화 시
   - fingerprint 프로브(std::fs stat/readdir/readFile만, git 서브프로세스 0 — 동기 I/O는 기존 규칙대로 `run_blocking` 경유) → 이전 fingerprint와 같으면 스킵(≤5분마다 1회는 강제 재조정), 다르면 `git worktree list` 재실행.
   - 결과가 기존 목록과 다으면 기존 `emit_worktree_changed` 채널로 emit (`kind` 확장: `created`/`removed`/`identityChanged` — 현재 페이로드는 단일 worktree identity라, 목록 diff에 맞게 repeat-emit 또는 payload 확장 필요).
2. **활성 프로젝트**: 기존 `onWorktreeChanged` 리스너가 이미 `refreshWorktrees`를 부르므로 GUI 측 수정 최소.
3. **비활성 프로젝트**: `useInactiveProjectWorktrees`가 `worktree_changed`도 구독해 해당 프로젝트만 재조회 (페이로드에 `workspaceId` 이미 있음).
4. in-app CRUD는 기존 즉시 emit 유지 — push 계층은 이미 패리티.

검증 포인트(구현 시): 에이전트 PTY에서 `git worktree add` → ≤30초 내 사이드바 반영(활성/비활성 모두); fingerprint 프로브가 steady state에서 git 서브프로세스를 0회 발생; probe 실패 시 fail-open으로 실스캔.

## 구현 결과 (2026-09-13)

권장 설계대로 구현 완료. 설계 대비 확정된 차이: kind는 `removed`/`identityChanged` 대신 기존 어휘를 재사용 — 목록 제거는 기존 `pruned`, 헤드/브랜치/lock/prunable 변화는 신규 `updated` (payload 확장 없이 기존 `WorktreeIdentity` 페이로드 유지).

### 백엔드 (src-tauri)

- `src/worktree/rescan.rs` (신규):
  - `read_worktree_admin_fingerprint(repo_root)` — 서브프로세스 0 프로브. `.git` → `gitdir:` → `commondir` 해석 후 worktrees 엔트리명/HEAD/gitdir/locked/체크아웃 존재, main+연결루트 HEAD 및 loose ref 내용(부재 시 packed 마커), 루트 존재, packed-refs/reftable mtime+size를 NUL 구분 문자열로. 예기치 않은 IO 오류는 `None`(fail-open). ref 추종은 `refs/` 상대경로만 — 경로 탈출 차단.
  - `probe_fingerprint` — 전용 스레드 + `recv_timeout(2s)`. 타임아웃 시 해당 워크스페이스는 `probe_wedged` 영구 설정(Orca의 stuck-gate 패리티) → 이후 모든 스윕이 실스캔.
  - `diff_worktrees(old, new)` — 체크아웃 경로 키로 created/pruned/updated 산출. identity는 `orca/<ws>/<slug>` 브랜치 파싱, 외부 워크트리는 `ws_id="detected"` + 디렉터리명 폴백.
  - `sweep_registry_once` — 첫 관찰은 무음 베이스라인(이벤트 없음). fingerprint 동일 + 5분 미경과 → 실스캔 스킵(last_scan만 갱신), 그 외 실스캔 후 diff emit. fingerprint는 스캔 직전 캡처.
  - `spawn_worktree_rescan_task` — 30초 interval(MissedTickBehavior::Delay), 스윕은 `run_blocking` 경유(동기 I/O 규칙 준수), 기존 `emit_worktree_changed` 채널로 emit. 비-git 워크스페이스/SSH는 스캔 제외.
- `src/worktree/mod.rs` — `pub mod rescan` 재export, 테스트 헬퍼 `setup_test_repo`를 `pub(crate)` 승격.
- `src/ipc/worktree.rs` — `WorktreeChangeKind::Updated` 추가 (wire: `"updated"`).
- `src/lib.rs` — setup 훅에서 `spawn_worktree_rescan_task` 스폰 (`workspace_registry` 클론 소비).

### 프론트엔드 (ui)

- `src/lib/types.ts` — kind 유니언에 `"updated"` 추가.
- `src/state/inactiveProjectWorktrees.ts` — 비-SSH 프로젝트 이벤트 필터에 `created`/`updated` 포함 (dirtyChanged만 계속 무시). SSH는 기존대로 전 종류 처리.
- `src/state/workspaceRuntime.ts` — `worktree_changed` 리스너가 `payload.workspaceId === workspaceId`일 때만 refresh (스윕이 모든 워크스페이스로 emit하므로 크로스 프로젝트 재스캔 방지).

### 테스트

- Rust (rescan.rs cfg(test), 11개): fingerprint 안정성 + worktree add/commit/checkout/lock/unlock 변화 감지, 연결 워크트리 루트에서의 관찰, 비-git → None, diff 4종(created/pruned/updated/외부 identity), 스윕 베이스라인 무음 → created → 정착, head 이동 → updated, 삭제 → pruned, fingerprint 위장 시 게이트 억제 + 5분 재조정 커버, **연속 30초 틱에서도 5분 재조정이 반드시 발화함을 고정하는 테스트**, probe 타임아웃 영구 폴백(첫 스윕 위지 포함), wire 직렬화.
- UI (inactiveProjectWorktrees.test.tsx, 12개): rescan `created`/`updated` 이벤트로 비활성 프로젝트 재목록화, `dirtyChanged` 무시 테스트 추가.

### 코드 리뷰 반영 (2026-09-13, 단일 게이트 리뷰어)

리뷰 결론 REQUEST_CHANGES의 blocker 2 + should-fix 3 모두 수정:

- **재조정 기아 (blocker)**: 게이트 스킵 시 `last_scan`을 갱신하던 것이 원인 — 30초 간격 연장이 5분 강제 재조정을 영원히 미루게 됨. `last_scan`을 "마지막 실스캔 전용"으로 분리하고 게이트 확장은 더 이상 건드리지 않음. 연속 틱 테스트로 고정.
- **스캔 격리 (blocker)**: 태스크 루프를 워크스페이스별 bounded 스캔으로 재구성 — `tokio::time::timeout(30s)` + 워크스페이스당 개별 `run_blocking` + 즉시 emit. 하나의 wedged 마운트가 다른 워크스페이스 스윕을 막거나 수집된 이벤트를 묵두는 일이 없음.
- **첫 스윕 위지 소실**: 위지 상태를 엔트리에서 분리해 `wedged: Mutex<HashSet<String>>`로 영속 — 실스캔 실패/베이스라인 부재와 무관하게 타임아웃 즉시 기록되어 추가 stuck 스레드 방지.
- **ref 추종 탈출구**: 백슬래시/콜론/선두 슬래시도 거부(Win32가 구분자로 해석), symlink out-of-scope 신뢰 경계 명시.
- **commondir fail-open**: 예기치 않은 IO 오류를 안정값으로 둔갑시키지 않고 프로브 실패(None)로 전파 — linked checkout의 잘못된 common dir 폴백 제거.
- 수용 안 함(근거): workspaceRuntime 크로스워크스페이스 필터의 별도 테스트 — 런타임 풀 하니스 필요 대비 1라인 술어이며 훅 레벨 동일 술어 테스트로 커버됨. 실서피스 QA(`bun tauri dev`)는 배포 전 수동 단계로 유지.

### 검증 (수정 후)

- `cargo test --lib worktree::rescan` — 11/11 통과. `cargo test --lib` — 1047 통과 / 2 실패(사전 존재·무관, 변경 전과 동일).
- `bun run --cwd ui test src/state/inactiveProjectWorktrees.test.tsx` — 12/12 통과.
- 리뷰 보고서: `.omo/evidence/worktree-auto-detect-code-review.md`.
