# 네이티브 터미널 attach 리플레이 그리드 파손 — 원인 진단 (2026-08-31)

## 증상
앱 재시작 직후 에이전트 세션 패네의 상단 행들이 글자 단편으로 흩어짐
("Thinking…", "128×128" 문장의 파편들이 여러 행에 scatter). 화면 하단의
실시간 출력(같은 문장)은 정상. 글리프·색상은 셀 단위로 온전하고 위치만 틀림
→ 렌더/폰트 문제가 아니라 **그리드 셀 좌표 자체가 잘못 기록된 상태**.

## 근본 원인: 80×24 고정 크기로 히스토리 리플레이 후 나중에 resize

attach가 패네 측정 전에 먼저 호출되면(bounds 없이) 이 경로를 탄다:

1. `cmd_native_terminal_attach` — UI가 `bounds`/`scaleFactor` 없이 호출하면
   `logical_bounds = None` (`src-tauri/src/ipc/native_terminal.rs` ~L427).
2. `attach_daemon_attachment_with_bounds` — bounds가 None이라
   `prepare_session_layout`(올바른 크기로 세션 선생성)을 건너뜀.
3. `attach_daemon_attachment` (`surface_host.rs` ~L829) fresh 분기:
   `let initial_dims = (80, 24);` → `NativeTerminal::new(80, 24)` →
   **`terminal.feed(&attachment.history)` — 링 버퍼 전체를 80컬 그리드에 파싱.**
   `DaemonAttachment`에는 원본 패네의 cols/rows가 없음 (`daemon/client.rs` L255).
4. 히스토리 바이트는 원래 패네 폭(예: ~190컬)에서 에이전트 TUI가 절대 커서
   주소(CUP/CHA/EL)를 섞어 쓴 것. 80컬 격자에서 파싱되면 커서 주소 clamp/오기록,
   wrap 위치 변화 → 셀이 엉뚱한 좌표에 기록됨.
5. 이후 `ensure_surface_attached`가 실제 크기로 resize(reflow)하지만 — reflow는
   soft-wrap만 재배치할 뿐 이미 잘못된 좌표에 기록된 셀은 복구 못 함.
6. 라이브 스트림은 올바른 폭으로 재개 → 하단만 정상. 에이전트는 스크롤백 위쪽을
   다시 그리지 않으므로 파손 행이 그대로 남음.

## 트리거 조건
- 앱 재시작(데몬 생존) 후 패네 복원 직렬에 측정 전 attach가 끼는 순간
  (`measureGeometry()`가 null을 반환 → bounds 생략, `NativeTerminalPane.tsx` L459~480).
- 히든/제로 사이즈 패네의 attach, 렌더러 evict 후 재attach.
- intermittent인 이유: bounds가 측분된 뒤 attach되면 `prepare_session_layout`이
  올바른 크기의 vt를 미리 만들어 히스토리를 올바른 폭에 feed → 정상.

## 스크린샷과의 대응
- 상단 파손 행 = 80컬로 잘못 파싱된 리플레이(재시작 22:16 이전 링 버퍼 내용).
- "e1 2" / "8x" 단편 = "…e 128×…" 문장이 80컵 경계에서 잘려 재배치된 흔적.
- 하단 정상 라인 = 재시작 후 라이브 스트림(올바른 폭).

## 배제된 후보 (실측)
- **로케일(C locale) 폭 불일치**: 이번 케이스 아님. 데몬 PID 99759(22:11, 디버그
  번들, 터미널에서 기동)의 env에 `LANG=en_US.UTF-8` 존재, PTY 자식 `zsh -l`들이
  상속. (미커밋 pty.rs LANG 주입 수정은 Dock 런치d 무환경 케이스용 — 별개 버그.)
- **오늘의 미커밋 수정들**: preedit 스냅샷 오버레이(surface_host.rs), CoreText
  스무딩 플래그(coretext_raster.rs), mouse serde(camelCase) — 전부 스냅샷 덮어쓰기/
  안티에일리어싱/입력 직렬화만 변경, 그리드 좌표를 건드릴 수 없음.
- **폰트/레스터**: 글리프·색이 온전하고 위치만 틀림 = 셀 추출/렌더가 아니라
  feed 시점(파서 폭) 문제의 전형.

## 참고: 라이브 프로세스 상태 (진단 시점 22:4x)
- 릴리즈 GUI PID 22277 (22:16, /Applications/Ferryx.app, 바이너리 22:08 빌드).
- 데몬 PID 99759 (22:11 기동, debug 번들) — **바이너리는 22:15에 재빌드됐지만
  실행 중인 프로세스는 22:11판(구 바이너리)**. pty.rs LANG 수정(21:40)을
  테스트하려면 데몬 완전 재시작 필요하나, env에 LANG이 이미 있어 이 세션엔 무관.

## 확인 방법 (재현 검증)
1. 빠른 확인: 폭 넓은 패네에서 에이전트 대화 충분히 쌓은 뒤 앱 재시작 반복 —
   파손이 간헐적으로 재현되면 본 진단 확정.
2. 결정적 계측: `cmd_native_terminal_attach` 입구에서 `bounds == None`으로
   attach가 들어오는 순간을 switchDebug/트레이스로 기록 → 파손 직전 실행과 대조.
