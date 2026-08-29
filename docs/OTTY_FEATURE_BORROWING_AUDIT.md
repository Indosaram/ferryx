# OTTY(otty-shell/otty) 차용 후보 기능 감사

- 조사일: 2026-08-29
- 대상: https://github.com/otty-shell/otty (Apache-2.0, Rust, ★311, 2025-09 시작)
- 로컬 클론: `/tmp/otty-investigate` (얕은 클론, 재조사 시 재활용 가능)
- 비교 기준: Ferryx 현황 — worktree 관리, 헤드리스 데몬(UDS), 원격 웹 클라이언트,
  libghostty-vt + WGPU 네이티브 터미널 / xterm.js 원격, 스플릿 패인(DnD 이식), 브라우저 탭,
  에이전트 활동 감지(`src-tauri/agent_detect`), 링 버퍼 + ReplayGap, 알림/오디오,
  CommandPalette(워크트리·탭 전환 전용), TerminalSearchOverlay

---

## 1. Block 기반 터미널 UI — 최우선 차용 후보

otty의 플래그십 기능. 터미널 출력을 "프롬프트/명령/출력" 원자 단위 블록으로 구조화.

### 핵심 설계 (그대로 배낄 수 있는 부분)

**셸 통합 훅 (DCS 이스케이프)**
- 포맷: `ESC P otty-dcs;block;<hex(JSON)> ESC \` — 페이로드는 hex 인코딩 JSON,
  4KB 상한, `v:1` 버전 필드, 미지 kind는 무시(하위호환). `crates/escape/src/dcs/`
- zsh: `add-zsh-hook preexec/precmd` → `{"phase":"preexec","cmd","cwd","time","id"}` / `precmd` emit
- JSON 이스케이프를 jq → python3 → python → POSIX 폴백 순으로 시도하는 방어적 스크립트
  (`assets/shell-integrations/otty.zsh`)
- **ZDOTDIR 래퍼 트릭**: 사용자 `.zshrc`를 건드리지 않고 임시 ZDOTDIR에 래퍼 rc를 써서
  원본 rc source → otty 훅 source 순으로 주입 (`app/src/widgets/terminal_workspace/services.rs`).
  bash는 `ENV`/`--rcfile` 래퍼로 동일 패턴. 스크립트는 `include_str!`로 바이너리에 내장.

**서피스 측 블록 모델 (`crates/surface/src/block.rs`)**
- `BlockMeta { id, kind, cmd, cwd, shell, exit_code, started_at, finished_at, is_alt_screen, is_finished }`
- `BlockKind::Command | Prompt | FullScreen` — alt-screen 진입 시 FullScreen 블록으로 분리해
  vim/htop 같은 TUI에는 블록 UI를 적용하지 않음
- 블록마다 독립 embedded Surface + 완료 시 `cached_text: Arc<str>` 캐시 →
  **화면에 안 보이는 스크롤백 영역까지 통째로 복사 가능**
- UI 오버레이: `block_rects()`로 블록별 픽셀 rect 계산 → 블록 우상단 액션 버튼
  (복사 등) 배치 (`crates/ui/terminal/src/block_layout.rs`, `block_controls.rs`)
- `BlockUiMode::ExternalOverlay` — 터미널 렌더러는 순수하게 두고 블록 UI는 오버레이 레이어로 분리

### Ferryx 적용 시 권장 아키텍처

블록 파싱을 **데몬 레이어**(TerminalOutputHub / `agent_detect` 파이프라인)에 두는 것.
- 데몬이 이미 원시 바이트 스트림을 파싱 중이므로(에이전트 감지) 블록 메타데이터를
  링 버퍼와 병렬로 기록하면 네이티브(libghostty-vt)와 xterm.js 원격 **양쪽 서피스가 동시에 혜택**.
- 주의: 네이티브 경로는 ghostty-vt가 자체 파서를 가지므로 otty식 커스텀 DCS가 소비/무시될 수 있음.
  실무 경로는 **OSC 133(FinalTerm 프롬프트 마크) + OSC 7(cwd)** — Ghostty가 네이티브로 지원하는
  셸 통합 규격이며 데몬에서도 파싱 가능. libghostty-vt FFI가 프롬프트 마크를 노출하는지는 검증 필요.
- 파생 가치: 명령별 exit code/실행시간 표시, 프롬프트 점프 내비게이션, 명령 단위 복사,
  에이전트 활동 감지(`ferryx-agent-activity-*`)의 정밀도 향상 — 명령 경계가 정확해짐.

---

## 2. Quick Launch — 저장 명령·SSH 타겟 트리

- 폴더/노드 트리(`NodePath = Vec<String>`)로 저장 명령 관리
- `CommandSpec::Custom { program, args, env, working_directory } | Ssh { host, port, user, identity_file, extra_args }`
  — env 주입, 작업 디렉터리까지 저장 (`app/src/widgets/quick_launch/types.rs`)
- 생성/편집은 위저드 폼, 인라인 편집, 컨텍스트 메뉴, DnD 재배치, 실행 중 상태 추적,
  실행 시 새 탭으로 열림(`TabsIntent::OpenCommandTab`)
- Ferryx 적용: CommandPalette가 워크트리/탭 전환만 하는 것을 확장해
  **프로젝트(워크스페이스)별 자주 쓰는 명령**(dev server, test, deploy)을 저장·실행.
  워크트리 모델과 궁합이 좋음(커맨드를 워크트리에 스코핑). 구현 난이도 낮음, 체감 가치 높음.

## 3. Explorer — 활성 터미널 cwd에 동기화되는 파일 트리 사이드바

- 활성 탭 셸의 cwd를 추적해 `ExplorerIntent::SyncRoot { cwd }`로 트리 루트를 재설정
  (`app/src/events/explorer.rs`, `widgets/explorer/`)
- 파일 워처(`watcher.rs`)로 실시간 갱신, 범용 트리 위젯 크레이트(`crates/ui/tree`)
- Ferryx 적용: Sidebar가 프로젝트/워크트리 계층만 보여주므로, **활성 패인 cwd를 따라가는
  파일 탐색기 패널**은 공백. 특히 워크트리마다 루트가 다른 구조에 잘 맞음.
  디스크 I/O는 기존 `run_blocking` 규칙 준수. 원격 웹 클라이언트에는 트리 스냅샷 전송 필요.

## 4. SSH 세션을 1급 시민으로

- `crates/pty/src/ssh.rs`: ssh2 크레이트를 로컬 PTY와 동일 `Session` 추상화 뒤에 구현.
  mio 폴 루프 통합, `SSHAuth::Password | KeyFile { path, passphrase }`
- Session 추상화 덕분에 블록 UI·explorer 동기화·quick launch가 **SSH 위에서도 그대로 동작**
- Ferryx 적용: 현재 SSH 지원 없음. 원격 스토리가 "웹 클라이언트 → 데몬"뿐인데
  **데몬이 SSH 세션을 호스팅**하면 앱을 닫아도 세션이 살아있는 Ferryx식 차별화 가능
  (daemon session survival 모델 재사용). 모바일 헤딩 UX 타겟과도 맞음. 공수는 큼.

## 5. 즉시 구현 가능한 소형 아이템 (quick wins)

| 아이템 | 내용 | Ferryx 착점 |
|---|---|---|
| Finder 파일 붙여넣기 → POSIX 경로 | macOS Cmd+V 시 페이스보드의 `public.file-url`을 URL 디코딩해 경로 텍스트를 PTY에 기입 (`crates/ui/terminal/src/clipboard.rs`) | 네이티브/웹 양쪽 터미널 붙여넣기에 적용. 이슈 #56 정확히 이 버그 |
| 분할 시 형제 균등화 + Cmd+D | 분할 후 같은 축 그룹의 모든 split 비율을 재계산해 리프 폭差 ≤1px (`pane_balance.rs::equalized_ratios` — 순수 함수, 유닛테스트 용이). macOS Cmd+D / 기타 Logo+Shift+D | `TerminalSplitView` 바이너리 트리에 그대로 이식 가능한 알고리즘. 자동 포커스/키보드 경로 주의점(PTT 오염, auto-repeat 필터)은 그들의 스펙 `specs/2026-08-13-cmd-d-equal-panes/`에 정리돼 있음 — 그대로 학습 자료로 쓸 것 |
| OSC 22 마우스 커서 셰이프 | 앱이 `OSC 22`로 마우스 커서 모양 지정 (`osc.rs`) | 네이티브 서피스에서 미지원 시 추가. 검증 후 착수 |
| 팝업 가드 패턴 | 컨텍스트 메뉴/인라인 편집을 어떤 이벤트든 하나의 가드 함수로 닫기 (`guards.rs`) | 이미 유사 구현 있을 것 — 참고만 |

## 6. "모든 것은 탭" 패턴 (선택)

- Settings, 위저드 폼, **에러조차 탭**으로 열림 (`TabsIntent::OpenSettingsTab/OpenWizardTab/OpenErrorTab`)
- Ferryx는 모달(SettingsDialog, ConfirmCloseTabDialog) 중심 — 스타일 선택의 문제.
  에이전트 설정 같은 복잡 폼을 탭으로 올리는 것만 고려할 만함. 우선순위 낮음.

## 7. 프로토콜 설계 디테일 (블록을 하든 안 하든 배낼 것)

- hex 전송 + 4KB 상한 + `v` 버전 필드 + **미지 kind 무시 정책** — 포워드 호환되는
  이스케이프 채널 설계. Ferryx가 데몬↔셸 확장 채널을 만 일 때 그대로 준용.

## 8. 차용하지 말 것

- **자체 VTE/렌더 스택**(`crates/vte`, `surface`, `libterm`, shaped_text, render_runs):
  otty가 Iced 위에 터미널 코어를 통째로 재구축한 것. Ferryx는 이미 libghostty-vt + WGPU라는
  더 성숙한 베팅을 했으므로 되돌아갈 이유 없음. 블록 UI의 *개념과 오버레이 방식*만 가져올 것.
- i18n 10개 국어 PR(#88), flatpak/패키지 매니저 배포(#32,33): Ferryx 상황과 불일치.

## 9. otty 로드맵 이슈가 주는 신호

- #37 AI 통합, #38 스마트 입력 위젯 → Ferryx가 이미 에이전트 퍼스트로 앞서 있음
- #36 시크릿 스토리지(SSH 패스프레이즈, quick launch env 시크릿) → quick launch/SSH 도입 시 함께 설계
- #28 서브셸 블록 UI — 블록 UI의 알려진 난제(쉘 중첩 시 id 충돌/중첩 훅) 참고
- #44 다중 인스턴스 방지 — Ferryx는 데몬+GUI attach 모델로 이미 해결

## 권장 도입 순서 (가치 × 적합도 ÷ 공수)

1. **Block 기반 터미널 UI** (최대 차별화. 데몬 레이어 파싱 + OSC 133/7 경로 검증이 선행 과제)
2. **Quick Launch** (공수 대비 체감 큼 — CommandPalette 확장형)
3. **Explorer cwd 동기화 사이드바**
4. Finder 파일 붙여넣기 POSIX 경로 (quick win)
5. 분할 균등화 + Cmd+D (quick win)
6. SSH 1급 세션 (원격 스토리 확장용, 공수 큼)
7. 에러/설정 탭 패턴 (선택)
