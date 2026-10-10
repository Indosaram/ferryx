# T3 Code 대비 Ferryx 모바일 리모트 UI/UX 정직한 격차 분석

작성: 2026-09-25 · 근거: T3 Code App Store 스크린샷 5장 + Ferryx 현재 코드 실측

### 레퍼런스 출처 (재현 가능)
App Store 페이지: <https://apps.apple.com/us/app/t3-code-remote-claude-more/id6787819824>

아래 스크린샷을 내려받아 대조했다 (`600x1300bb.webp`). `/tmp`는 휘발성이므로 URL을 남긴다:

| 파일 | 화면 | URL |
|---|---|---|
| `threads.webp` | 스레드 목록 (진입 화면) | `https://is1-ssl.mzstatic.com/image/thumb/PurpleSource221/v4/dc/57/b3/dc57b34b-1a15-8901-d156-4c7067e06a9d/threads.png/600x1300bb.webp` |
| `thread.webp` | 스레드 대화 (사용자 버블 + `Worked for 2m`) | `https://is1-ssl.mzstatic.com/image/thumb/PurpleSource211/v4/b8/e3/88/b8e388bb-0874-c100-9c63-9cb9a6b2b193/thread.png/600x1300bb.webp` |
| `terminal.webp` | 터미널 (전체 폭 PTY) | `https://is1-ssl.mzstatic.com/image/thumb/PurpleSource221/v4/5a/09/4d/5a094d3c-3a10-eb11-adb6-e40aa30a170e/terminal.png/600x1300bb.webp` |
| `environments.webp` | 환경(머신) 목록 | `https://is1-ssl.mzstatic.com/image/thumb/PurpleSource211/v4/fb/d3/cf/fbd3cfb9-d6b9-324a-b6c8-10cef1c47722/environments.png/600x1300bb.webp` |
| `review.webp` | 리뷰/디프 화면 | `https://is1-ssl.mzstatic.com/image/thumb/PurpleSource221/v4/a7/9e/7d/a79e7df1-02c6-6152-292e-28c1331d3a7e/review.png/600x1300bb.webp` |

## 결론 (한 줄)
**아니요. 현재 상태는 T3 Code 수준이 아닙니다.** 직전 작업은 "AI slop 제거"에 그쳤고,
T3 Code와 비교하면 **정보 구조(IA) 자체가 다릅니다.** 게다가 채팅 스트림에는 **텍스트로 파싱되지 않는
바이너리 프레임을 그대로 붙여넣는 실제 버그**가 있어, 채팅 화면은 원시 이스케이프 문자열을 출력합니다.

---

## 1. T3 Code 레퍼런스 (App Store 스크린샷에서 직접 확인)

### 1-1. 진입 화면 = 스레드 목록 (인사말 아님)
| 요소 | T3 Code |
|---|---|
| 행 구성 | 작은 저장소 아바타 타일 + 저장소명(작은 대문자) + 우측 상대시간 / 굵은 한 줄 제목 / **모노스페이스 메타** "branch · machine" / 우측 프로바이더 글리프 |
| 상태 라벨 | 행 우측 정렬: `Working`, `Approval`, `Sends on reconnect` |
| 그룹 | `Unsent`, `Snoozed (2)`, `Settled (3)` — 조용한 섹션 헤더 + 접기 |
| 하단 바 | 메뉴 버튼 + 넓은 `Search` 필 + 컴포즈 버튼 |

### 1-2. 스레드 화면 (대화)
- 사용자 메시지: **우측 정렬 + 액센트(파랑) 라운드 버블**
- 에이전트 작업: `Worked for 2m ›` **접히는 한 줄**로 압축 (기본 접힘)
- 어시스턴트 출력: **버블 없음. 배경 위 순수 산문** (아바타 없음, 테두리 없음)
- 턴 하단: 작은 모노스페이스 타임스탬프 + 복사 글리프
- 컴포저: 한 줄 — 선행 `+`, 플레이스홀더 "Ask the repo agent, or run a com…", 마이크, **원형 위쪽 화살표 전송**

### 1-3. 터미널 화면
- 헤더: 뒤로 화살표 + `Terminal` 제목 + 저장소 서브타이틀 + 터미널 아이콘 버튼
- 본문: **전체 폭 원시 PTY 출력** (크롬 없음)
- 우하단: 작은 플로팅 키보드 버튼

---

## 2. Ferryx 현재 상태 (코드 실측)

### 2-1. 채팅 스트림이 원시 이스케이프를 출력 (실제 버그)
백엔드 `src-tauri/src/remote/server.rs`의 `encode_remote_terminal_frame()`은 바이너리 프레임을
OSC 엔벨로프로 감싸 보냅니다:

