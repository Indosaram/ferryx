# Ferryx를 쓰지 않을 이유 — 적대적 비채택 분석 (2026-10-24)

> 관점: Ferryx를 깎아내리려는 회의적 개발자·CTO·보안 담당자·구매 담당자.
> 방법: 9개 병렬 explore 레인(포지셔닝, 설치/플랫폼, 신뢰성, 보안, 비즈니스, 복잡도/UX, 문서, 리소스, 시장) + 오케스트레이터 직접 검증.
> 기준 리비전: `main` @ `62ed79f2` (2026-10-24), 앱 버전 `2026.1024.1`.

## 증거 태그

| 태그 | 의미 |
| --- | --- |
| **SOURCE** | 코드·문서·사이트 카피를 직접 읽어 확인한 사실 (`path:line`) |
| **MEASURED** | 사고 기록, 로그, git/명령 출력에서 나온 실측 수치 |
| **INFERRED** | 위 사실에서 끌어낸 추론. 반증 가능성 있음 |

메모리 노트 경로는 `$MEM = /Users/indo/.omo/memory/agents/ferryx-f3088a01/repo/notes` 기준이다.

---

## 요약 — 회의론자가 내릴 결론

1. **세션을 맡기기 무섭다.** 9/26~10/15 사이 실측 세션 유실 사고만 8건 이상, 최대 54/74 패널 손실. 마지막 두 구조적 수정(c5049bf9, f1730fa3)은 재측정 기록이 없다. 그런데 사이트는 "절대 작업을 잃지 않는다고 보장"한다.
2. **스스로 "tmux 대비 새 능력은 없다"고 말한다.** 단일 벤더 사용자에게는 `claude --worktree`/Codex 워크트리를 쓰라고 공식 문서가 안내한다. 그리고 그 벤더들이 원격 제어까지 흡수 중이다.
3. **회사가 쓰기 어렵다.** 오픈소스가 아닌 SUL-1.0(9/14 MIT에서 전환), 버스 팩터 1, 개인 머신에서 빌드되는 릴리스, 구매할 수 없는 유료 플랜, 단일 호스트 릴레이, SLA·지원 채널 부재.
4. **보안 검토를 통과하기 어렵다.** 비-opaque 세션은 릴레이에서 평문, 에이전트가 터미널에 타이핑하는 기능이 기본 ON, 머신 grant = 원격 코드 실행, 릴레이와 신뢰 루트 공유, CSP `unsafe-eval`.
5. **처음 보는 사람에게 미완성으로 보인다.** 사이트는 지원 안 하는 Gemini CLI를 광고하고 에이전트 수를 11개로 말하며(실제 16), README는 제거된 QR 페어링을 광고하고, 저장소 루트에는 에이전트 작업 잔해가 커밋돼 있다.

---

## 심각도 순위

| # | 비채택 사유 | 심각도 | 영역 |
| --- | --- | --- | --- |
| 1 | 세션 유실 이력과 "절대 잃지 않음" 과장 | 치명 | 신뢰성 |
| 2 | 벤더 자체 기능 흡수 + 자기 인정한 차별점 부족 | 치명 | 포지셔닝/시장 |
| 3 | 비-OSI 라이선스와 "free and open" 카피 | 높음 | 비즈니스/신뢰 |
| 4 | 1인 프로젝트·과도한 변경량·개인 머신 릴리스 | 높음 | 벤더 리스크 |
| 5 | 릴레이 평문 경로, 기본 ON 에이전트 제어, 공유 신뢰 루트 | 높음 | 보안 |
| 6 | 유료 플랜 구매 불가 + Free 1대 제한 + 단일 호스트 릴레이 | 높음 | 가격/운영 |
| 7 | 설치·권한·플랫폼 마찰 | 높음 | 설치 |
| 8 | 레포에 강제되는 `.orca-worktrees`/`orca/*` 브랜치와 레거시 이름 | 높음 | UX/팀 정책 |
| 9 | 개념·기능 과다, 단축키 충돌, 터미널 충실도 | 중상 | 복잡도 |
| 10 | 공개 주장과 실제 불일치, 사용자 문서 부재, 저장소 위생 | 중상 | 문서/지원 |
| 11 | 메모리·디스크·지연 비용 | 중간 | 성능 |

---

## 1. 세션 유실 이력과 "절대 잃지 않음" 과장 — 치명

**주장.** Ferryx의 핵심 약속은 "데몬이 PTY를 소유하므로 에이전트 세션이 살아남는다"이다. 그런데 바로 그 약속이 실측으로 반복해서 깨졌고, 사이트 카피는 여전히 보장이라고 말한다.

**증거.**
- 사이트 카피: `site/src/lib/locale.ts:28` — "Automatic workspace state snapshotting and background daemon reattachment **guarantee you never lose work on crash or exit**." (SOURCE)
- README는 반대로 인정: `README.md:134-135` — "stopping the daemon or rebooting the host can still end live PTYs." (SOURCE)
- 실측 사고 원장 (MEASURED, 모두 `$MEM/incidents/`):

