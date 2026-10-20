# Ferryx 인앱 Supervisor 에이전트 — 구조적 가능성 + 유사 사례 조사 (2026-10-16)

## 결론 (먼저)

**구조적으로 가능하고, Ferryx는 경쟁 제품 대비 부품이 가장 많이 갖춰진 상태다.** 필요한 4개 축 — (1) 여러 세션 출력 관찰, (2) 화면 상태(프롬프트) 해석, (3) 사람처럼 입력 주입, (4) 단일 컨트롤러 보장/권한 — 이 이미 데몬/원격 게이트웨이 코드에 각각 존재한다. 빠진 것은 "이것들을 묶어서 LLM이 호출할 수 있는 하나의 도구 표면(MCP/CLI)"과 "GUI가 없어도 도는 데몬측 상태 감지 루프"다. 즉 새 터미널 기술이 아니라 **배선(integration) + 정책(safety) 문제**다.

업계도 같은 방향으로 수렴 중이다: herdr("agents drive herdr through the cli and socket api ... wait until another agent is genuinely blocked"), cmux("CLI and socket API to ... send keystrokes"), aoaoe("It polls your AoE sessions, reads agent output, decides when to act"), amux(steering + watchdog), Claude Code agent teams, Warp orchestrated runs. **"사람 대신 TUI를 보고 키를 쳐주는 LLM 감독자"를 제품 내장형으로 제대로(권한·감사·안전 포함) 낸 곳은 아직 없다** — 대부분 dry-run 기본값의 실험 단계(aoaoe)거나, 에이전트가 서로를 부르는 소켓 API만 제공(herdr/cmux)한다.

---

## 1. Ferryx에 이미 있는 부품 (소스 근거)

| 축 | 이미 있는 것 | 근거 |
|---|---|---|
| 출력 관찰 | 세션별 512 KiB 순번(sequence) 링버퍼 + broadcast, 뒤처지면 `ReplayGap` | `src-tauri/src/terminal/output_hub.rs:8,13,66` |
| 추가 구독자 | 데몬 `Attach { session_id, after_sequence }` — GUI 첨부와 독립된 소비자 | `src-tauri/src/daemon/protocol.rs:578` |
| 입력 주입 | `Write` / 세대(generation) 검증 `RemoteWrite` | `protocol.rs:505,550`, `daemon/server.rs:3728,4007` |
| 헤드리스 화면 재구성 | `RemoteTerminalMirror` = libghostty-vt `NativeTerminal`을 GUI 없이 돌려 grid 생성 (원격 서버는 데몬 안에서 기동) | `src-tauri/src/remote/mirror.rs:14-33`, `daemon/server.rs:5281` |
| 프롬프트 해석 | 화면 → `Question/Approval/Plan/Menu` 프롬프트 카드 + 답변 키 계획 (claude/codex/omo/omp/pi 어댑터 + fixture) | `remote/reference_chat/types.rs:543`, `prompts.rs:358-383`, `prompt_claude.rs` 등 |
| 사람처럼 입력 | 대상별 직렬 큐, bracketed paste 감지, 텍스트와 Enter 분리 전송, Stop=Escape, 확인 불가 시 `OutcomeUnknown`(자동 재전송 금지) — herdr-web-ui 포팅 | `remote/reference_chat/input.rs:1-60` |
| HTTP 표면 | `/api/v1/reference-chat/{id}/screen|prompt|submit|answer|stop|history` | `remote/server.rs:5264-5273` |
| 상태 신호 | 훅 기반 확정 상태(omo/pi/omp 확장 → 상태 소켓) + 화면 규칙 엔진 `Working/Blocked/Idle` (11개 에이전트 manifest) | `daemon/agent_extension.rs`, `daemon/agent_state.rs`, `agent_detect/engine.rs:10-14`, `agent_detect/manifests/*.toml` |
| **에이전트용 control API 골격** | `create/start/prompt/stop` mutate, `read(limit)`, `wait(target, until, timeout≤300s)`, 인벤토리 이벤트 replay, **단일 컨트롤러 lease** (`ControlConflict`), 64KiB 프롬프트 상한, owner epoch 검증 | `src-tauri/src/ferryx_scope/control/{mod,service,local,lease}.rs` (`service.rs:42,96`, `local.rs:21-31`, `lease.rs:6-10`) |

> ⚠ `ferryx_scope/control`은 **현재 어떤 `mod`/`#[path]`로도 `lib.rs`에 연결되어 있지 않다** (`lib.rs:12-39` 모듈 목록, `ferryx_scope` 참조는 history/ssh 경로뿐). 즉 설계·테스트는 있으나 빌드에 포함되지 않은 휴면 코드다. Supervisor의 가장 자연스러운 출발점.

## 2. 빠진 것 (Gap)