```
"]777;ferryx;" + JSON({kind:"output"|"replay"|"replayGap", sequence, ...}) + "" + ("c"?) + payload
```

그런데 `ui/src/remote/RemoteApp.tsx`의 채팅 `handleMessage`는 이렇게 처리합니다:

```ts
let displayText = raw;
try {
  const parsed = JSON.parse(raw);
  if (parsed?.type === "output" && typeof parsed.data === "string") displayText = parsed.data;
  else if (parsed?.type === "agentTurn" || parsed?.type === "toolOutput") ...
  else if (parsed?.type === "grid" || parsed?.type === "remoteStatus") return;
} catch { /* plain text */ }
```

엔벨로프가 붙은 문자열은 `JSON.parse`가 **항상 실패**하므로 `catch`로 떨어져 **원시 엔벨로프가 그대로 버블에 들어갑니다.**
기대하는 `{type:"output", data}` / `agentTurn` / `toolOutput` 형태는 **백엔드 어디에서도 생성되지 않습니다.**

재현 결과(엔코더 재구현 → RemoteApp 디코드 로직 그대로 실행):
```
JSON.parse succeeded? false
bubble = "\u001b]777;ferryx;{\"kind\":\"output\",\"sequence\":\"1042\"}\u0007\u001b[2J\u001b[H\u001b[38;5;214m✻ Thinking…\u001b[0m\r\n\u001b[1m> \u001b[0mRefactor the remote chat"
```

**파생 문제 4가지**
1. TUI 리페인트(`\x1b[2J\x1b[H…`)마다 청크가 **상한 없이 누적** → 버블 무한 성장
2. `\r` 기반 인플레이스 진행 표시가 **합쳐지지 않음** → 중복 부분 프레임이 쌓임
3. 제출한 프롬프트의 **PTY 에코가 어시스턴트 버블에 붙어** 사용자 텍스트가 중복됨
4. 실제 `remoteStatus` generation 프레임을 **인식하지 못해** 이 소켓에서는 리사이즈 핸드셰이크가 안 돎

> Terminal 뷰가 멀쩡했던 이유: `RemoteTerminal`은 `?render=grid`로 **다른 렌더 경로**를 타고 `parseGridFrame`이 처리합니다.

### 2-2. IA(정보 구조)가 T3 Code와 다름
| 항목 | T3 Code | Ferryx 현재 |
|---|---|---|
| 진입 화면 | 스레드 목록(작업 큐) | 단일 대화 + 빈 상태 |
| 세션/스레드 전환 | 목록에서 선택 | 상단 터미널 탭 스트립 |
| 어시스턴트 턴 | 버블 없음, 순수 산문 | `rounded-2xl` 버블 + `Bot` 아바타 |
| 에이전트 작업 | `Worked for 2m` 접힘 행 | 평평하게 노출 |
| 상태 표현 | `Working`/`Approval`/`Sends on reconnect` | `ActivityIndicator`만 |
| 컴포저 | `+` / 한 줄 / 마이크 / 원형 전송 | 키캡 로우 + 칩 로우 + 전송 |

### 2-3. 직전 작업에서 실제로 제거된 것
- 거대한 `sky-500` 그라디언트 `Bot` 아바타 → `Code2` 아이콘 카드
- `How can I help you today?` 주변 소비자 카피 정리
- 프롬프트 카드 → 모노스페이스 숏컷 스타일

### 2-4. 직전 작업에서 **남아 있던** slop (수정 전 스냅샷 — 아래 4절에서 전부 처리됨)
- `DEFAULT_STARTER_PROMPTS` 3종("What's the status of this worktree?", "Show git diff", "Help me debug")이
  **여전히 기본 렌더 경로**이고 테스트가 이를 고정하고 있음
- 어시스턴트 턴에 `rounded-2xl` 버블 + `Bot` 아바타 + `sky-950/80` 테두리가 그대로
- `MobileChatQuickActions`의 `DEFAULT_QUICK_ACTIONS`("Git status", "Run tests", "Explain", "Review diff", "Stop")
  칩 로우가 그대로 — T3 Code에는 없는 장식
- `Sparkles` 아이콘 임포트가 quick actions에 잔존
- 하드코딩된 `zinc-*` 팔레트가 테마 토큰(`bg-background`, `text-foreground`)과 혼재

---

## 3. 격차 해소에 필요한 작업 (우선순위)