| 날짜 | 사건 | 손실 |
| --- | --- | --- |
| 09-25 | 데몬 fd soft limit 256 → EMFILE, 하루 3회 전멸 (`$MEM/ferryx-daemon-fd-limit-256-session-death.md`) | 전체 |
| 09-26 | 라이브 핸드오버 (`2026-09-26-handover-lost-35-of-55-sessions.md`) | 35/55 |
| 09-26 | 한 번의 업그레이드에 핸드오버 2홉 (`2026-09-26-handover-lost-26-of-37-sessions.md`) | 26/37 |
| 09-26 | 수정 설치 후에도 유실 (`$MEM/facts/ferryx-handover-fix-did-not-stop-session-loss-2026-09-26.md`) | 37/42 |
| 09-27 | 강제 핸드오버 (`2026-09-27-forced-handover-926-1-to-927-2-lost-40.md`) | 62→22 |
| 10-07 | 앱 재시작 시 핸드오버 없는 데몬 교체 (`2026-10-07-app-restart-daemon-replacement-without-handover.md`) | 54/74 |
| 10-08 | 강제 종료가 같은 프로세스 그룹의 데몬까지 죽임 (`2026-10-08-force-quit-kills-daemon-same-process-group.md`) | 53/72 |
| 10-08 | 원인 불명 SIGTERM + 재연결 attach 결함 (`2026-10-08-daemon-sigterm-2328-no-handover-and-reconnect-attach-defect.md`) | 다수 |
| 10-13 | Dock 종료가 launchd 데몬까지 SIGTERM (coalition 귀속) (`2026-10-13-dock-quit-terminates-launchd-daemon-attributed-to-app.md`) | 전체 |
| 10-15 | Dock 강제 종료로 핸드오버 입양 세션 사망, 메커니즘 미증명 (`2026-10-15-dock-force-quit-killed-handover-adopted-sessions.md`) | 20/32 |
| 10-23 | 1011→1022 업그레이드 EPIPE로 중단 (`$MEM/facts/maho-mac-reinstall-2026-1022-4-and-handover-broken-pipe-2026-10-23.md`) | 자동 업그레이드 불가 |
| 10-24 | 재부팅 후 모든 에이전트 패널이 영원히 "Reconnecting…" (`$MEM/facts/ferryx-reboot-stuck-reconnecting-root-cause-2026-10-24.md`) | 수동 정리 필요 |

- 이 기록은 다른 사용자가 아니라 **개발자 본인 머신**에서 나왔다. 외부 사용자는 이보다 다양한 환경을 가진다. (INFERRED)
- 세션 외 신뢰성 사고도 있다. 하루 4회 GUI 프리즈, 7.5 GB, WKURLSchemeTask 127,192개 누수 (`$MEM/facts/ferryx-gui-freeze-unanswered-ipc-leak-2026-10-22.md`). macOS 디스크 쓰기 위반 보고 18건, 최대 137 GB (`2026-09-26-app-freeze-disk-write-storm.md`, 프리즈 원인 미확정). 탭 전환 빈 패널은 가설 5개가 반증됐고 근본 원인이 없다 (`$MEM/facts/ferryx-tab-switch-blank-until-hover-falsified-hypotheses-2026-10-14.md`). (MEASURED)
- 수정 변동량: 최근 300커밋 중 132개가 "fix"이고, 이 300개는 6일(10-19~10-24) 안에 몰려 있다. (MEASURED, git)

**가장 강한 반론.** 모든 사고에 근본 원인과 커밋이 남아 있다. 10-18 핸드오버는 119/119, 10-24 핸드오버는 12/12 세션을 무손실로 넘겼다 (`$MEM/facts/ferryx-app-install-zero-loss-handover-and-catalog-v2-sync-2026-10-18.md`). 문제를 숨기지 않고 찾아서 고친 기록이라는 해석도 가능하다.

**재반론.** 세션 유실 클래스가 10-23까지 새로 나왔다. 10-15 사건은 "메커니즘 미증명"이고, c5049bf9(launchd job 분리)와 f1730fa3(재부팅 처리)은 실측 재검증 기록이 없다.

**무력화하려면.** `locale.ts:28`의 "guarantee"를 삭제한다. Dock quit, force quit, 재부팅, 버전 갭 업그레이드를 자동 회귀 시나리오로 만들어 릴리스마다 공개 결과로 남긴다. 업그레이드 경로 호환표(어느 버전에서 어느 버전으로 안전한지)를 공개한다.

---

## 2. 벤더 자체 기능 흡수와 자기 인정한 차별점 부족 — 치명

