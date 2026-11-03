# Ferryx 미지원 기능 갭 리서치 (경쟁 터미널·멀티플렉서·에이전트 매니저 대비)

- 작성일: 2026-11-01
- 범위: 다른 터미널 멀티플렉서, 터미널, 에이전트 세션 매니저에는 있고 Ferryx에는 없는 사용자 기능.
- 제외: 내장 브라우저, 내장 원격 데스크톱(진행 중).
- 조사 대상 (37개 제품, 15개 리서치 레인)
  - 멀티플렉서·세션 도구: tmux 3.8, zellij 0.45, screen(일부), tmuxinator/tmuxp/sesh/smug, tmate/upterm/sshx, asciinema/vhs, mosh/Eternal Terminal
  - 터미널: WezTerm, kitty 0.49, Ghostty 1.4, Alacritty, Rio, Contour, iTerm2 3.7, Windows Terminal 1.25/1.26, Tabby, Warp, Wave, Hyper, Termius, Electerm, VS Code/Zed/JetBrains 터미널
  - 에이전트·워크트리 매니저: Orca, cmux, Herdr/Ovis, Conductor, Paradev(구 Superset), Claude Squad, Crystal, Sculptor, Vibe Kanban, container-use
- Ferryx 근거: `ui/src`·`src-tauri/src`·`crates` 소스 인벤토리 2개 레인과 직접 grep/`git log -S` 확인.
- 원본 레인 결과(제품별 기능표와 근거 URL 전부)는 이 리서치 세션의 `waves/` 디렉터리에 있다. 아래에는 대표 URL만 넣었다.

## 1. 결론 요약

추천 1순위는 비용이 작고 Ferryx의 정체성(에이전트 감독 + 워크트리 매니저)에 바로 붙는 다섯 가지다.

1. **pane 줌 노출**: 리듀서는 이미 있고 트리거만 없다.
2. **입력 브로드캐스트**: 여러 에이전트에 같은 프롬프트를 보낸다.
3. **OSC 133 셸 통합**: 프롬프트 점프, 마지막 출력 선택, 명령 완료 알림.
4. **커맨드 팔레트를 모든 액션으로 확장**.
5. **워크트리별 setup/run/archive 스크립트와 `.worktreeinclude`**.

여기에 **OSC 7501 계열 에이전트 상태 프로토콜**을 더하면 경쟁 에이전트 매니저(Orca, cmux, Conductor, Herdr)와의 격차 대부분을 메운다.

## 2. 이미 있어서 갭이 아닌 것

아래 항목은 경쟁 제품에서 흔히 내세우지만 Ferryx에도 이미 있다.

| 기능 | Ferryx 위치 |
|---|---|
| vi 키 copy mode | `ui/src/components/TerminalCopyMode.tsx` |
| 스크롤백 검색 | `TerminalSearchOverlay.tsx`. 대소문자 무시로 고정돼 있고 정규식은 없다 |
| OSC 8 링크 | — |
| Kitty graphics | — |
| 단축키 리매핑과 프리셋 | `ui/src/lib/shortcuts.ts` |
| 라이트/다크 자동 전환 | — |
| pane 드래그 분할·스왑·균등화 | — |
| 레이아웃 자동 복원 | — |
| 데몬 세션 영속과 무중단 핸드오버 | — |
| 에이전트 감지와 대기/완료 알림 | 16개 에이전트 |
| 승인 인박스 | — |
| Git 변경 패널 | stage, commit, push |
| 원격 웹·모바일 클라이언트 | View/Control 역할, Web Push 포함 |
| SSH, Tailscale, 페어링 릴레이, QUIC 직결 | — |
| 원격 드래그 업로드 | — |
| 파일 미리보기 | — |
| 리소스 모니터 | — |
| 이미지 붙여넣기 | 원격 경로 처리 포함 |
| OSC 52 클립보드 쓰기 | `crates/ferryx-vt/src/engine.rs:118`의 `on_clipboard_write`가 `UiEventKind::ClipboardWrite`를 만든다 |

## 3. 우선순위 랭킹