**P0 — 채팅이 실제로 동작하게 만들기**
1. `ui/src/remote/remoteTerminalFrames.ts`: OSC 엔벨로프 디코더 + 제어 시퀀스 스트리퍼
2. `RemoteApp` `handleMessage`를 디코더 기반으로 교체, `replayGap` 무시
3. 누적 상한 + 중복 청크 가드 + 사용자 에코 억제

**P1 — T3 Code IA로 재구성**
4. 어시스턴트 턴: 버블/아바타 제거, 배경 위 산문
5. `Worked for <duration>` 접힘 행 추가
6. 사용자 버블을 액센트 토큰으로 전환
7. `DEFAULT_STARTER_PROMPTS`와 인사말 삭제, 빈 상태를 조용한 컨텍스트 패널로
8. 컴포저를 T3 한 줄 형태(`+` / 텍스트 / 마이크 / 원형 전송)로 축소

**P2 — 검증**
9. 채팅 테스트를 산문이 아니라 **구조**로 검증하도록 재작성
10. 390x844 뷰포트 실제 렌더 스크린샷으로 T3 레퍼런스와 육안 대조

---

## 4. P0/P1 처리 결과 (2026-09-25 완료)

### P0 — 채팅 프레임 디코딩 (수정 완료)
신규 `ui/src/remote/remoteTerminalFrames.ts` + `RemoteApp.tsx` `handleMessage` 교체.

- 엔벨로프(`\x1b]777;ferryx;` + JSON + `\x07`)를 먼저 디코드, `replayGap` 무시
- CSI/OSC/`\r` 제어 시퀀스 제거, 빈 청크 early-return
- 제출 프롬프트의 PTY 에코 억제(`lastSubmittedPromptRef`), 중복 tail append 스킵
- 어시스턴트 버블 12000자 상한(초과 시 앞에서 트림)

**증거**
| 항목 | 결과 |
|---|---|
| `remoteTerminalFrames.test.ts` | 10/10 통과 |
| `RemoteUI.test.tsx` | 50/50 통과 |
| 백엔드 인코더 재구현 대조 | `"✻ Thinking…\n> "` — 이스케이프 0바이트 |
| 뮤테이션 (디코더 항상 null) | 3 failed → 복원 GREEN |
| 뮤테이션 (CSI 제거 비활성) | 1 failed → 복원 GREEN |

### P1 — T3 IA 재구성 (수정 완료)
`ui/src/remote/chat/` 4개 컴포넌트 + 3개 테스트.

- 사용자 턴: 우측 정렬 `bg-primary` 액센트 버블
- 어시스턴트 턴: **버블·아바타 제거**, 배경 위 산문 (`assistant-message-body`)
- `Worked for <duration>` 접힘 행 (기본 접힘)
- `DEFAULT_STARTER_PROMPTS`·인사말·`Sparkles` 삭제, 빈 상태를 조용한 컨텍스트 패널로
- 컴포저 단일 행(`+` / 텍스트 / 마이크 / 원형 전송→정지)

**증거**
| 항목 | 결과 |
|---|---|
| `src/remote/chat/` | 29/29 통과 (12 + 10 + 7) |
| `src/remote/` 전체 | **437/437 통과**, 31개 파일 |
| 뮤테이션 (접힘 계약 파괴) | 1 failed → 복원 GREEN |
| 뮤테이션 (액센트 버블 되돌림) | 1 failed → 복원 GREEN |
| 프로덕션 빌드 | tsc + vite exit 0 |
| 번들 검증 | `777;ferryx;` / `replayGap` / `Worked for` 존재, 인사말 0건 |
| 앱 동기화 | `/Applications/Ferryx.app/.../ui/dist/` 교체, 버전 `2026.922.1` 유지 |

### 부수적으로 잡은 결함
`MobileChatMessage.test.tsx`에 `cleanup()`이 없어 DOM이 누적됐다(다른 두 chat 테스트 파일에는 있음).
그 탓에 새 copy-button 테스트 2건이 이전 렌더 잔여물을 매치해 실패했다. `afterEach(cleanup)` 추가로 해결.

---

## 5. P3 — 스레드 목록 진입 화면 (2026-09-25 완료)

4절의 "아직 남은 더 깊은 IA 격차" 중 **진입 화면**을 실제로 구현했다.