**주장.** 타깃 사용자 대부분은 Ferryx가 필요 없다. 이걸 Ferryx 자신의 문서가 말한다.

**증거.**
- `site/src/content/docs/compare/tmux-git-worktree.md:40` — "None of it adds capability over tmux". (SOURCE)
- `site/src/content/docs/use-cases/parallel-ai-agents.md:84` — "Nothing above requires Ferryx. One vendor's worktree flag, or git worktree plus tmux… gets you the isolation". (SOURCE)
- `site/src/components/landing/Alternatives.astro:9-10`는 단일 에이전트 사용자를 `claude --worktree`나 Codex Worktree 모드로 안내한다. (SOURCE)
- Claude Code는 `-w` 워크트리와 Remote Control(QR, 폰 동기화, 푸시, `--spawn worktree`)을 직접 제공한다. Codex 앱에도 워크트리와 Handoff가 있다 (`docs/SITE_SEARCH_PRIORITIES_2026-09-19.md:105-118, 154-160, 195-201`). 사용자들은 "codex app remote is good enough"라며 herdr를 떠났다 (`docs/research/cmux-orca-herdr-user-complaints-2026-10-14.md:1584-1585`). (SOURCE, 외부 기능 상태는 2026-09-19 기준)
- 카테고리 포화: "all these ADEs are mostly the exact same" (`docs/research/cmux-orca-herdr-user-complaints-2026-10-14.md:1140-1147`). 2026-10-14 기준 Orca 92k, herdr 44k, cmux 28k 스타. Ferryx는 브랜드 검색에 제3자 결과가 0건이다 (`$MEM/facts/ferryx-aeo-question-mining-2026-09.md`). (SOURCE/MEASURED)
- 헤드라인 "Parallel agentic development. Zero bloat." (`README.md:9`)는 구매자를 지목하지 않고, libghostty·wgpu·3.10 ms 같은 체감 불가능한 아키텍처 지표로 시작한다 (`README.md:11-12, 61`). (SOURCE)
- 가장 가까운 경쟁자(cmux, Orca, herdr) 비교 페이지는 모두 `draft: true`로 미공개다 (`docs/compare-drafts/*.md:4`). Orca 초안은 디자인 모드·에디터·diff 리뷰를 Orca 우위로 인정한다. (SOURCE)
- Conductor 비교 페이지는 "there's no Ferryx cloud"이고, SOC 2·SSO·멀티플레이어가 없다고 인정한다 (`site/src/content/docs/compare/conductor.md:43`). (SOURCE)

**가장 강한 반론.** 한 저장소에서 여러 벤더의 에이전트를 충돌 없이 돌리는 것, GUI와 독립된 데몬, 셀프호스트 가능한 폰 원격은 벤더 앱이 주지 않는 실제 쐐기다. Claude Remote Control은 Pro/Max 구독이 필요하고, API 키·Bedrock을 거부하고, Claude 전용이다 (`docs/SITE_SEARCH_PRIORITIES_2026-09-19.md:203-212`).

**재반론.** 그 쐐기의 시장은 "멀티 벤더 + 컴플라이언스 제약"이라는 좁은 교집합이다. 그 사용자층은 셀프호스팅을 선호하는 tmux 파워유저와 겹치고, 이들은 이미 tmux로 돌아가는 중이다 (`cmux-orca-herdr-user-complaints-2026-10-14.md:58, 1089`).

**무력화하려면.** 헤드라인을 결과 중심으로 바꾼다. 예: "Claude와 Codex를 한 레포에서 충돌 없이, 노트북을 닫아도 살아 있게, 폰에서 조종." cmux/Orca/herdr 비교 페이지를 공개한다. 2026-10 기준으로 벤더 기능 상태를 다시 검증한다.

---

## 3. 비-OSI 라이선스와 "free and open" 카피 — 높음

**증거.**
- `LICENSE:1-22`는 Sustainable Use License 1.0이다. "only for your own internal business purposes or for non-commercial or personal use", 배포는 "free of charge for non-commercial purposes"만 허용된다. (SOURCE)
- 커밋 `48ee5a9c` (2026-09-14) "chore(license): relicense from MIT to SUL-1.0". (MEASURED) 출시 한 달 만의 비공개 방향 전환은 이 시장이 가장 두려워하는 패턴이다. herdr는 반대로 AGPL에서 Apache-2.0으로 바꾼 뒤 채택이 늘었다 (`docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md` §7). (SOURCE/INFERRED)
- `site/src/content/docs/docs/pricing.mdx:29` — "completely free and **open**". 법무팀이 걸고넘어질 표현이다. (SOURCE)
- 내부 분석 문서 `docs/FERRYX_COMPETITIVE_SENTIMENT_ADOPTION_2026-09-12.md:59`에는 아직 "MIT 라이선스"라고 적혀 있다. (SOURCE)
- 경쟁자 라이선스: Orca는 MIT, herdr는 Apache-2.0, cmux는 GPL, tmux는 ISC다 (positioning 레인). (SOURCE)