3. 유닛 테스트(RED): 80×24 vt에 절대커서 주소 바이트 feed 후 실제 폭으로 resize →
   셀 좌표 어긋남 단정 테스트.

## 수정 구현 완료 (2026-08-31 심야, A안)
- `daemon/protocol.rs`: `AttachOk`에 `ptyCols`/`ptyRows` 추가(`#[serde(default]` — 구버전 데몬 JSON skew 허용) + serde 왕복 테스트.
- `daemon/server.rs`: attach 응답에 `get_session().get_size()` 결과 포함(세션 부재 시 None).
- `daemon/client.rs`: `DaemonAttachment`에 `pty_cols`/`pty_rows` 전달.
- `native_terminal/surface_host.rs` `attach_daemon_attachment`:
  - fresh 분기: `NativeTerminal::new(pty_cols.unwrap_or(80), pty_rows.unwrap_or(24))` — 80×24 강제 제거.
  - 기존 세션 분기: reset+feed 전에 PTY 크기로 그리드 정렬(producer 크기로 파싱), feed 후 `session.layout` 크기로 복원 → bounds≠PTY 케이스(탭 전환 후 재attach 등)까지 포함 수정.
- 회귀 테스트 `attach_replays_history_at_daemon_pty_size`: pty 120×30 + `\x1b[100GX` 히스토리 → dimensions (120,30) + X가 col 99에 기록 단정. 수정 전 RED `(80,24) != (120,30)` 확인 후 GREEN.
- 검증: `cargo test --lib native_terminal` 81 passed, `--lib daemon` 68 passed, surface_host/input_boundary contract 17 passed, `cargo check` 경고 증가 0.
## 구조적 해결 구현 완료 (2026-08-31 심야): 리사이즈 원장 + 분할 리플레이
바이트 스트림이 폭 변경을 자기기술하지 못하는 설계 결함을 제거했다. 이제 세션 중간에
폭이 몇 번 바뀌었어도 리플레이가 당시 라이브 그리드와 시간충실적으로 동일해진다.

- `terminal/output_hub.rs`: `ResizePoint { sequence, cols, rows }` 원장(세션당, 상한 4096)이
  출력과 같은 시퀀스 카운터를 공유(`BoundedBuffer::allocate_sequence`). spawn 시
  `record_initial_size`, 모든 리사이즈는 `record_resize`로 마커 기록.
  `snapshot_after_segmented`가 리사이즈 경계마다 `HistorySegment { cols, rows, bytes }`로
  분할 — 평문 `history`는 구버전 호환용으로 그대로 병행(바이트 동일). 리사이즈 후 출력이
  없으면 빈 바이트 트레일링 세그먼트로 최종 폭 전달.
- `TerminalService::resize`가 마커 기록의 단일 관문: 데몬 요청 arm뿐 아니라 원격 웹
  게이트웨이의 3곳 resize 호출(`remote/server.rs`)까지 전부 원장에 기록됨.
- `daemon/protocol.rs`: `HistorySegmentWire`(base64) + `AttachOk.historySegments`,
  `Lagged.segments` — 모두 `#[serde(default)]`로 구버전 daemon/client skew 허용.
- `native_terminal/surface_host.rs`: `feed_attachment_history` — 세그먼트가 있으면 구간별로
  폭을 맞추며 feed, 없으면 기존 평문 경로. attach(fresh/existing)와 Lagged 복구 전부 적용.
- 테스트: 허브 3종(경계 분할/eviction 후 시작 폭/트레일링), 프로토콜 serde 왕복, 그리고
  surface_host 회귀 `attach_replays_segmented_history_across_resizes`(80폭 구간 후 120폭
  리사이즈 → 절대커서 바이트가 올바른 셀에 기록). RED→GREEN 확인. 검증: lib output_hub 11,
  native_terminal 86, daemon 69, daemon_persistence_contract 9(--test-threads=1 규약 준수),
  cargo check 새 경고 0.
- 남는 본질 한계(문서화됨): 리사이즈 경계의 소량 in-flight 바이트는 SIGWINCH 전달 시점
  탓에 어느 쪽 시퀀스에도 걸릴 수 있으나, 라이브 패네가 경험한 것과 동일한 경계라 재현
  정확도에 영향 없음. 셀 그리드 권위 상태(데몬이 vt를 들고 셀 스냅샷 배포)는 멀티 소비자
  요구가 생길 때의 별도 아키텍처 과제.
- 적용 조건: 데몬 완전 재시작(신규 바이너리) 필요. 프로토콜 필드는 상하위 호환이라
  구버전 GUI/daemon 혼용 시에도 기존 경로로 안전하게 동작.

## 수정 방향 (미구현, 원인 확정 후 선택 필요)
- A(권장): 데몬이 `DaemonAttachment`에 마지막 PTY winsize(cols/rows)를 실어
  보내고, GUI는 그 크기로 vt를 생성한 뒤 히스토리를 feed. 이후 bounds가 다르면
  resize+PTY 통지. 히든 패네 케이스까지 결정적으로 해결. → **구현 완료(상단)**
- B: UI에서 첫 attach를 실제 측정 이후로 지연/재시도(bounds 보장).
- C: bounds 없이 attach되어 80×24로 리플레이한 세션에 플래그를 달고, 최초
  bounds 확정 시 reset + 재리플레이(afterSequence=0)로 자가 복구.