1. **LLM 도구 표면 부재** — MCP 서버 없음, `ferryx send/read/wait` 같은 CLI 없음 (`src-tauri/src/bin/cli.rs`는 account/browser/pair/remote/--daemon만). 외부 에이전트는 UDS JSON이나 원격 HTTP에 직접 붙어야 한다.
2. **화면 기반 상태 감지가 GUI 전용** — `agent_detect::default_engine()`의 실사용 호출은 `native_terminal/surface_host.rs:2140` 단 한 곳(보이는 pane 렌더 루프). 백그라운드 탭·GUI 종료 상태에서는 훅 없는 에이전트(codex/claude 등)의 Blocked/Idle 전이를 데몬이 모른다. → 데몬에 `RemoteTerminalMirror` 기반 헤드리스 감지 루프가 필요.
3. **다중 구독 비용** — Attach가 세션당 1개. 데몬 내부(in-process)에서 `TerminalOutputHub` 직접 구독하면 해결되지만 외부 프로세스형 supervisor면 N개 소켓.
4. **정책 계층 부재** — 어떤 프롬프트를 자동 승인해도 되는지(allow-list), 감사 로그, 사람 개입 에스컬레이션, 비용/루프 상한. control의 lease는 "동시에 두 컨트롤러 금지"까지만 한다.
5. **(UX) 사람과의 공존** — 사람이 같은 pane에 타이핑 중일 때 supervisor 입력 차단(lease를 데스크탑 포커스/키 입력과 연동), 원격 active-desktop lock과의 관계 정의.

## 3. 제안 아키텍처

```
                ┌────────────── Ferryx daemon (PTY 소유자) ──────────────┐
 PTY sessions → │ TerminalOutputHub ─┬─> GUI attach (기존)                │
                │                    └─> HeadlessObserver (신규)          │
                │                         RemoteTerminalMirror per session│
                │                         agent_detect + detect_reference_│
                │                         prompt → AttentionEvent         │
                │ AgentStateHub (훅) ───────────┘                         │
                │ ControlService (ferryx_scope/control 활성화)            │
                │   list / read(screen|history) / wait(until) / prompt /  │
                │   answer(promptId, option) / stop  + Lease + AuditLog   │
                └──────────────┬──────────────────────────────────────────┘
                               │ MCP (stdio/UDS) + ferryx CLI
                    ┌──────────┴──────────┐
                    │ Supervisor agent     │  (Claude Code/Codex/omo 등을
                    │ 이벤트 구동 루프      │   하나의 Ferryx 세션으로 띄움)
                    └─────────────────────┘
```

핵심 설계 원칙:
- **Supervisor 자체도 Ferryx 세션 하나**로 띄운다(LLM 런타임 내장 X). 모델/비용은 사용자의 기존 에이전트가 부담하고 Ferryx는 도구(MCP)만 제공 → 유지비 최소, 사람이 그 pane을 열어 감독자를 감독 가능.
- **폴링이 아니라 이벤트 구동**: `wait(until: blocked|idle|prompt)`로 깨어남 (control의 `wait`가 이미 이 형태). aoaoe식 주기 poll+LLM 호출은 비용 폭증.
- **원시 키 입력보다 구조화 답변 우선**: `answer(promptId, optionId)`는 `reference_answer_keys`로 화면 검증 후 키를 계획 — 화면이 바뀌었으면 거부. 임의 `write`는 별도 고권한 도구.
- **안전**: (a) 프롬프트 종류별 정책(파일 편집 승인 OK, `rm`/push/네트워크 승인은 사람 에스컬레이션), (b) 감독 대상 출력은 신뢰 불가 데이터로 태깅(간접 프롬프트 인젝션), (c) 모든 입력 감사 로그 + UI에 "supervisor가 입력함" 표시, (d) 사람 키 입력 시 lease 즉시 회수, (e) 세션당/시간당 입력 상한.

## 4. 단계별 로드맵 (추정 난이도)

| 단계 | 내용 | 난이도 |
|---|---|---|
| P0 | `ferryx_scope/control` 모듈 연결 + 로컬 UDS/CLI(`ferryx agents list|read|wait|send|answer`) | 낮음~중 (골격·테스트 존재) |
| P1 | 데몬 HeadlessObserver: 세션별 mirror + agent_detect/prompt 감지 → AttentionEvent (GUI 없이) | 중 (성능: 세션 N개 VT 파싱, 디바운스) |
| P2 | MCP 서버 래퍼 (stdio, 위 도구 노출) + supervisor 프리셋(시스템 프롬프트/정책 파일) | 낮음 |
| P3 | 정책·감사·lease-사람입력 연동·UI 표시(어떤 pane을 누가 조작 중) | 중~높음 (보안 설계) |
| P4 | 원격(paired host/SSH) 세션까지 확장 — control의 host/owner/epoch 모델이 이미 대비 | 중 |

