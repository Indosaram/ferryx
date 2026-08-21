# Orca Lite Parity Audit — 판정 및 수정 구현 계획

> 작성일: 2026-08-21
> 방법: delegate-web-dag — ChatGPT Web 감사 워커 2명(프론트/백엔드 분리) + 로컐 검증 게이트 4종 + fan-in 검증인 (run `dag_76a2f4ca-c17d-4a6d-b948-c30aca745a68`)
> 기준선: 현재 working tree — main `f5efc72` + 미커밋 수정 레이어 포함 (감사 시점 그대로)
> 참조: `ORCA_LITE_FIX_PLAN.md` (설계 계획), `ui/original-dist/` (원본 Orca Electron 렌더러 = canonical parity 기준), `.omo/drafts/orca-ui-recovery.md` (스코프 경계 원본)
> 원본 판정 기록: `.omo/senpi-task/dag/results/dag_76a2f4ca-c17d-4a6d-b948-c30aca745a68/final-verifier.txt`

## 1. 종합 판정

**부분 준수 — 백엔드·계약 계층은 설계 계획대로 구현됨. UI/UX 축은 by-design gap을 제외해도 파리티 미달. 현재 상태로 릴리스 승인 불가.**

- Phase 1~5 핵심 불변식(PTY lifecycle, 1-Writer-1-Worktree, dirty/unmerged 삭제 방어, Git 인자 하드닝, CSP, camelCase + 구조화 에러)은 코드와 테스트 양쪽에서 실증.
- Phase 8 성능(spawn_blocking Git I/O, xterm/WebGL 지연 로딩)도 실증.
- 그러나 gap 제외 후에도 결함 11건 잔존. 최악 1건(D1)은 백엔드 lease와 프론트엔드 spawn 순서가 모순되는 **런타임 차단 결함**.

## 2. 검증 게이트 결과 (감사 시점 트리, 4/4 exit 0)

| 게이트 | 결과 | 비고 |
|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | **PASS** — 45 passed / 0 failed | unit 26 + integration 8/1/3/7 |
| `cargo clippy --all-targets -- -D warnings` | **PASS** — 0 경고 | 콜드캐시(`CARGO_TARGET_DIR` scratch) 재검으로 미커밋 소스 실제 lint 확인 |
| `bun run --cwd ui test` | **PASS** — 8 files / 26 tests | |
| `bun run --cwd ui build` | **PASS** | eager 218.14kB(gzip 68.29) / lazy xterm 332.63kB / webgl 111.96kB, 청크 경고 없음 |

게이트 로그: `/tmp/orca-parity-audit/*.log` (휘발성), 요약 `/tmp/orca-parity-audit/local-gates.txt`

## 3. 직전 검증 실패 5건 재확인 — 5/5 해소

1. `PtyManager::spawn_in_worktree` 누락 컴파일 실패 → `src-tauri/src/terminal/pty.rs:56` 정의, worktree_safety 7/7 green
2. e2e `UnmergedBranch` unwrap panic → `tests/e2e_agent_workflow.rs:104`가 destructive API 사용, 1 passed
3. clippy `WriterLeaseGuard` 미사용 → `manager.rs:257` 생성, `pty.rs:66`·`session.rs:37,47` 소비
4. `ipcErrors.test.ts` 임포트 실패 → `ui/src/lib/ipcErrors.ts` 존재, 2 tests green
5. `createWorktree` path 필드 불일치 → 리디자인으로 해소: `CreateWorktreeRequest`(`ipc/worktree.rs:13-17`)는 `workspaceId` + `worktree{wsId,slug}` + `baseRef`, 경로는 서버에서 파생(`:94-96`), `deny_unknown_fields`로 과잉 필드 거부

## 4. by-design gap (결함 아님 — 스코프 경계 명시 항목)

브라우저 pane · 에디터/Monaco · PR 뷰 · 원격 호스트 관리 · 자동화 · 에이전트 오케스트레이션(dashboard/kanban/map/notes) · `src-tauri/src/ipc/*`에 없는 IPC에 의존하는 모든 표면 (현재 IPC = worktree + terminal뿐, `lib.rs:28-39`).

알려진 한계(추후 오케스트레이션 지원 시 재검): `selectAgents`가 터미널 탭을 `ActiveAgent`로 투영 (`ui/src/state/workspaceStore.ts:224-241`). 하드코드 샘플 제거라는 Phase 6.3 요건은 충족하지만, 진짜 agent 세션 식별은 백엔드 기능 부재로 불가 — gap 항목에 귀속.

## 5. 수정 구현 계획 (우선순위별)

### P0 — 런타임 차단 결함

#### D1. same-worktree 이중 writer (차단급)
- **문제**: 분할 활성화 시 같은 worktree에 두 번째 세션을 spawn하고(`workspaceStore.ts:165`), 마지막 탭 교체 시 기존 세션 close **전에** 교체 세션을 먼저 spawn(`:184` → `:186`). 백엔드 lease(`src-tauri/src/worktree/manager.rs:248`)가 둘 다 `WRITER_ALREADY_ACTIVE`로 거부 → single-tab split·last-tab close가 실제 런타임에서 깨짐. Phase 2 완료조건 "single-tab split 정상 작동"·"마지막 tab close 후 valid state" 무력화.
- **수정 방향**: close-then-spawn 직렬화 + secondary pane에 primary 세션 재사용, 또는 lease를 writer-intent 단위로 완화(백엔드 논의 필요).
- **완료 조건**: 실 백엔드 연동으로 single-tab split 및 마지막 탭 닫기가 lease 거부 없이 동작.