등급 기준:
- **P0**: 비용이 작고 정체성에 바로 맞는다. 다음 스프린트 대상.
- **P1**: 가치가 높지만 비용이 중간이다.
- **P2**: 있으면 좋지만 우선순위가 낮다.
- **P3/skip**: Ferryx 포지셔닝과 맞지 않거나 이미 대체 수단이 있다.

| # | 기능 | 보유 경쟁 제품 | Ferryx 현황(근거) | 가치 | 비용 | 등급 |
|---|---|---|---|---|---|---|
| 1 | pane 줌/최대화 | tmux, WezTerm, Ghostty, kitty, iTerm2, WT, Warp, Wave, cmux, Herdr, Contour | 리듀서 `TOGGLE_PANE_EXPANDED`와 `expandedLeafId`(`ui/src/state/layout.ts:672`)는 있다. 단축키·메뉴·버튼은 없다. `git log -S`상 처음부터 연결된 적이 없고 알림 라우팅(`faad9666`)만 내부적으로 쓴다 | 높음 | 매우 낮음 | **P0** |
| 2 | 입력 브로드캐스트/동기화 | tmux, zellij, iTerm2, WT, Warp, Wave, Tabby, Termius, kitty(kitten), Electerm | 없음. UI·백엔드에서 `broadcast`/`sync_input` 사용자 기능 0건 | 높음. 여러 에이전트에 같은 지시를 보낼 때 쓴다 | 낮음~중 | **P0** |
| 3 | OSC 133 셸 통합 (프롬프트 점프, 마지막 출력 선택·복사, 명령 완료 알림) | Ghostty, WezTerm, kitty, tmux 3.8, zellij 0.45, VS Code, WT, iTerm2, Rio, Contour | 없음. `native_terminal/sys/types.rs:293`의 `semantic_prompt_boundary`가 false로 고정돼 있고 UI에 기능이 없다 | 높음. 셸 작업과 에이전트 외 명령에 유용하다 | 중. 셸 스크립트 주입과 ghostty-vt 마크 연동이 필요하다 | **P0** |
| 4 | 범용 커맨드 팔레트 | Ghostty, WezTerm, kitty, WT, VS Code, Warp, Rio, Contour, Conductor | 워크트리와 탭만 다루고 명령은 "Show changes" 하나다(`CommandPalette.tsx:22`). `SHORTCUTS`에 액션 제목이 이미 있다 | 높음 | 낮음 | **P0** |
| 5 | 워크트리별 setup/run/archive 스크립트, gitignore 파일 복사(`.worktreeinclude`) | Orca(`orca.yaml`), Conductor(`conductor.json`), Paradev, Wave, VS Code tasks | 없음 | 매우 높음. 새 워크트리에서 `.env` 복사와 install을 자동화한다 | 중 | **P0** |
| 6 | 에이전트 상태 표준 프로토콜 (OSC 7501, OSC 21337) | zellij main, cmux 0.65, kitty 0.50(미출시), herdr-core, iTerm2 3.7 | main에는 없다. 다른 브랜치에 OSC 7501 커밋(`0ee73dc3`, `93c3e562`)이 있지만 HEAD에 포함되지 않았다. 따라서 "머지 마무리" 항목이다 | 높음. 사용자 정의 에이전트와 스크립트도 상태를 보고할 수 있게 된다 | 낮음(머지) | **P0** |
| 7 | 오케스트레이션 CLI/API (pane read/send/wait-output, 이벤트 구독) | Herdr(`agent prompt --wait`, `pane wait-output`, `events.subscribe`), zellij `subscribe`·`action`, cmux socket API, WezTerm `cli`, kitty `@`, Paradev SDK, Conductor MCP | 부분 지원. `ferryx agents` CLI, MCP 허브, supervisor는 있다. pane 출력 대기와 이벤트 스트림은 없고, tmux식 `ls`/`attach` CLI도 없다(`bin/cli.rs`) | 높음 | 중 | **P1** |
| 8 | 레이아웃 템플릿과 이름 있는 세션 저장·전환 | zellij KDL(`dump-layout`), kitty sessions, WT workspaces(1.26), Warp Tab Configs, iTerm2 arrangements, cmux, tmuxinator/tmuxp, Termius | 자동 세션 하나만 있다(`session/mod.rs`) | 중~높음 | 중 | **P1** |
| 9 | PR·CI 상태 통합 (워크트리 → PR 생성, 체크, 리뷰 코멘트) | Orca(GitHub/GitLab/Bitbucket, 스택 PR), Conductor(Checks 탭, PR 페이지), Paradev, cmux 사이드바 | GitHub 이슈 미리보기만 있다 | 높음 | 중~높음 | **P1** |
| 10 | diff 리뷰 코멘트를 에이전트에 전달 | Orca, cmux, Warp, Conductor | 변경 패널은 보기와 커밋만 된다 | 높음 | 중 | **P1** |
| 11 | 스크롤백 고급 기능 (정규식·대소문자 옵션, 파일 덤프, `$EDITOR`로 열기) | Ghostty(`write_scrollback_file`), zellij(`EditScrollback`), WT(`exportBuffer`, regex), Herdr, iTerm2, tmux | 대소문자 무시 검색만 된다(`TerminalSearchOverlay.tsx:94`) | 중 | 낮음 | **P1** |
| 12 | 포트 패널과 포워딩 UI | Paradev(포트 자동 발견), Conductor(자동 포워딩), Orca, Tabby, Termius, cmux 사이드바 | `DevServerPortForward`(`worktree/manager.rs:39`)는 쓰이지 않는 스텁이다 | 중~높음. 원격 워크트리 개발 서버 확인에 필요하다 | 중 | **P1** |
| 13 | 퀵 커맨드와 스니펫 | Orca Quick Commands, iTerm2 Snippets, WT `.wt.json`, Warp Workflows, Termius, Electerm, Tabby 플러그인 | 없음 | 중 | 낮음 | **P1** |
| 14 | 멀티 윈도우 | 거의 전부(Ghostty, WezTerm, kitty, iTerm2, WT, Paradev, cmux…) | `main` 창 하나뿐이다(`lib.rs:2916`) | 중. 멀티 모니터에 필요하다 | 중~높음. 네이티브 서피스가 창마다 필요하다 | **P1** |
| 15 | 퀵/퀘이크 터미널, 전역 단축키 | Ghostty, kitty, WT, iTerm2, Warp, Wave, Tabby, Rio, Contour, Electerm | 없음 | 중 | 중 | **P2** |
| 16 | 힌트 모드 (URL·경로·해시를 키보드 라벨로 선택) | WezTerm QuickSelect, kitty hints, Alacritty, Contour, Rio, tmux-fingers | 없음. 링크 클릭만 된다 | 중 | 낮음~중 | **P2** |
| 17 | 체크포인트와 세션 포크 (턴 단위 되돌리기, 대화 분기) | Conductor Checkpoints, cmux Vault/fork, Crystal revert | 없음. 재개만 된다 | 중~높음 | 중~높음 | **P2** |
| 18 | 트리거 (출력 정규식에 하이라이트·알림·명령 실행) | iTerm2 Triggers, Electerm, Tabby 로그인 스크립트, tmux 훅 | 없음 | 중 | 중 | **P2** |
| 19 | 세션 녹화와 재생 (asciicast) | WezTerm `record`/`replay`, iTerm2 로깅·Instant Replay, Termius 세션 로그, asciinema | 없음. "replay"는 재접속 스트림을 말한다 | 중. 에이전트 작업 감사·공유에 쓴다 | 중 | **P2** |
| 20 | 파일 트리와 경량 에디터 | Orca(Monaco), Warp, Wave, Paradev, Conductor(diff 내 편집) | 읽기 전용 미리보기만 있다 | 중 | 높음 | **P2** |
| 21 | 무활동·출력 정지 알림 | tmux `monitor-silence`, screen, iTerm2 | 에이전트 상태 알림이 대부분을 대신한다. 일반 셸 pane은 대상이 아니다 | 낮음~중 | 낮음 | **P2** |
| 22 | 읽기 전용 pane, pane 재시작 | WT, Ghostty `toggle_readonly`, Contour | 탭 재시작만 있다 | 낮음~중 | 낮음 | **P2** |
| 23 | 앱 내 터미널 테마 선택 | Ghostty, kitty themes kitten, WT, Warp, Herdr | Ghostty 설정을 가져오기만 한다 | 낮음~중 | 낮음 | **P2** |
| 24 | 작은 편의 기능 (copy-on-select, 닫은 터미널 탭 다시 열기, 터미널 우클릭 메뉴) | Ghostty, WT, Tabby, Hyper, Herdr | 우클릭 IPC `cmd_native_terminal_context_menu`는 있지만 UI가 호출하지 않는다. 다시 열기는 브라우저와 파일 탭만 된다 | 낮음~중 | 매우 낮음 | **P2** (빠른 정리 대상) |
| 25 | 플러그인 시스템 | zellij(WASM), WezTerm(Lua), Herdr(마켓플레이스), Hyper, Orca(알파), kitty kittens | 없음 | 중 | 높음 | **P2** |
| 26 | 상태 표시줄 커스터마이즈 | tmux, iTerm2, Herdr, Contour | 없음 | 낮음 | 중 | **P3** |
| 27 | SFTP 브라우저와 다운로드 | Tabby, Termius, Orca, Wave, upterm | 업로드(드래그)만 된다 | 중 | 중 | **P2** |
| 28 | 스크린리더로 터미널 텍스트 읽기 | WT, Hyper, Electerm, Ghostty(부분), Contour | 부분 지원. `aria-live`만 있고 그리드는 접근할 수 없다 | 중(접근성) | 높음 | **P2** |
| 29 | Linear/Jira 연동, 워크스페이스 자동 이름 | Orca, Paradev, Conductor, cmux | 없음 | 중 | 중 | **P2** |
| 30 | 다중 사용자 실시간 공유 (멀티플레이어 커서, 공유 링크) | zellij web, sshx, upterm, Termius, Warp | 원격 View/Control은 있다. 다중 사용자 초대는 없다 | 낮음~중 | 높음 | **P3** |
| 31 | 세션 잠금 | tmux `lock-after-time`, screen | 없음 | 낮음 | 낮음 | **P3** |
| 32 | mosh, 시리얼, Telnet | Termius, Tabby, WezTerm(serial), Electerm | 없음. QUIC 직결과 재접속이 mosh 역할을 한다 | 낮음 | 중 | **skip** |
| 33 | tmux control mode(`-CC`) | iTerm2, WezTerm(nightly), cmux `ssh-tmux`, Contour | 없음 | 낮음. 자체 데몬과 겹친다 | 높음 | **skip** |
| 34 | AI 명령 도움 (자연어 → 명령, 출력 설명) | Warp, iTerm2 AI Chat, Termius, Electerm, WT(Canary) | 없음 | 낮음. Ferryx는 에이전트를 직접 띄우는 도구다 | 중 | **skip** |
| 35 | Jujutsu 지원 | sesh, (cmux) | 없음 | 낮음 | 중 | **P3** |