### 신규
- `ui/src/remote/chat/MobileChatThreadList.tsx` — T3 스레드 목록
  - 행: 사각 글리프 타일(`Terminal` 아이콘) + 굵은 한 줄 제목 + 모노스페이스 `<worktree> · <agent>` 메타 + 우측 상태/상대시간
  - 상태 라벨: `working` → `Working`(`text-status-working`), `waiting` → `Approval`(`text-status-warning`),
    `done` → `Done`(`text-status-success`), `data-status` 속성 부여
  - worktree별 그룹 헤더(`이름 · 개수`), 활성 행은 `bg-accent/40` + 우측 `Check`
  - 하단 고정 검색 필 — **실제로 필터링한다**(제목 + 메타 + 상태 라벨, 대소문자 무시).
    검색 중에는 그룹 헤더를 숨기고 평면 목록으로 전환, 무매치 시 `No matching threads`
- `ui/src/remote/chat/MobileChatThreadList.test.tsx` — 7개 케이스

### 수정
- `ui/src/remote/RemoteApp.tsx`
  - `viewMode` 타입에 `"threads"` 추가. 기본값은
    `typeof window !== "undefined" && window.innerWidth > 0 && window.innerWidth < 768 ? "threads" : "terminal"`.
    **`innerWidth > 0` 가드는 필수** — JSDOM은 `innerWidth === 0`이라 기존 테스트가 terminal을 기대한다.
  - 헤더에 `remote-view-mode-threads` 버튼(Chat 버튼 앞), `<RemoteWorkspaceMirror>` 자식에 첫 번째 분기로
    threads 브랜치 추가. 행은 `terminalTabs`에서 파생, 비어 있으면 `options`로 폴백.
  - 행 선택 시 해당 탭으로 `selectContext` 후 `chat` 뷰로 전환

### 검증
| 항목 | 결과 |
|---|---|
| `MobileChatThreadList.test.tsx` | 7/7 |
| `src/remote/chat/` | 36/36 |
| `src/remote/` 전체 | **444/444**, 32개 파일 |
| 뮤테이션 (검색 필터 무력화) | 2 failed → 복원 GREEN |
| 뮤테이션 (활성 행 마커 제거) | 1 failed → 복원 GREEN |
| `tsc --noEmit` | exit 0 (테스트 파일은 tsconfig에서 제외됨) |
| 프로덕션 빌드 | exit 0 |
| 번들 검증 | `remote-view-mode-threads` / `thread-search-input` / `777;ferryx;` 모두 존재 |
| 앱 동기화 | 완료, 버전 `2026.922.1` 유지 |

### 여전히 남은 것
- 턴 단위 상태 모델(`Sends on reconnect` 등)과 별도 environments 화면은 미구현
- 스크린샷 육안 검증은 이 워크스테이션에서 불가(5절 참조). 모바일 진입 화면이 스레드 목록이라는
  사실은 통합 테스트(`RemoteApp.threadsEntry.test.tsx`)로 검증한다.

---

## 4. 아직 덮지 않은 더 깊은 IA 격차 (이번 범위 밖)

P1은 **스레드 화면 내부**만 T3에 맞춘다. 다음은 여전히 남는다:

- **진입 화면**: T3는 스레드 목록(작업 큐)이 첫 화면이다. Ferryx는 단일 대화 화면으로 바로 들어간다.
  Ferryx에서 이걸 맞추려면 세션/스레드 인덱스 + 그룹(`Unsent`/`Snoozed`/`Settled`) + 검색이 필요하고,
  백엔드에 스레드 개념이 없다면 서버 변경이 선행돼야 한다.
- **상태 라벨**: T3의 `Working`/`Approval`/`Sends on reconnect` 같은 턴 단위 상태 모델.
  Ferryx에는 `ActivityIndicator`(thinking/running_tool/waiting_for_input)만 있다.
- **환경(머신) 화면**: T3는 별도 environments 화면을 갖는다. Ferryx는 `MobileHostDrawer`가 부분적으로 담당.

## 5. 검증 환경 제약 (실측)

이 워크스테이션에서는 실제 렌더 스크린샷을 찍을 수 없다. 둘 다 실패:

- `Bun.WebView`: `new Bun.WebView(...)` 생성 후 navigate 단계에서 무응답 (WebKit GUI 세션 없음)
- Playwright: 번들된 `chromium_headless_shell-1193` 부재. 존재하는 `chromium_headless_shell-1243`로
  실행하면 `FATAL:base/apple/mach_port_rendezvous_mac.cc:159 ... unknown error code (141)`로 SIGTRAP.
  시스템 Chrome(`channel: "chrome"`)도 동일하게 실패.

따라서 육안 대조 대신 **T3 레퍼런스 이미지 + 코드 레벨 검증**으로만 확인했다.
스크린샷 기반 검증이 필요하면 GUI 세션이 있는 환경에서 별도로 돌려야 한다.