**반론.** 사내 업무용 사용은 명시적으로 허용되므로 대부분의 개발팀에는 문제가 없다.
**재반론.** MSP·컨설팅·사내 배포 툴체인 번들링·디스트로 패키징은 막힌다. "source-available"을 "open"으로 부르는 순간 신뢰를 잃는다.
**무력화하려면.** 카피를 "source-available"로 고친다. 라이선스 FAQ에 허용·금지 사례 표를 넣는다. 라이선스 결정 자체도 재검토한다.

---

## 4. 1인 프로젝트, 과도한 변경량, 개인 머신 릴리스 — 높음

**증거.**
- `git shortlog -sne --all`: `indo@local` 2609커밋. 사람 식별자는 모두 한 사람(GitHub ID 47408490)이고, 나머지 약 61개는 봇과 에이전트다. (MEASURED)
- 첫 커밋 2026-08-20, 약 9주 동안 2,021커밋(HEAD 기준, 전체 브랜치 기준 2,726). 주당 148~438커밋, W43이 438로 최고다. (MEASURED) 이 숫자는 안정화가 아니라 재작업으로 읽힌다. (INFERRED)
- 태그가 불규칙하다. 이틀 동안 `v2026.08.26.1`~`.11`의 11개, `0.1.0`과 `v0.1.0`이 공존하고, 마지막 릴리스 태그는 10-12인데 커밋은 10-24까지 계속된다. 날짜 기반 버전이라 브레이킹 체인지 신호가 없다 (`docs/releases/LOCAL_RELEASE_RUNBOOK.md:36`). (MEASURED)
- 릴리스는 개인 머신(`macbook`, `omaki`, `maho-win`)에서 서명·조립·배포된다 (`docs/releases/LOCAL_RELEASE_RUNBOOK.md:4, 17-19`). CI 증명(attestation)과 재현 가능 빌드가 없어 SLSA·SOC2 공급망 검토에서 약점이다. (SOURCE)
- 업데이터는 개인 GitHub 저장소 하나와 minisign 키 하나에 의존하고, 매시간 확인하며, Windows는 `passive` 설치다 (`src-tauri/tauri.conf.json:34-42`, `ui/src/lib/updater.ts:310`). (SOURCE)
- 저장소 활동 상당 부분을 AI 에이전트가 수행한다(Cursor Agent, Grok Bot, ferryx-agent 커밋 저자). 내부 운영 문서 일부는 한국어다. (MEASURED/INFERRED)

**반론.** 응답 속도가 빠르다. 라이선스가 사내 포크를 허용하므로 최악의 경우 자체 유지보수가 가능하다.
**재반론.** 코드베이스(App.tsx 4,396줄, Tauri 커맨드 241개)를 외부 팀이 떠안는 것은 현실적이지 않다.
**무력화하려면.** 공개 로드맵과 변경 로그, 안정 채널과 베타 채널 분리, CI 기반 서명 릴리스와 provenance, 소스 에스크로 또는 라이선스 전환 약정.

---

## 5. 보안: 릴레이 평문 경로, 기본 ON 에이전트 제어, 공유 신뢰 루트 — 높음

**증거 (실제 코드 결함 후보).**
- **릴레이가 평문을 볼 수 있다.** `src-tauri/src/remote/session_transport.rs:37-39`: `if !opaque { return Ok(SessionTransport::Plain(stream)); }`. 문서 스스로 `docs/account-service.md:107`에서 "do not describe a live account attach as encrypted until that wiring lands"라고 쓴다. (SOURCE) 다만 현재 프로덕션 경로 중 `opaque=false`로 가는 것이 어느 것인지는 이번 패스에서 확정하지 않았다.
- **기본 릴레이가 제3자다.** `src-tauri/src/remote/state.rs:20` `DEFAULT_RELAY_URL = "https://relay.ferryx.dev"`. (SOURCE)
- **신뢰 루트를 릴레이와 공유한다.** `docs/direct-peer.md:60` — "the account signing key is not independent of the relay host". 자동 신뢰가 기본 ON이다. (SOURCE)
- **머신 grant는 곧 원격 코드 실행이다.** `docs/paired-daemon-machine-access.md:11` — "permits browsing raw filesystem paths and executing programs as the daemon's OS user". (SOURCE)
- **에이전트가 터미널에 타이핑하는 기능이 기본 ON이다.** `src-tauri/src/agent_control/policy.rs:38-43` `enabled: AtomicBool::new(true)`. 사람과의 충돌을 막는 장치는 3초 타이핑 가드뿐이다 (`policy.rs:25`). (SOURCE)
- **페어링 비밀이 약하다.** 6자리 PIN, 전역 5회 실패 예산(공격자가 정상 페어링을 잠글 수 있음), 데몬 재시작 시 리셋. 릴레이 PIN 경로에서는 릴레이가 PIN을 본다 (`src-tauri/src/remote/auth.rs:263-276, 500`, `relay_client.rs:193, 200-202`). (SOURCE)
- **베어러 토큰은 30일 idle 만료라** 활성 토큰은 사실상 무기한이다. `remote-auth.json`(0600)과 localStorage에 저장된다 (`auth.rs:278, 1259`). (SOURCE)
- **LAN 게이트웨이**는 CORS `allow_origin(Any)`이고 (`src-tauri/src/remote/server.rs:7355`), 환경 변수로 insecure direct·insecure relay를 켤 수 있다 (`server.rs:7562-7569`, `relay_client.rs:851-855`). (SOURCE)
- **Webview CSP**가 `script-src 'self' 'unsafe-inline' 'unsafe-eval'`이고, 프로덕션 설정에 dev 서버 origin `127.0.0.1:5173`이 남아 있다 (`src-tauri/tauri.conf.json:30`). (SOURCE)
- **허브 통합**은 데몬 프로세스가 Slack 토큰(`~/.omon/secrets`)을 읽고 cron 셸 페이로드를 실행한다 (`docs/hub-cutover.md:3, 7`, 기본 OFF). (SOURCE)
- **릴레이·계정 측 데이터 보존 정책이 문서화되지 않았다.** 릴레이는 피어 IP와 SQLite 과금 상태를 보관한다. (INFERRED)