## 4. 근거 URL (대표)

- pane 줌: https://github.com/tmux/tmux/blob/7f2a35ad3321f9ba57a1062ca73b1f3ff26aca53/tmux.1?plain=1#L3834 · https://wezterm.org/config/lua/keyassignment/TogglePaneZoomState.html · https://ghostty.org/docs/config/keybind/reference#toggle_split_zoom
- 브로드캐스트: https://zellij.dev/documentation/keybindings-possible-actions.html#toggleactivesynctab · https://iterm2.com/documentation-menu-items.html · https://docs.warp.dev/terminal/entry/synchronized-inputs/ · https://sw.kovidgoyal.net/kitty/kittens/broadcast/
- OSC 133: https://ghostty.org/docs/features/shell-integration · https://wezterm.org/shell-integration.html · https://zellij.dev/documentation/shell-integration.html · https://github.com/tmux/tmux/blob/7f2a35ad3321f9ba57a1062ca73b1f3ff26aca53/tmux.1?plain=1#L6516
- 커맨드 팔레트: https://ghostty.org/docs/config/keybind/reference#toggle_command_palette · https://sw.kovidgoyal.net/kitty/kittens/command-palette/ · https://wezterm.org/config/lua/keyassignment/ActivateCommandPalette.html
- 워크트리 스크립트: https://www.conductor.build/docs/reference/scripts · https://www.conductor.build/docs/reference/worktreeinclude · https://github.com/ParadevAI/Paradev/blob/8b523240cffd1501024297ff9efed55d258f2798/apps/docs/content/docs/setup-teardown-scripts.mdx
- 에이전트 상태 프로토콜: https://herdr.dev/core/ · https://github.com/kovidgoyal/kitty/blob/5a6ea12f29f7a35cfd89afcf8c80cffa25386f02/docs/changelog.rst?plain=1#L251-L255 · https://iterm2.com/documentation-session-status.html
- 오케스트레이션 CLI: https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/docs/next/website/src/content/docs/agent-automation.mdx#L59 · https://zellij.dev/documentation/zellij-subscribe.html · https://sw.kovidgoyal.net/kitty/remote-control/
- 레이아웃과 세션: https://zellij.dev/documentation/layouts.html · https://sw.kovidgoyal.net/kitty/sessions/ · https://github.com/microsoft/terminal/releases/tag/v1.26.2734.0 · https://docs.warp.dev/terminal/windows/tab-configs/
- PR·CI·리뷰: https://www.conductor.build/docs/reference/checks · https://github.com/ParadevAI/Paradev/blob/8b523240cffd1501024297ff9efed55d258f2798/apps/docs/content/docs/pull-requests.mdx · https://docs.warp.dev/code/code-review/
- 스크롤백 덤프: https://ghostty.org/docs/config/keybind/reference#write_scrollback_file · https://zellij.dev/documentation/options.html#scrollback_editor
- 포트: https://github.com/ParadevAI/Paradev/blob/8b523240cffd1501024297ff9efed55d258f2798/apps/docs/content/docs/ports.mdx · https://www.conductor.build/changelog/0.83.0-auto-port-forwarding
- 스니펫: https://docs.warp.dev/terminal/entry/yaml-workflows/ · https://docs.termius.com/terminal/snippets
- 퀵 터미널: https://ghostty.org/docs/config/keybind/reference#toggle_quick_terminal · https://sw.kovidgoyal.net/kitty/kittens/quick-access-terminal/
- 힌트: https://wezterm.org/quickselect.html · https://sw.kovidgoyal.net/kitty/kittens/hints/
- 체크포인트: https://www.conductor.build/docs/reference/checkpoints
- 트리거: https://iterm2.com/documentation-triggers.html
- 녹화: https://wezterm.org/cli/record.html · https://docs.termius.com/organize-and-connect-to-hosts/session-logs
- 플러그인: https://zellij.dev/documentation/plugin-api.html · https://wezterm.org/config/plugins.html · https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/docs/next/website/src/content/docs/plugins.mdx#L54