#### D2. D1을 못 잡는 테스트 맹점 (D1과 동일 증분으로 수정)
- **문제**: `workspaceStore.test.tsx:33, :72`가 lease 의미론 없는 가짜 `spawnTerminal`을 주입해 실제 백엔드에서 실패할 시나리오가 통과함.
- **수정 방향**: 스파이가 writer-lease 거부(`WRITER_ALREADY_ACTIVE`)를 흉내 내도록 스펙 강화.
- **완료 조건**: D1 수정 전 상태에서 이 테스트가 RED가 되는지 확인(수정 후 GREEN).

### P1 — UX 파리티 코어 (gap 제외 영역에서 원본 대비 결함)

#### D3. 키보드 단축키 사실상 전무
- **증거**: `ui/src` 내 유일 핸들러 `TabBar.tsx:41`(Enter/Space 닫기). 원본은 `plugin-manifest`에서 terminal `splitRight/splitDown/closePane/copySelection/paste/clear/search/focusNextPane`, tab `close/newTerminal/next/previous/selectByIndex` 등 30여 바인딩 — 전부 `App.tsx:70-86`에 이미 구현된 기능에 매핑됨.
- **완료 조건**: terminal/tab 계열 최소 바인딩(신규/닫기/다음/이전 탭, split right/down, pane 닫기) 구현 + 단축키 표시.

#### D4. 설정 표면 없음 + 동작 안 하는 Settings 버튼
- **증거**: `Sidebar.tsx:92-94` onClick 없는 IconButton. 원본은 9+ 섹션(`Settings-yKTVxZPa.js`). terminal/appearance/shortcuts/input/notifications 섹션은 gap 예외가 **아님**. 동작하지 않는 버튼 노출은 부재보다 나쁨.
- **완료 조건**: Settings 버튼 제거 또는 최소 설정(터미널 외관/단축키) 연결.

#### D5. inert 사이드바 내비게이션
- **증거**: `Sidebar.tsx:25`(Workspace), `:32`(Agents), `:42`(Search — `⌘K` 힌트만 있고 바인딩 없음), `:62`(Agent options).
- **완료 조건**: 동작 연결 또는 제거. `⌘K`는 구현하거나 힌트 제거.

#### D6. 정의되지 않은 디자인 토큰
- **증거**: `tailwind.config.js:10-13`은 `background`/`foreground`뿐, `index.css`에 CSS 변수 없음 — 그런데 `Sidebar.tsx:19`(`w-sidebar bg-worktree-sidebar`), `:41`(`bg-primary`), `:89`(`bg-status-success`) 등 미정의 유틸리티 사용. 원본 기준: `I18nProvider-*.css` zinc 다크 토큰(`--background #0a0a0a`, `--card #171717`, `--muted #262626`, `--accent #404040` 등).
- **완료 조건**: semantic 토큰 체계 정의 + 미정의 유틸리티 0건.

#### D9. 백엔드 명령 12개 중 8개만 wrapper 존재
- **증거**: `ui/src/lib/tauri.ts`에 없는 6개 — `cmd_worktree_delete`, `_delete_destructive`, `_delete_preview`, `_status`, `cmd_terminal_signal`, `cmd_terminal_list` (`lib.rs:31,33,36,37,38,39`). 삭제 안전 플로우(preview→safe→destructive)와 terminal signal이 UI에서 접근 불가.
- **완료 조건**: 6개 wrapper + 해당 UI 플로우(삭제 확인 시 branch/HEAD/upstream/merge 정보 표시 — Phase 3.4 요건).

### P2 — 파리티 보강

#### D7. 사이드바 리사이즈 없음
- `Sidebar.tsx:19` 고정 `w-sidebar`. 원본: `useSidebarResize-BhlGhEjK.js`.

#### D8. 분할 divider 드래그 없음
- `TerminalSplitView.tsx:23-45`에 divider 엘리먼트/로직 없음.

#### D10. `worktree_changed` 이벤트 종류 미달 (Phase 6.1 이탈)
- **증거**: `src-tauri/src/ipc/worktree.rs:36-40`은 `Created`/`Deleted`/`DestructivelyDeleted`뿐. 계획 §6.1(`ORCA_LITE_FIX_PLAN.md:562-570`)은 dirty-state change, lock/prune, branch change 요구.

#### D11. 문서 드리프트 (사소)
- `PERF_NOTES.md:50` "25 tests" → 실제 26.

## 6. runtime 미검증 항목 (정적 감사로 증명 불가 — 릴리스 전 실기 필수)

- macOS 네이티브 Tauri PTY interactive smoke (open/resize/split/close, worktree create/delete)
- Ctrl-C foreground process group 전달
- terminal 10회+ 반복 open/close 누수 (process/session/lease)
- spawn 직후 프롬프트 유실 여부 (listener-before-spawn은 코드상 보장)
- 실 GPU에서 Canvas fallback / context loss 동작

## 7. 증거 아티팩트

| 항목 | 경로 |
|---|---|
| 최종 판정 (authoritative) | `.omo/senpi-task/dag/results/dag_76a2f4ca-c17d-4a6d-b948-c30aca745a68/final-verifier.txt` |
| 게이트 상세 | `/tmp/orca-parity-audit/local-gates.txt`, `*.log`, `*.exit` (휘발성) |
| Web 감사 요약 | `/tmp/orca-parity-audit/reports/{orca-parity-frontend,orca-parity-backend}.md` (휘발성) |
| Web 감사 스코프 | `47061838…`/`e605dd95…` — 검증 수용 후 폐쇄됨. 상세 테이블은 폐쇄된 대화에 있었으나, 판정 근거 인용 10건은 검증인이 repo 소스에서 전부 재확인 완료 |
