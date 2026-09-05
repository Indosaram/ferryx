# Code Review: Native Terminal 스크롤 하단 고정(Bottom-Lock) 수정

**리뷰 일시**: 2026-09-04
**리뷰 방식**: review-work 5-Agent 병령 리뷰 (Goal Oracle / QA Executor / Code Quality Oracle / Security Oracle / Context Miner)
**심수 대상**:
- `src-tauri/src/native_terminal/surface_host.rs` (스크롤 하단 고정 수정 + 동시 작업 에이전트 감지 스로틀 상호작용)

---

## 1. Executive Summary & Verdict

**Verdict**: **수정 완료 — 승인 (프로덕션 배포 준비 완료)**

원래 과업(윈도우 리사이즈/페인 닫기 시 뷰포트가 하단에서 이탈하는 현상 수정)은 5개 리뷰 에이전트 전원이 통과 판정. 리뷰 과정에서 발견된 차단 결함 1건(동시 작업인 에이전트 감지 스로틀의 trailing 전이 누락)과 보안/품질 지적 3건은 모두 이번 세션에서 수정 완료 및 테스트로 잠금.

- Goal Oracle: **PASS** (단, 스로틀 Hole A 지적 → 수정됨)
- Security Oracle: **PASS** (MEDIUM 1건 지적 → 수정됨)
- Quality Oracle: **CHANGES_REQUESTED** (3건 → 전부 수정됨)
- Context Miner: **GAP 없음** (모든 그리드 변경 경로가 하단 복원으로 커버됨 확인)
- QA Executor: **PASS** (변이 검증 기반, F-1/F-2 후속 2건 → F-2는 테스트 추가로 잠금, F-1은 문서화)

QA 증빙: `.omo/evidence/qa/st_01a06cd6/ferryx-scroll-fix-manual-qa.md`

---

## 2. 원래 수정 사항 (스크롤 하단 고정)

### 2.1 근본 원인 (코드 확인 완료)
1. 리사이즈 로직이 `sb.offset < max_offset`이면(트랙패드 관성으로 하단 1줄 위만 해도 참) 무조건 `prior_scroll_ratio`를 계산해 `Row(target_offset)`을 강제 → Ghostty 뷰포트가 `.active`(하단 고정)에서 `.pin`(행 고정)으로 전환, 이후 리플로우에서 뷰가 하단 위에 좌초.
2. 하단에 있을 때(ratio `None`) 리사이즈 후 명시적 하단 복귀 호출이 아예 없었음.
3. `terminal.reset()` + 히스토리 재후급(attach 기존/신규 분기, pump Lagged/Gap 복구) 후에도 `scroll_viewport(Bottom)`이 없어 재생 직후 뷰포트가 상단 부근에 먬무름.

### 2.2 수정 내용
- `is_at_bottom` 감지: 스크롤백 없음 또는 `sb.offset >= max_offset - BOTTOM_LOCK_TOLERANCE_ROWS` (상수 = 2행, 트랙패드 관성 허용치 문서화).
- 하단 상태였다면 리사이즈/재생 직후 `ScrollViewport::Bottom` 강제( `.active` 복원).
- 의도적으로 위로 스크롤한 경우엔 기존대로 비율 보존(`Row(target_offset)`).
- 적용 위치: `prepare_session_layout`, `reattach_existing_session_with_bounds`, `attach_daemon_attachment` (기존+신규 분기), pump Lagged/Gap 핸들러.
- **Bug B (Goal Oracle 발견, 선존재 결함)**: Lagged 핸들러는 segmented 재후급 후 그리드가 마지막 세그먼트 치수로 남는 문제 수정 — `session.layout` 치수로 재리사이즈 후 Bottom 적용(잘못된 그리드의 하단 고정 방지).