## 5. 유사 사례 (원문 확인한 것 위주)

| 사례 | 감독 방식 | 관찰 | 입력 | 감독자=LLM? | 비고 |
|---|---|---|---|---|---|
| **agent-of-agent-of-empires (aoaoe)** [repo](https://github.com/Talador12/agent-of-agent-of-empires) | AoE 세션을 poll → reason → execute | tmux 출력 캡처 | tmux 키 입력 | **예** (rules / claude-code / opencode reasoner) | "dry-run by default", dry-run도 LLM 토큰 소모. 가장 직접적인 선례 |
| **Tmux-Orchestrator** [repo](https://github.com/Jedward23/Tmux-Orchestrator) | Orchestrator→PM→Engineer 계층 Claude | tmux 창 | tmux send-keys, `schedule_with_note.sh` 자기 체크인 | 예 | 체크인 없으면 멈춤 — 이벤트 신호 부재의 한계 |
| **herdr** [repo](https://github.com/herdrdev/herdr) | 런타임 + 에이전트 간 제어 | pane 상태 working/blocked/idle | CLI·socket API로 pane 생성/프롬프트/대기 | 에이전트가 직접 사용 | Ferryx reference_chat 입력 계층은 herdr-web-ui 포팅 |
| **cmux** [repo](https://github.com/manaflow-ai/cmux) | Ghostty 기반 macOS 터미널 | OSC 9/99/777 + `cmux notify` 훅, 알림 링 | CLI·socket API로 send keystrokes | 외부 에이전트가 사용 | 관찰은 알림 중심, 화면 해석 없음 |
| **amux** [repo](https://github.com/mixpeek/amux) | 대시보드/폰에서 steering | 세션 상태 + provenance ledger | "type into any running session" | 일부(watchdog 자동 compact/재시작/재전송) | 출처(provenance) 기록이 안전장치 |
| **ccmanager** [repo](https://github.com/kbwo/ccmanager) | 다중 Claude Code 관리 | 설정형 상태 감지(busy/waiting/idle) | PTY | **Auto Approval(실험): AI 검증으로 안전 프롬프트 자동 승인** | supervisor의 "자동 승인" 부분만 구현한 예 |
| **Claude Squad** [repo](https://github.com/smtg-ai/claude-squad) | tmux 기반 다중 세션 | tmux | tmux, autoyes | 아니오 | 상태 표시 빈약 |
| **Mato** [repo](https://github.com/mr-kelly/mato) | 다수 백그라운드 세션 | 탭별 live activity 신호 ("tmux is blind") | 키보드 | 아니오 | 관찰 UX 참고 |
| **Claude Code agent teams / hooks** [teams](https://code.claude.com/docs/en/agent-teams) · [hooks](https://code.claude.com/docs/en/hooks) | 리드 세션이 teammate 조정 | Notification 훅: `permission_prompt`, `idle_prompt`, `agent_needs_input`, `agent_completed` 등 | teammate 메시지 | 예 | "significantly more tokens" 경고. 훅 = 가장 신뢰도 높은 상태 신호 |
| **Warp orchestrated runs** [docs](https://docs.warp.dev/platform/orchestration/multi-agent-runs) | parent agent가 child 생성 | 탭 상태 | 자연어 steering, API `parent_run_id` | 예 | cloud child는 부모 인증/크레딧 상속 |
| **OpenHands Agent Canvas** [repo](https://github.com/All-Hands-AI/OpenHands) | 다중 에이전트 control center | ACP 세션 | 세션별 채팅 | 사람 주도 | ACP로 Claude Code/Codex/Gemini 등 통합 |
| **Agent Client Protocol (ACP)** [repo](https://github.com/zed-industries/agent-client-protocol) | 에디터↔에이전트 JSON-RPC | 구조화 세션 업데이트/권한 요청 | prompt 요청 | 호스트 결정 | 스크래핑의 대안 표준 |
| Conductor / Crystal(→Nimbalyst) / Sculptor / Vibe Kanban(종료 예정) / Uzi / claude-flow(Ruflo) | 병렬 워크트리·세션 관리자 | 각자 | 각자 | 대부분 아니오 | 관리 UI는 많지만 LLM 감독자는 드묾 |
| AutoGen GroupChatManager / CrewAI hierarchical | 대화형 매니저 에이전트 | API 메시지 | API | 예 | TUI가 아닌 SDK 수준 — 개념적 선례 |

(Conductor, Sculptor, Cursor/Devin 세부 메커니즘은 이번 세션에서 원문으로 재검증하지 않음.)

## 6. 알려진 실패 모드와 대응

- **TUI에서 idle/입력대기 판별이 불안정** — 업계 공통 난제. ccmanager도 휴리스틱 감지. 대응: 훅/확장 신호 우선 → 화면 규칙은 보조, `gap/truncated`면 답변 거부(Ferryx `is_answerable` 이미 존재).
- **간접 프롬프트 인젝션** — 감독 대상 출력(웹 검색 결과, 파일 내용)이 감독자에게 지시처럼 읽힘 ([Greshake et al., arXiv:2302.12173](https://arxiv.org/abs/2302.12173)). 대응: 출력은 데이터 채널로만 전달, 위험 승인은 사람에게.
- **비용·폭주 루프** — 주기 poll+LLM 판단은 토큰 소모 (aoaoe 문서 자체 경고, Claude teams 토큰 경고). 대응: 이벤트 구동 wait, 규칙 기반 1차 필터(LLM 없이 처리 가능한 "계속할까요? y"류), 세션/시간당 상한.
- **사람과 입력 충돌 / 중복 입력** — 대응: lease + 사람 입력 시 회수, 대상별 직렬 큐, `OutcomeUnknown`이면 재전송 금지(이미 구현된 원칙).
- **세션 유실 위험** — supervisor가 kill/재시작을 하면 안 됨. 데몬이 PTY 소유자라는 Ferryx 불변식 유지: supervisor 도구에 signal/kill 미노출(`input.rs` 원칙과 동일).

## 7. 판단

- 차별화 포인트로 유효하다: 경쟁사는 "에이전트끼리 소켓으로 부르기"(herdr/cmux) 또는 "외부 실험 데몬"(aoaoe)에 머물러 있고, Ferryx는 **헤드리스 VT 미러 + 에이전트별 프롬프트 파서 + 안전한 입력 트랜잭션 + lease 있는 control API**를 이미 갖고 있어 화면 검증형 감독자를 낼 수 있는 몇 안 되는 위치.
- 권장 첫 단계는 P0+P2(휴면 control 모듈 연결 + MCP/CLI 노출) — 사용자가 이미 쓰는 Claude Code/omo 세션을 감독자로 써 바로 검증 가능. P1(헤드리스 감지)이 실제 "사람처럼 감시"의 핵심 비용 항목.

## 근거 수집 방법
- 코드: 이 리포 HEAD `3f3c904e` 기준 직접 grep/read + explore 서브에이전트 코드맵(주장 중 "데몬에 헤드리스 화면 재구성 없음"은 `remote/mirror.rs` 확인으로 **반박/수정**함).
- 사례: librarian 서브에이전트 조사 후, 핵심 README/문서를 직접 fetch해 문구 대조(herdr/cmux/amux 설명은 원문 기준으로 교정, 원문 미확인 주장은 제외).

---

## 8. 종합 결론 — 이미 있는 omon-gateway 기준 (2026-10-16 추가)

대상: `/Users/indo/code/project/omon-gateway` (HEAD `e57b472`). 읽은 파일: `src/sessions/{control,probe,probe.py,watch,external,overview,registry}.rs`, `src/lanes/{runner,settle,signals,kibitzer,guard,state}.rs`, `src/judge/mod.rs`, `docs/AGENTS_DELEGATE.md`, `bot_profiles/blocks/lanes.md`, 커밋 `1bbe7e2` / `245a587`.

### 8.1 핵심 판단

**"여러 세션을 감시하고 사람처럼 입력하는 감독 에이전트"는 이미 omon-gateway에 omo 세션 한정으로 구현되어 있다.** 그러니 Ferryx에 감독자 두뇌를 새로 만들 필요는 없다. 남은 일은 gateway의 감독 루프가 **Ferryx 세션(=omo가 아닌 모든 TUI 에이전트)까지 대상으로 삼게 하는 것**이다. 역할 분담은 다음과 같다:

- **omon-gateway = 두뇌/정책** (목표 주입, 완료 검증, 판단, 오너 에스컬레이션, 비용 상한)
- **Ferryx = 눈/손** (모든 PTY 세션의 화면 관찰, 프롬프트 해석, 안전한 입력 전송, 그리고 사람이 감독 상황을 보는 UI)

### 8.2 gateway에 이미 있는 감독자 부품

| 감독 기능 | gateway 구현 | 근거 |
|---|---|---|
| 세션 발견 (멀티 머신) | 로컬 + SSH 호스트 목록, 호스트가 멈춰도 다른 호스트는 막지 않음, stale 캐시 | `sessions/registry.rs` (`hanging_host_times_out_without_blocking_other_hosts`) |
| **관찰 (구조화)** | 화면 스크래핑이 아니라 omo RPC 소켓(`~/.omo/agent/rpc/tui/*.sock`, `rpc.sock`)의 `get_state`/`get_messages`로 `isStreaming`, `compacting`, `blocking_question`, 대기 질문(`questions`), 마지막 user/assistant를 수집 | `sessions/probe.py:21-200`, `probe.rs:28-48` |
| 상태 분류 | `Working / Question / Blocked / Idle` | `sessions/overview.rs:18-35` (`state_of`) |
| 입력 | `omo thread send <id> <text> --mode auto|steer|follow_up --idempotency-key <uuid>` (실행 중인 턴에는 steer, 끝난 뒤에는 follow-up) | `sessions/control.rs:407-437` |
| 질문 답변 | 스냅샷에서 `request_id`로 대기 질문을 찾은 뒤 `extension_ui_response` 전송 | `control.rs:461+`, `probe.py:236-239` |
| 중단 | `interrupt` (지원하는 세션만, `controls` 광고 확인) | `control.rs:440-459` |
| 감시 알림 | 60초 주기 tick, 질문은 SQLite로 중복 제거(재시작 후에도 1회), 완료는 streaming→idle 전이에만 알림, 터미널에서만 답할 수 있는 질문은 사용자에게 터미널로 안내 | `sessions/watch.rs:15,95-140` |
| **외부 세션 구동 (사람처럼 일 시키기)** | 기존 세션을 lane으로 `adopt` → 목표 + Done 조건 + `[CLAIM_DONE] key | 증거` 규약을 1회 주입 → 이벤트(Notify) 또는 300초 fallback마다 새 assistant 메시지를 cursor로 읽고 → 대기 메시지 전송; 오류는 3회 이상 120초 넘게 이어져야 실패 처리 | `sessions/external.rs:24-30,86-400` (`drive`) |
| 완료 검증 | 주장만으로 완료 처리하지 않음: 명령/파일/URL/PR 신호 검사(`lanes/signals.rs`) + jev diff 판정(`lanes/settle.rs`, 운영 규칙은 brief에서 빼고 deliverable만 채점) + 5분 이의 제기 창 → `verified_done` | `lanes/settle.rs`, `bot_profiles/blocks/lanes.md` "Done rule" |
| 판단 LLM | 소형 모델 판단 + 일일 상한 + 401/429 circuit breaker + 보수적 fallback + 외부 전송 전 경로/토큰 마스킹 | `judge/mod.rs` |
| 오너 의도 감시 | kibitzer: 이벤트로 arm, 30분 간격, 판단 실패 시 재arm | `lanes/kibitzer.rs:1-15` |
| 자원 상한 | load / mem / disk 기준으로 lane 실행 보류(HOLD) | `lanes/guard.rs` |
| 감독자 자신 | Discord의 lead 세션(LLM)이 `omo-gateway lane start/send/object` CLI를 도구로 사용 | `docs/AGENTS_DELEGATE.md`, `bot_profiles/blocks/lanes.md` |

즉 §3에서 "Ferryx에 새로 만들어야 한다"고 적은 정책 계층(완료 검증, 판단 상한, 에스컬레이션, 자원 상한)은 **gateway에 이미 운영 수준으로 있다.**

### 8.3 gateway의 한계 = Ferryx가 채울 자리

1. **omo 계열 세션만 다룬다.** 관찰과 입력이 omo RPC 소켓과 `omo thread` CLI에 묶여 있다(`probe.py` endpoints, `control.rs` send). Claude Code, Codex, OpenCode 등 RPC가 없는 TUI 에이전트는 볼 수도 조작할 수도 없다. 그런데 Ferryx는 바로 그런 에이전트용으로 화면 기반 프롬프트 파서(claude/codex/...)와 `write_input`을 갖고 있다(§1).
2. **폴링 기반이다.** watch는 60초 tick이고, 머신마다 ssh로 python probe를 한 번씩 실행한다(`PROBE_TIMEOUT` 20초). Ferryx 데몬은 PTY 출력을 sequence 단위로 push하므로, 이벤트 구동 `wait(until)`을 줄 수 있다(`ferryx_scope/control/service.rs:96`).
3. **사람용 감독 화면이 Discord뿐이다.** "지금 감독자가 어느 pane에 무엇을 입력했는지"를 데스크탑에서 볼 수 없다. Ferryx가 pane 테두리, 인박스, 감사 표시를 맡을 수 있다.
4. **사람과의 입력 충돌을 막지 않는다.** gateway는 send에 idempotency key만 붙인다. 사람이 같은 TUI에 타이핑 중인지는 모른다. Ferryx의 lease와 포커스 정보가 필요하다.

### 8.4 권장 구조 (omo 중심안 — §9로 대체됨)

```
 Discord / Ferryx UI ──> lead LLM 세션 (gateway)            <- 두뇌: lanes, judge, kibitzer, guard
                              │  omo-gateway lane / sessions CLI
                              ▼
                 SessionControl (gateway, Target = machine:session)
                    ├─ omo backend   : probe.py + omo thread   (기존, 구조화)
                    └─ ferryx backend: ferryx agents list|read|wait|send|answer|interrupt   <- 신규
                                              │ UDS (로컬) / ssh (원격 머신, gateway 패턴 그대로)
                                              ▼
                         Ferryx daemon: ferryx_scope/control (휴면 코드 활성화)
                           + HeadlessObserver (RemoteTerminalMirror + agent_detect + reference prompt)
                           + reference_chat 입력 트랜잭션 + lease (사람 입력 시 회수)
```

설계 결정:
1. **계약은 gateway의 `SessionControl`을 기준으로 맞춘다.** Ferryx CLI가 같은 명령 형태(list/read --cursor/send --mode --idempotency-key/answer request_id/interrupt), 같은 JSON 형태(`LiveSession`: streaming, blocking_question, questions[], last_assistant), 같은 exit code(`3` not running, `4` refused, `5` other; `control.rs:94-106`)를 쓰게 한다. 그러면 gateway 쪽 변경은 `CommandRunner` 백엔드를 하나 추가하는 것으로 끝나고, `watch`, `external drive`, `[CLAIM_DONE]`, 완료 검증 전체가 수정 없이 Ferryx 세션에 적용된다.
2. **신호는 구조화된 것을 우선한다.** Ferryx 세션 안에서 도는 에이전트가 omo/pi/omp라면 Ferryx도 화면 대신 omo RPC나 훅 상태를 쓴다(gateway가 증명한 방식). 화면 파서는 RPC가 없는 claude/codex 등에만 쓰고, `gap/truncated`면 답변을 거부한다.
3. **"Ferryx 앱 안의 감독 에이전트"는 gateway lead 세션을 Ferryx pane으로 띄우는 것으로 충족한다.** 새 LLM 런타임을 넣지 않는다. Ferryx에는 "감독자 pane" 프리셋과, 감독자가 입력한 pane에 붙는 표식, lane 상태 뷰만 추가한다.
4. **안전 규칙은 gateway의 규칙을 그대로 쓴다.** 완료는 `verified_done`만 인정하고, judge 상한과 circuit breaker, 외부 전송 마스킹, 자원 HOLD를 우회하지 않는다. Ferryx는 그 아래 층으로 lease(사람 우선)와 kill/signal 미노출을 더한다.

### 8.5 수정된 로드맵

| 단계 | 작업 | 위치 | 효과 |
|---|---|---|---|
| S1 | `ferryx_scope/control` 연결 + `ferryx agents list/read/send/answer/interrupt --json` (gateway 계약 호환) | Ferryx | gateway가 Ferryx 세션을 발견·읽기·입력할 수 있게 됨 |
| S2 | gateway `registry`/`control`에 ferryx 백엔드 추가 (로컬 + ssh, 기존 `transport_argv` 재사용) | gateway | `/sessions`, watch, external lane이 claude/codex TUI 세션에도 동작 |
| S3 | Ferryx 데몬 HeadlessObserver (GUI 없이 Blocked/Question 감지) + `wait` → gateway watch를 폴링에서 이벤트로 | Ferryx (+gateway 수신) | 60초 지연이 사라지고 백그라운드 탭도 감지 |
| S4 | lease: 사람이 그 pane에 입력하면 감독자 입력을 `ControlConflict`로 거부하고 gateway가 HOLD 처리 | Ferryx | 입력 충돌 방지 |
| S5 | Ferryx UI: 감독자 pane 프리셋, 조작된 pane 표식/감사 로그, lane 상태 패널 | Ferryx | 사람이 데스크탑에서 감독자를 감독 |

**가장 먼저 할 일은 S1+S2다.** 기존 감독 로직을 하나도 다시 쓰지 않고 감독 대상만 "omo 세션"에서 "Ferryx의 모든 에이전트 세션"으로 넓힌다. 그것만으로 원래 문제였던 "여러 세션을 번갈아 확인하고 입력 넣기"의 대부분이 gateway의 `/sessions`, 질문 알림, lane 구동으로 해결된다. 남는 "사람처럼 실시간으로 지켜보기"는 S3가 맡는다.

### 8.6 확인하지 못한 것
- gateway 바이너리의 실제 동작이나 실행 중인 서비스는 실행해 보지 않았다(소스만 읽음). `docs/AGENTS_DELEGATE.md` 자체도 "Exact CLI help acceptance remains pending"이라고 적고 있다.
- `omo thread send`의 steer/follow_up 의미가 Ferryx TUI 입력(텍스트 + Enter)과 1:1로 대응하는지는 정의가 필요하다. TUI에는 "실행 중인 턴에 끼워 넣기"가 에이전트마다 다르다(예: Claude Code는 실행 중 입력이 대기열에 들어감). S1 계약에서 `--mode`별 동작을 에이전트별로 명시해야 한다.

---

## 9. 최종 결론 — 모든 에이전트 지원 기준 (§8.4–8.5 대체)

전제(사용자 확정): omo는 1급 지원 대상이지만 **목표는 가능한 모든 에이전트**(codex, claude code, omp, cline, opencode, cursor, aider 등)다. 그래서 §8의 "gateway의 omo 계약(`omo thread send --mode steer|follow_up`, `extension_ui_response`)을 범용 계약으로 쓰자"는 제안은 **철회**한다. 그렇게 하면 omo 고유 개념이 다른 모든 에이전트의 계약에 들어간다.

### 9.1 판단

**Ferryx가 에이전트 중립적인 "세션 제어 계약 + provider 어댑터 계층"을 갖고, omon-gateway는 그 계약을 쓰는 두뇌(소비자) 중 하나가 된다.** omo의 1급 지원은 "계약 자체가 omo 모양"이라서가 아니라 "omo 어댑터가 가장 높은 충실도로 구현된다"는 뜻이 된다.

이 구조가 Ferryx에서만 가능한 이유는 **Ferryx 데몬이 모든 세션의 PTY를 소유하기 때문이다.** gateway는 RPC 소켓이 있는 에이전트만 볼 수 있다(`sessions/probe.py` endpoints가 `~/.omo/agent/rpc` 전용). Ferryx는 RPC가 없는 TUI도 화면과 PTY 입력이라는 공통 바닥을 이미 갖고 있다. 그러니 "모든 에이전트"의 최소 지원은 첫날부터 가능하고, 에이전트별 네이티브 어댑터는 그 위에 충실도를 올리는 증분이다.

### 9.2 3단 충실도 모델 (capability tier)

| Tier | 관찰 | 입력/답변 | 대상 (근거: `.omo/reports/herdr-mobile-source-audit-20261005/all-agent-provider-matrix-20261006.md`) |
|---|---|---|---|
| **N: Native 소유자 제어** | 구조화된 이벤트/히스토리 | 프로토콜 prompt / interrupt / approval / question 응답 | omo·pi·omp(RPC, Ferryx 확장 `daemon/agent_extension.rs`), codex(app-server turn/interrupt), opencode·crush·mimo(HTTP/SSE), cursor·cline·copilot·goose·grok·kimi(ACP), claude(Agent SDK, 또는 훅), gjc(SDK broker), prime-agent(daemon), openclaw(gateway) |
| **B: Bridge** | 확장/훅이 보내는 의미 이벤트 | 버전 고정 브리지가 원래 입력 경로로 전달 | 이미 실행 중인 claude TUI(훅 + 소유 브리지), aider(`InputOutput` 호스트 브리지), droid, devin, hermes, codebuff, rovo, antigravity |
| **S: Screen 바닥 (모든 TUI)** | 헤드리스 VT 미러 + 규칙 엔진(`agent_detect/manifests/*.toml` 11종) + 화면 프롬프트 파서(`reference_chat/prompt_{claude,codex,omo,omp,pi}.rs`) | `write_input` + `reference_chat/input.rs` 입력 트랜잭션(bracketed paste, 텍스트와 Enter 분리, `OutcomeUnknown`) | 등록되지 않은 커스텀 명령까지 포함한 모든 PTY 세션 |

원칙은 기존 공유 계약 문서(`all-agent-shared-contracts-20261006.md`)를 그대로 따른다:
- **live owner ≠ saved-state resume.** 저장된 세션 ID로 새 프로세스를 띄우는 것은 attach가 아니다. 감독자가 경쟁 writer를 만들어 대화를 fork하면 안 된다(omp는 경쟁 writer가 생기면 sibling ID로 분기한다).
- **누락된 어댑터를 not-applicable로 바꾸지 않는다.** 그 에이전트는 S tier로 동작하고, manifest가 "native / bridge 필요 / screen only"를 구분해 표시한다.
- **auto-approve 플래그로 승인 기능을 흉내 내지 않는다** (`--yes-always`, Claude Squad의 autoyes 같은 방식 금지).

### 9.3 에이전트 중립 계약 (초안)

```
AgentSession {
  target: { host, owner, epoch, backendSessionId }      // ferryx_scope/control TargetRef 그대로
  agent: "claude" | "codex" | "omo" | ... | "unknown"   // 실행 파일/패키지 식별, title regex 아님
  tier: "native" | "bridge" | "screen"
  state: working | question | blocked | idle | exited   // gateway overview.rs와 같은 어휘
  stateSource: provider | hook | screen                 // 판정 근거 (신뢰도)
  pending: [{ id, kind: question|approval|plan|menu, options[], source, answerable }]
  transcript: { cursor, gap }                           // native=구조화 메시지, screen=화면 tail
  capabilities: {
    sendIdle, sendWhileRunning: "queue"|"steer"|"reject",
    interrupt: "native"|"escape-key"|none,
    answer: "native"|"screen-validated"|none
  }
}
ops: list · read(cursor) · wait(until, timeout) · send(text, idempotencyKey) · answer(pendingId, option) · interrupt
```

- `send`의 실행 중 동작은 **에이전트마다 다르다는 사실을 계약에 드러낸다** (`sendWhileRunning`). omo의 steer와 follow_up은 이 필드 값 중 하나일 뿐이다.
- `answer`는 screen tier에서 반드시 **같은 화면에서 검증한 뒤 키를 계획**한다(`reference_answer_keys`, `is_answerable`). 화면이 바뀌었거나 gap/truncated면 거부한다.
- 판정 근거(`stateSource`)를 노출하므로 감독자 정책이 "screen 근거만으로는 자동 승인하지 않는다" 같은 규칙을 걸 수 있다.
- 외부 표면은 `ferryx agents ... --json` CLI와 MCP(stdio) 두 개다. 같은 계약을 쓴다.

### 9.4 omon-gateway의 위치

- gateway는 이 계약의 **첫 번째 소비자**다. lanes(목표 주입, `[CLAIM_DONE]`), settle·judge(완료 검증), kibitzer, guard, Discord 알림이 이제 Ferryx를 통해 모든 에이전트에 적용된다. gateway의 `SessionControl`은 `omo thread` 대신 `ferryx agents`를 부르는 백엔드를 갖는다(`CommandRunner` 추상화와 ssh `transport_argv` 재사용).
- gateway의 기존 omo 직결 경로(`probe.py`)는 Ferryx 밖에서 실행된 omo 세션용으로 남겨도 된다. 장기적으로는 Ferryx의 omo 어댑터가 같은 RPC(`get_state`/`get_messages`/`extension_ui_response`)를 쓰게 해서 중복을 없앤다. **gateway가 omo에서 증명한 방식이 Ferryx omo 어댑터의 기준 구현이 된다.**
- 정책은 계약의 `tier`/`stateSource`를 보고 결정한다. 예: native question은 judge가 답할 수 있다. screen-only 승인 프롬프트는 오너에게 에스컬레이션한다(gateway watch의 "터미널에서 답해야 하는 질문" 처리와 같은 성격).

### 9.5 로드맵 (최종)

| 단계 | 작업 | 결과 |
|---|---|---|
| A1 | 계약 + `ferryx_scope/control` 연결 + `ferryx agents` CLI/MCP, **S tier를 모든 세션에 적용**: 데몬 HeadlessObserver(세션별 `RemoteTerminalMirror` + `agent_detect` 11종 + 화면 프롬프트 파서 5종), 매칭되지 않는 에이전트는 `unknown`(출력 정지 시간 기반 idle만) | 첫날부터 모든 에이전트 list/read/wait/send, 5종은 화면 기반 answer |
| A2 | gateway에 ferryx 백엔드 추가 | gateway 감독 기능 전체가 모든 에이전트로 확장 |
| A3 | N tier 1차: **omo·pi·omp(이미 있는 Ferryx 확장 소켓 + gateway 방식) → codex app-server → claude(Notification 훅: `permission_prompt`, `idle_prompt`, `agent_needs_input`)** | 사용량 상위 에이전트에서 상태가 확정적이고 답변이 구조화됨 |
| A4 | N tier 2차: **ACP 클라이언트 하나**로 cursor·cline·copilot·goose·grok·kimi를 한 번에 처리(capability 협상은 에이전트별 증거로 확인), HTTP/SSE 어댑터 하나로 opencode·crush·mimo 처리 | 프로토콜 계열별로 어댑터를 재사용해 수를 줄임 |
| A5 | B tier 브리지(aider, droid, hermes 등), lease(사람 입력 우선), UI(감독자 pane, 조작 표식, tier 배지) | 나머지 에이전트 충실도 향상, 공존 안전 |

**A1이 핵심이다.** PTY를 소유한 Ferryx만 이 단계를 할 수 있고, 이것만으로 "모든 에이전트를 감시하고 입력한다"는 목표의 바닥이 깔린다. 그 뒤 단계들은 같은 계약 아래에서 에이전트별 신뢰도를 screen → native로 올리는 작업이다. 감독 로직(gateway)은 다시 짤 필요가 없다.

### 9.6 리스크
- **S tier 판정 오탐**: 화면 규칙은 휴리스틱이다(업계 공통 난제, §6). 그래서 S tier에서는 자동 답변을 기본으로 끄고 감시와 알림만 한다. 화면 파서가 있는 5종만 검증된 답변을 허용한다.
- **N tier 버전 흔들림**: provider 매트릭스의 "Remaining hard contract" 열이 그대로 리스크 목록이다. 실행 중인 프로세스의 실제 버전을 확인한 뒤에만 wire 계약을 고른다.
- **헤드리스 관찰 비용**: 세션 N개의 VT 파싱을 데몬에서 계속 돌린다. 출력이 있을 때만 갱신(dirty)하고 감지는 디바운스해야 한다(측정 필요, 미검증).