**반론.** 게이트웨이는 기본 OFF이고, 셀프호스트 릴레이가 있다. Noise식 핸드셰이크는 구현돼 있다(`attach_crypto.rs`). 데스크톱 텔레메트리는 없다. 위험을 문서가 정직하게 공개한다.
**재반론.** 보안 검토자는 "정직하게 공개된 평문 경로"도 평문 경로로 본다. 기본값이 ON인 원격 키 입력은 감사에서 즉시 걸린다.
**무력화하려면.** 모든 릴레이 경로를 E2E로 강제하고 `Plain`을 제거한다. 에이전트 제어 기본값을 OFF로 바꾼다. PIN 실패 예산을 디바이스·소스별로 나누고 영속화한다. CSP에서 `unsafe-eval`과 dev origin을 제거한다. 보안 백서와 데이터 보존 정책을 공개한다.

---

## 6. 가격·운영: 살 수 없는 유료 플랜, Free 1대, 단일 호스트 릴레이 — 높음

**증거.**
- `site/src/content/docs/docs/pricing.mdx:25` — "Only the Free plan is available today… checkout is not open yet." (SOURCE)
- Free는 머신 1대뿐이고, 두 번째 등록은 `PLAN_LIMIT_REACHED`로 막힌다 (`docs/billing-operations.md:45, 467`). 유료 결제는 `403 PLAN_COMING_SOON` (`billing-operations.md:100`). 결과적으로 호스티드 릴레이로 원격 머신 2대 이상을 쓸 합법적인 방법이 셀프호스트밖에 없다. (SOURCE)
- 결제가 실패하면 7일 유예 뒤 모든 계정 기반 원격 연결이 끊긴다 (`pricing.mdx:82-87`). (SOURCE)
- 가격 모델이 여러 번 뒤집혔다. 30일 체험과 Paddle 계획(09-25)이 영구 Free와 Lemon Squeezy(09-30)로 바뀌었다 (`$MEM/facts/ferryx-commercial-model-plan-2026-09-25.md`, `ferryx-relay-account-gating-and-billing-architecture.md`). (SOURCE)
- 프로덕션 릴레이는 단일 호스트 `omarchy`의 cloudflared 터널 뒤에서 dev 프로파일 빌드로 돈다. 릴레이 자체는 TLS가 없고, 한국 사용자는 HKG를 경유해 p50 133 ms다 (직결 19 ms) (`$MEM/facts/ferryx-relay-monetization-free-only-launch-deploy-2026-10-22.md`, `ferryx-relay-production-hosting-research-2026-10-22.md`). SLA와 상태 페이지가 없다. (SOURCE/INFERRED)
- 도메인을 이미 한 번 이전했다(`relay.checka.cc`는 폐기). (SOURCE)
- 지원 채널은 Discord와 GitHub뿐이다 (`README.md:17`). 이메일 지원, DPA, 엔터프라이즈 계약이 없다. (SOURCE/INFERRED)

**반론.** 로컬 사용과 셀프호스트는 무제한 무료이고, 예정 가격도 싸다(Pro $5, Team 좌석당 $7).
**무력화하려면.** 결제를 열거나 Free 머신 한도를 올린다. 릴레이를 이중화하고 상태 페이지를 둔다. 지원 SLA 티어를 명시한다.

---

## 7. 설치·권한·플랫폼 마찰 — 높음