### 2.3 Ghostty 시맨틱 검증 (Security/Quality Oracle이 vendor 소스로 직접 확인)
- `PageList.scrollbar()`: `.active`일 때 offset == total_rows - rows == max_offset (산술 일치).
- `scroll(.row(n))` 자체 클램프: n==0 → `.top`, n ≥ total_rows - rows → `.active`. 따라서 비율 복원 분기가 하단 근처에서 `.pin`을 재도입할 수 없음.
- `f64 as usize`는 saturating 캐스트(NaN → 0)라 패닉 불가; 상류에 row 9999 클램프 회귀 테스트 존재.
- 뮤텍스 위생: 6개 삽입 지점 모두 guard 스코프 내 await 없음, 이중 잠금 없음 (pump의 feed와 동일 parking_lot 뮤텍스라 pre-read/resize/post-read 원자성 보장).

---

## 3. 리뷰 차단 결함 (동시 작업 스로틀) 및 수정

### 3.1 Hole A — trailing 상태 전이 누락 (Quality/Goal Oracle: 차단)
**결함**: 에이전트 감지 50ms 스로틀은 수신 메시지에서만 평가됨. 부착된 패인의 출력 버스트가 50ms 안에 끝나면(에이전트가 마지막 프레임 출력 후 입력 대기로 침묵하는 정형적 패턴) 마지막 프레임의 Working→Blocked/Idle 전이가 **영원히 평가되지 않음** → 스피너 무한 회전.

**수정**:
- `agent_detect_pending` 플래그 추가: 스로틀이 청크를 건너뛸 때 설정.
- pump 루프를 `tokio::time::timeout(AGENT_DETECT_TRAILING_IDLE = 60ms, messages.recv())`로 변경 — 버스트가 조용해지면 강제 trailing 감지 1회 실행 (bounded-wait, 유휴 패인 비용은 뮤텍스 락 1회/60ms로 무시 가능).
- 회귀 테스트 `attached_throttled_burst_still_emits_trailing_state_transition` 추가: 부착 패인에 스로틀 창 안 청크 2개(두 번째가 blocked 프레임)를 통과시키고 전이 도착을 보장. 구조적 RED — pump timeout 분기가 없으면 assert가 타임아웃.

### 3.2 MEDIUM — 백그라운드 패인 무제한 스냅샷 (Security Oracle)
**결함**: `!surface_attached` 무조건 우회는 배경 패인이 청크마다 O(rows×cols) 스냅샷 + 51개 룰 정규식 평가를 실행 (perf 감사 NT-02 CRITICAL 경로 재개방; 초당 ~2,500청크 시 다중 코어 포화 가능).

**수정**:
- 무조건 우회를 이중 간격으로 교체: `AGENT_DETECT_INTERVAL_ATTACHED = 50ms` / `AGENT_DETECT_INTERVAL_DETACHED = 250ms` (배경 패인은 프레임이 아니라 상태 전이만 필요).
- `detach_session`에서 `last_agent_detect_at = None` 리셋 → 탈부착 직후 첫 청크가 즉시 감지(기존 `detached_session_still_reports_agent_state_transitions` 테스트가 결정론적으로 유지됨).

### 3.3 품질 지적 반영
- `BOTTOM_LOCK_TOLERANCE_ROWS` 상수 추출 및 양쪽 리사이즈 블록에 적용 (매직 넘버 `2` 제거).
- 하단 고정 테스트를 3형태로 확장: 결합 성장(80x24→120x30), 가로 전용 성장(→160x30, 가로 분할 페인 닫기 시나리오), 세로 축소(→160x12, max_offset 반대 방향 변화).
- 중복 제안(리사이즈 로직 헬퍼 추출)은 기각 — 기존 중복의 확장이고 두 호출부가 대여 관계로 헬퍼 시그니처가 깔끔하지 않아 smallest-change 원칙 우선.

---

## 4. Context Miner 결과 (GAP 0건)

surface_host.rs의 모든 그리드 변경 경로 분류:
- **COVERED**: flat feed(611), segmented feed(617/620), prepare_session_layout 리사이즈(858-878), reattach 리사이즈(969-989), 기존 세션 재부착(1040-1070), 신규 세션 재생(1089-1094), Lag 복구(1193-1215), Gap 복구(1232-1235).
- **NO-ACTION**: 일반 Output feed(1148, 라이브 출력 경로 — 강제 하단 스크롤 시 의도적 스크롤백 열람이 깨지므로 Ghostty 기본 동작에 위임), 명시적 유저 스크롤 IPC, macOS 휠 모니터, 원격 미러(독립 터미널), input.rs의 dead-code 격리 상태.
- UI 동기화: `dispatchBounds` 성공 시 `refreshScrollbar()` 호출로 React 스크롤바가 네이티브 오프셋에 재동기화됨 확인 (NativeTerminalPane.tsx:1473-1495). 기존 UI 테스트가 썸 10%→20% 갱신을 이미 잠금.
- 기존 컨트랙트 테스트 중 구동작을 고정한 것 없음.