## 5. 추천 로드맵

1. **바로 할 것 (며칠 단위, P0와 #24)**
   - pane 줌 단축키와 메뉴 연결
   - 커맨드 팔레트를 `SHORTCUTS` 전체로 확장
   - 터미널 우클릭 메뉴 연결
   - copy-on-select
   - 닫은 터미널 탭 다시 열기
2. **다음 스프린트**
   - 입력 브로드캐스트: 탭 단위 토글과 경고 띠
   - OSC 7501 수신: 기존 에이전트 상태 모델에 매핑
   - 워크트리 setup/run/archive 스크립트와 `.worktreeinclude`
3. **그다음**
   - OSC 133 셸 통합 (bash/zsh/fish/PowerShell 주입 필요)
   - 스크롤백 덤프와 정규식 검색
   - 포트 패널
4. **분기 과제 (P1의 나머지)**
   - 오케스트레이션 CLI (`pane read`/`wait-output`/`events`)
   - 레이아웃 템플릿
   - PR·CI 통합과 diff 리뷰 코멘트
   - 멀티 윈도우

## 6. 한계

- screen/byobu/dtach 전용 레인은 중간에 중단했다. 이들의 고유 기능(세션 잠금, 무활동 감시, 로깅)은 tmux와 power-features 레인에서 이미 다뤘다.
- 등급은 Ferryx 포지셔닝을 바탕으로 한 판단이다. 사용량 데이터는 반영하지 않았다.
- Ferryx 현황은 소스를 읽어서 판단했고, 실행해서 확인하지는 않았다.