**증거.**
- Windows 설치 파일이 서명되지 않아 SmartScreen 경고가 뜬다 (`README.md:34`). Microsoft Store 경로는 수동 제출이고 API 파이프라인은 막혀 있다 (`$MEM/facts/ferryx-msstore-api-credentials-blocked-no-entra-tenant.md`). (SOURCE)
- Windows와 Linux는 x64 전용이다 (`README.md:34-37`, `scripts/install.ps1:259-260`). Snapdragon 노트북, Graviton/Ampere 서버, 라즈베리파이는 제외된다. (SOURCE)
- `scripts/install.sh:150-164`는 이미 제거된 `ferryx-cli-linux-arm64`를 아직 요청해서 다운로드 실패로 끝난다. 반면 `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:45`는 aarch64를 지원한다고 쓴다. (SOURCE)
- "헤드리스" CLI가 Linux에서 GTK 3와 WebKitGTK 4.1을 링크한다 (`README.md:103`, `src-tauri/Cargo.toml:247-249`). 최소 VPS 이미지에 데스크톱 스택을 끌어와야 한다. (SOURCE)
- GPU가 없으면 네이티브 터미널 패널이 빈 화면이 된다. 오류 문자열은 `src-tauri/src/native_terminal/renderer/gpu_context.rs:176`, 재현 기록은 `$MEM/facts/omarchy-ferryx-native-terminal-no-gpu-blank-pane-2026-10-16.md`. VM, RDP, 구형 GPU가 위험하다. (MEASURED)
- macOS 첫 실행에서 전체 디스크 접근, 손쉬운 사용, 알림 권한을 요구한다 (`docs/PERMISSIONS_ONBOARDING_2026-09-06.md`). TCC 프롬프트는 수렴하지 않는다 ("Prompts do not converge", `docs/macos-tcc-permissions.md:202`). 터미널에 전체 디스크 접근을 주는 것은 보안팀이 거부한다. (SOURCE)
- 원격 머신 연결은 계정 로그인이 필수다 (`README.md:115-116`, `install.sh:347-385` 자동 `ferryx-cli up`). (SOURCE)
- CLI 배포가 `curl | bash` 하나다. 서명·체크섬 검증이 없고 Homebrew/winget/apt도 없다 (`README.md:37, 109`). (SOURCE/INFERRED)
- 사이트 설치 페이지의 Gatekeeper·SmartScreen 섹션이 템플릿 문구다 (`site/src/content/docs/blog/ferryx-install-macos.md:14-16`, `ferryx-install-windows.md:14-16`). (SOURCE)

**반론.** macOS는 notarize된 유니버설 DMG이고, NSIS는 per-user 설치라 관리자 권한이 필요 없으며, 소스 빌드 사용자는 소수다.
**무력화하려면.** Windows 코드 서명을 적용한다. install.sh의 arm64 분기를 정리한다. CLI에서 GTK 의존성을 분리한다. 소프트웨어 렌더 fallback을 넣는다. brew/winget을 제공한다. 권한은 처음에 다 받지 말고 필요할 때 요청한다.

---

## 8. 레포에 강제되는 워크트리 레이아웃과 레거시 이름 — 높음

**증거.**
- 워크트리는 `<repo>/.orca-worktrees/<ws>/<slug>`, 브랜치는 `orca/<ws>/<slug>`로 고정이다 (`src-tauri/src/worktree/manager.rs:15, 382`). `.git/info/exclude` 처리가 없다 (manager.rs grep 결과 0건). 그래서 `git status`, 에디터 인덱싱, 도구 glob에 노출되고, 경쟁 제품 이름("orca")이 붙은 브랜치가 PR로 올라간다. (SOURCE/INFERRED)
- 경로 규칙이 세 가지로 불일치한다. 로컬은 `<ws>/<slug>`, SSH는 `.orca-worktrees/wt-{slug}` (`src-tauri/src/ssh/worktree.rs:95`), scope 헬퍼는 `orca/{slug}` 브랜치에 `wt-{slug}` 경로다 (`src-tauri/src/ferryx_scope/ssh/helper.rs:298, 311`). (SOURCE)
- 저장소 전반에 "orca" 4,731회, "rorca" 348회가 남아 있다. 데몬 소켓은 `/tmp/rorca-{uid}/daemon.sock`이다. 미완성 리브랜드나 포크처럼 보인다. (MEASURED)

**반론.** root jail 모델이 안전하고, 기존 워크트리 가져오기(`ExternalWorktreeImportDialog`)가 있다.
**무력화하려면.** `info/exclude` 자동 등록, 브랜치 접두사 설정 옵션, 세 레이아웃 통합, 사용자에게 보이는 orca/rorca 이름 마이그레이션.

---

## 9. 개념·기능 과다, 단축키 충돌, 터미널 충실도 — 중상