---

## 5. Goal Oracle 추가 확인 사항
- 페인 닫기 시 살아남은 패인은 리마운트되지 않음(TerminalPane에 key 부재 → 순수 DOM 지오메트리 변경) → 리사이즈 경로와 동일 코드 커버.
- `resized == false`인 set_bounds(백그라운드 재생 후)는 뷰포트를 건드리지 않는 것이 옳음 — 유저가 만든 `.pin`은 보존되어야 하고, 재생 경로는 자체적으로 Bottom을 이미 수행.
- 스크롤백 ≤ 2행에서 상단까지 스크롤한 유저도 리사이즈 시 하단으로 스냅됨(공차 2행의 경계) — 최대 2행 변위로 의도된 트레이드오프, 문서화됨.
- Lagged 치수 복원(Bug B) 외 추가 후속 없음.

---

## 6. 검증 증거 (본 세션 실행)

- `cargo test --lib native_terminal::surface_host` → **22 passed; 0 failed** (trailing + Lagged 치수 복원 테스트 포함)
- `cargo test --lib native_terminal` → **99 passed; 0 failed**
- `cargo check` → exit 0, surface_host.rs 경고 0건
- Goal/Security Oracle이 각자 머신에서 독립 실행하여 통과 재현

### QA Executor (omo-senpi-qa-executor, 독립 변이 검증) — **PASS**
샌드박스 트리(`/tmp/qa-st_01a06cd6-tree`, 바이트 동일 사본 + ghostty/ui/dist 심볼릭 링크)에서 변이 테스트 수행, 리포지토리 무변경(md5/diff 확인):
- 필수 명령 3종 모두 exit 0 재현 (21/21, 98/98, check 경고 0)
- trailing 테스트는 trailing 감지 제거 변이에서 실제 실패 확인 (RED 유효: `left: ["working"]` vs `right: ["working", "blocked"]`)
- **F-1**: 리사이즈 경로 `else { Bottom }` 분기는 Ghostty `resize()`가 `.active` 뷰포트를 자체 재앵커링하기 때문에 관측 불가(분기 삭제 변이에도 22/22 그린) — 방어적 코드로 유지, 문서화로 처리
- **F-2**: Lagged 치수 복원은 실제 하중 담당(삭제 시 그리드가 (40,10)에 좌초, 전체 스위트는 그린) → **커밋된 회귀 테스트 `lagged_recovery_restores_grid_to_pane_layout_dimensions` 추가로 잠금 완료**
- 이중 간격 스로틀 + detach 타이머 리셋이 1시간 굶주린 간격 극한 조건에서도 제약 4를 유지함 확인

---

## 7. 알려진 후속 과제 (차단 아님)
1. 리사이즈 로직 중복(`prepare_session_layout` vs `reattach_existing_session_with_bounds`) — 헬퍼 추출은 품질 오라클도 필수로 보지 않음. 다음 대규모 작업 시 정리 권장.
2. Lagged 치수 복원의 `derived_cell_metrics()`는 스케일 팩터 미반영 (attach 분기와 동일 패턴) — HiDPI에서 셀 크기 미세 불일치 가능. 후속 확인.
3. 뮤텍스 홀딩 중 FFI resize(전체 스크롤백 리플로우)는 기존 구조 — 단일 전역 세션 뮤텍스 경합은 별도 성능 과제.
4. 리사이즈 경로 `else { Bottom }` 분기는 현재 Ghostty 시맨틱에서 관측 불가(QA F-1: resize()가 자체 재앵커링) — 방어적 유지. 향후 ghostty 핀업 시 이 분기가 하중을 받게 되면 그때 관측 가능한 테스트를 추가.