**증거.**
- 사용자가 익혀야 할 개념이 16개를 넘는다. workspace, worktree, tab, pane, session, machine, SSH host, paired daemon, relay, PIN, inbox, supervisor, DAG viewer, browser, mobile chat, plan 등이다 (AGENTS.md, `ui/src/components/` 구조). (SOURCE)
- 설정 섹션 11개 (`ui/src/components/settings/types.ts:4-15`), 설정 코드 약 16,251줄, 영속 키 29개 (`ui/src/lib/storageKeys.ts`). (MEASURED)
- 기능 확장: 임베디드 브라우저와 디자인 모드, DAG 뷰어, 슈퍼바이저, 허브(Slack/cron), 과금 UI. 루트의 `PARITY.md`는 Slack/iMessage/Kakao 패리티를 다룬다. 사용자 불만 1순위인 "터미널 안에 브라우저와 로그인 버튼"과 정확히 겹친다 (`docs/research/cmux-orca-herdr-user-complaints-2026-10-14.md:59, 624-626`). (SOURCE)
- 비-Mac에서 터미널에 포커스가 있어도 앱이 Ctrl+digit, Ctrl+Tab, Ctrl+PageUp/Down을 가로챈다 (`ui/src/lib/shortcuts.ts:760`). tmux, readline, vim 사용자의 습관을 깬다. 키보드 copy mode가 없고 테마가 항상 다크다 (`docs/research/ferryx-vs-cmux-orca-herdr-complaint-gaps-2026-10-14.md` themes 4-6). (SOURCE)
- 렌더러는 Ghostty 렌더러가 아니라 자체 WGPU 렌더러다. 글리프 아틀라스 손상, 디스플레이 전환 손상, 오버레이 순서 문제 등 docs/에 렌더 관련 버그 문서가 22개 있다. (MEASURED)

**반론.** 대부분 opt-in이라 로컬 사용자는 workspace, tab, pane만 본다. libghostty와 Ghostty 설정 가져오기로 전환 비용이 낮다.
**무력화하려면.** 점진적 공개(브라우저, DAG, 슈퍼바이저, 원격, 플랜은 숨김 기본), "tmux-safe" 단축키 프리셋, copy mode, 시스템 테마 추종.

---

## 10. 공개 주장과 실제의 불일치, 사용자 문서 부재, 저장소 위생 — 중상

**증거.**
- 사이트는 Gemini CLI를 지원 에이전트로 광고한다 (`site/src/components/landing/AgentSupport.astro:27`, `site/src/lib/locale.ts:28` "Gemini Flash", 콘텐츠 파일 약 24개). 실제 매니페스트 16개 중 Gemini는 없고, Google 에이전트는 `agy`(Antigravity)다 (`src-tauri/src/agent_detect/manifests/`, `ui/src/lib/agentResume.ts:31`). (SOURCE/MEASURED)
- 사이트는 에이전트 수를 11개라고 하고 README는 16개라고 한다 (`AgentSupport.astro:2-15`, `PainPoints.astro:64` vs `README.md:65-66`). (SOURCE)
- README는 "QR or PIN" 페어링을 광고하지만 (`README.md:84, 152`, `README.zh-CN.md:80, 140`) `ui/src`에는 QR 구현이 0건이다. QR 방식은 폐기됐다 (`$MEM/facts/ferryx-qr-pairing-method-discarded-2026-10-16.md`). (MEASURED)
- 관리형 채팅 전송은 여전히 Codex에 하드와이어돼 있다 (`src-tauri/src/ferryx_scope/chat/mod.rs:25, 62`). (SOURCE)
- "Agent Support Matrix" 블로그 글에 표가 없다 (`site/src/content/docs/blog/agent-support-matrix.md:10-40`). 같은 템플릿 문장이 14개 파일에 반복돼 SEO 필러로 보인다. (SOURCE)
- 루트에 CHANGELOG, SECURITY, CONTRIBUTING, FAQ, 트러블슈팅 문서가 없다. 사이트 docs는 6페이지뿐이다. (MEASURED)
- `docs/`는 164개 항목의 내부 엔지니어링 로그다(`*_ROOT_CAUSE_*`, `*_FIX_*`, 스토어 제출 팩 등). (MEASURED)
- 내부 산출물이 git에 추적된다. `.omo/` 1,579개 파일, `PARITY.md`, `recovery-final-ssh-rust-repair-20261014.md`, `task-2/*.log` (`git ls-files`). 평가자에게는 방치된 에이전트 작업 덤프로 보인다. (MEASURED)

**반론.** 사이트는 "any command runs in a pane"이라고 하니 Gemini도 실행은 된다. 엔지니어링 기록의 투명성은 엄밀함의 신호일 수도 있다.
**무력화하려면.** 매니페스트를 단일 출처로 삼아 사이트 에이전트 목록을 자동 생성하고, QR 문구를 삭제한다. 내부 산출물을 untrack하고 gitignore한다. 사용자 가이드, 트러블슈팅 인덱스, CHANGELOG를 만든다.

---

## 11. 메모리·디스크·지연 비용 — 중간

**증거.**
- wgpu 30 이전 실측: footprint 5,937 MB(피크 7,494 MB), 그중 IOSurface 89%. 비누수 기준선도 약 922 MB였다 (`docs/FERRYX_MEMORY_BASELINE_2026-09-05.md`). 수정 커밋 `29ea50be` 이후의 soak 측정은 없다. (MEASURED)
- 워크트리마다 `node_modules`와 `target`이 복제되고, 공유·중복 제거는 범위 밖으로 명시돼 있다 (`docs/FERRYX_WORKTREE_DISK_SCOPE_2026-09-12.md` §7). 에이전트 N개면 빌드 산출물도 N벌이다. (SOURCE)
- 설치 용량: 앱 179 MB에 `daemon-runtime` 169 MB(버전 3개 보관)가 더해진다. "Tauri라 작다"는 기대와 어긋난다. (MEASURED)
- 시작 지연: 스테이플링 미적용 Gatekeeper 온라인 검사 3~8초 (`docs/RELEASE_APP_INITIAL_LOAD_LATENCY_ROOT_CAUSE.md` §1). (MEASURED)
- 입력 지연 구조: SSH 경로는 순차 게이트 때문에 처리량이 1/RTT로 제한된다(RTT 200 ms면 초당 5키) (`$MEM/ferryx-terminal-typing-drop-and-latency-causes.md`). (SOURCE)
- 세션별 고정 버퍼: 링 512 KiB, 보존 512 KiB, 미러 2,000줄 (`src-tauri/src/terminal/output_hub.rs:8`, `retained_history.rs:12`, `screen_mirror.rs:15`). TUI 세션의 스크롤백은 413줄밖에 안 된다. (SOURCE/MEASURED)

**반론.** 현재 GUI RSS는 약 255 MB로 관측된다. fd 한도는 수정됐다 (`src-tauri/src/lib.rs:71, 96, 130`).
**무력화하려면.** 릴리스별 soak와 footprint 벤치를 공개한다. 워크트리 간 의존성 공유(심볼릭 링크나 캐시) 옵션과 오래된 runtime 정리를 넣는다.

---

## 반대편 결론 — 그래도 Ferryx를 쓸 사람

다음 조건이 겹치는 사용자에게만 지금 Ferryx가 합리적이다.
1. **여러 벤더**의 CLI 에이전트를 **한 레포에서 동시에** 돌린다.
2. 에이전트가 **GUI 종료와 업데이트를 견디기**를 원하고, 원격 리눅스 머신을 SSH 포트 없이 붙이고 싶다.
3. 폰 원격 조작을 **벤더 구독이나 벤더 서버 없이**(셀프호스트 릴레이) 하고 싶다.
4. 개인 또는 사내 사용이고, 초기 소프트웨어의 회귀를 감수할 수 있다.

이 교집합 밖의 사용자, 즉 단일 벤더 사용자, 엄격한 보안·구매 절차가 있는 회사, tmux 파워유저, ARM/GPU 없는 환경, 원격 머신 2대 이상을 호스티드로 쓰려는 사람에게 위 반대 논리는 현재 증거로 반박되지 않는다.

## 가장 싸고 효과가 큰 수정 순서

1. 카피 정정 (몇 시간): `locale.ts:28` "guarantee" 삭제, Gemini를 agy로, 11을 16으로, QR 문구 삭제, "free and open"을 "source-available"로.
2. 저장소 위생 (몇 시간): `.omo/`, `task-2/`, `PARITY.md`, `recovery-*.md` untrack, CHANGELOG 추가.
3. 레포 침범 최소화 (하루): `.orca-worktrees` info/exclude 자동 등록, 브랜치 접두사 설정.
4. 보안 기본값 (며칠): 에이전트 제어 기본 OFF, CSP `unsafe-eval`·dev origin 제거, 평문 릴레이 경로 제거.
5. 신뢰 신호 (지속): 세션 생존 회귀 시나리오 결과를 릴리스마다 공개, Windows 서명, CI 릴리스와 provenance, cmux/Orca/herdr 비교 공개.

## 이 보고서의 한계

- 외부 경쟁사 기능 상태는 저장소 문서의 2026-09-19 조사에 의존하며 이번에 재조회하지 않았다.
- 릴레이 평문 경로가 현재 프로덕션 트래픽에서 실제로 쓰이는지(`opaque=false` 호출자)는 확정하지 않았다.
- c5049bf9(Dock quit)와 f1730fa3(재부팅) 수정의 실효성은 측정하지 않았다. 이 보고서는 "재측정 기록이 없다"는 사실만 주장한다.
- 성능 수치 다수는 wgpu 30 이전 측정이다.
