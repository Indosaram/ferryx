# Ferryx Attention Surface — 리서치 결과 및 설계 권고

**작성:** 2026-09-25 · **상태:** 리서치 완료, 설계 결정 대기 · **선행 문서:** `FERRYX_ATTENTION_INBOX_SCOPE_2026-09-12.md`

## 0. 결론 (Bottom line)

1. **지금 배선된 `AttentionInboxDialog`는 폐기 대상이다.** 별도 모달 + 호버 전용 진입점은 이번 리서치에서 확인된 **안티패턴 3종을 동시에** 밟고 있다: (a) 이원화된 durable 표면, (b) 호버 전용 진입점, (c) 키보드 경로 부재.
2. **정답은 "알림 센터를 확장해 하나의 durable Attention 표면으로 통합"** 이다. 새 표면을 만들지 않는다. 기존 `notificationCenterStore`가 이미 durable 저장·영속·읽음 상태·mark all read를 갖고 있으므로, 여기에 **섹션·필터·배지 의미·키보드 드레인**을 얹는다.
3. **transient(토스트/OS 알림)와 durable(목록)은 분리하되, durable이 단일 진실이다.** 조사한 8개 도구 전부 두 계층을 운영하며, **혼합 리스트는 없다**. 토스트는 durable 항목의 투영일 뿐 별도 저장소가 아니다.
4. **T3 Code에는 인박스 화면이 없다 — 사이드바가 인박스다.** 섹션은 `pinned → active(inbox) → snoozed → settled`, 상태는 행별 pill이며, 알림은 별도 transient 계층이고 읽음 상태는 **세 번째 독립 시스템**이다. 즉 "인박스"를 별도 화면으로 만들지 않는다는 결정 자체가 선례와 일치한다.
5. **가장 강한 사용자 불만은 "다음 것"으로 가는 키보드 경로 부재다.** Orca #12577("네이티브 알림 클릭이 유일한 도달 경로"), Herdr #318(정렬이 attention 우선이 아님) → 커뮤니티 플러그인 `herdr-attention` 등장. 이 지점이 실제 사용성의 핵심이다.

---

## 1. 현재 구현 감사 (증거)

### 1.1 이미 있는 것

| 구성 | 위치 | 상태 |
|---|---|---|
| durable 저장소 + 영속 | `ui/src/lib/notificationCenter/notificationCenterStore.ts`, `notificationCenterPersistence.ts` | 동작. `InboxState{version, nextUpdateOrder, entries}` |
| 항목 스키마 | `notificationCenter/types.ts` | `NotificationEntry{ id, workspaceId, sessionId, labels, subject: agent\|terminal, reason: waiting\|done\|bell, firstOccurredAt, lastOccurredAt, updateOrder, revision, occurrenceCount, read: {unread}\|{seen,seenAt} }` |
| 읽음 의미 | 동일 | `markEntriesRead` / `markAllRead` / `dismissEntry` / `clearAll` |
| 표면 | `components/notification/NotificationCenterPopover.tsx` | 팝오버, backdrop, 빈 상태, 행별 상태 dot, mark-all-read |
| 배지 카운트 | `components/notification/useNotificationCenter.ts` | `unreadCount = entries.filter(unread)` |
| 진입점 | `NotificationCenterButton.tsx` | 사이드바에 **상시 노출** (호버 불필요) |
| 에이전트 인벤토리(원격) | `ferryx_scope/control/` | 크레이트 밖, 게이트웨이 미연결 |
| 데스크탑 인박스(신규) | `components/AttentionInboxDialog.tsx` + `features/ferryx/control/desktopInventory.ts` | 동작하나 **이원화·호버 진입·무단축키** |

### 1.2 신규 인박스의 실제 동작 (한계 포함)

- 행 생성 조건이 좁다: `if (!activity) continue` — 활동 기록이 있는 세션만.
- 표시 필터가 좁다: `rows = items.filter(a => a.state === "waiting" || isUnread(a))`, `isUnread = done && !seen` → **working은 안 보인다**.
- 진입점: 프로젝트 행 호버 시 `Bell` 아이콘(`opacity-0 group-hover/project:opacity-100`) 또는 우클릭 네이티브 메뉴. **단축키·명령 팔레트 없음**(`shortcuts.ts`·`CommandPalette` 검색 결과 0).
- 원격 프로젝트에도 열린다(게이트 없음). 상태를 못 읽으면 행 대신 `Inventory incomplete: <호스트>`.

### 1.3 이 구현이 안티패턴인 이유 (리서치 근거)

| 안티패턴 | 출처 | 현재 구현 |
|---|---|---|
| 이원화된 durable 목록 | Linear·Slack·GitHub·VS Code·JetBrains·macOS 전부 **단일 canonical 목록** | 알림 센터 + 인박스 모달 2개 |
| 호버 전용 진입점 | 조사한 8개 도구 **전부 상시 노출**(사이드바 배지·벨·nav 아이콘) | `opacity-0` 호버 전용 |
| 키보드 경로 부재 | Orca [#12577](https://github.com/stablyai/orca/issues/12577), Herdr [#318](https://github.com/herdrdev/herdr/issues/318) + `herdr-attention` 플러그인 | 단축키 없음 |
| 기계 생성 작업의 소음 | Orca [#22181](https://github.com/stablyai/orca/issues/22181): 활성 워크트리 23개 중 15개가 CLI/자동화 생성 → 전부 배너 | provenance 구분 없음 |
| 상태 글리프 모호 | Orca [#229](https://github.com/stablyai/orca/issues/229): 초록 점을 "unread"로 오독 | 색만 사용, 기호 채널 없음 |
| 해결된 항목의 조용한 소멸 | NN/g: "해결/확인되면 자동 만료" / VS Code [#44123](https://github.com/microsoft/vscode/issues/44123) "dismissed는 영구 소실" | 정의 없음 |

---

## 2. 선례 조사

### 2.1 T3 Code (요청 대상) — 인박스 화면이 없다

- **표면:** 좌측 스레드 사이드바가 곧 인박스. 섹션 타입 `pinned | active | snoozed | settled`이고 코드 주석이 active 구간을 "the inbox"라 부른다. 별도 라우트·메뉴·화면 없음. ([Sidebar.logic.ts](https://github.com/pingdotgg/t3code/blob/main/apps/web/src/components/Sidebar.logic.ts), [thread-sidebar.md](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md))
- **상태:** 행별 pill `Pending Approval > Awaiting Input > Working/Connecting > Plan Ready > Monitoring > Completed`, 프로젝트 행은 하위 스레드의 최고 우선순위로 롤업. 색 예약: amber=act now, sky=in motion, red=broken.
- **정렬:** 섹션 우선, active는 **수동 정렬**(드래그, 서버 영속)이며 "스레드 활동은 순서를 바꾸지 않는다". settled는 정착 시각순.
- **알림:** 별도 transient 계층(인앱 토스트 / OS 알림 / 사운드 / taskbar dot / 모바일 push)이 **같은 상태를 읽지만 같은 표면은 아님**. 승인·질문·실패는 즉시, 완료는 백그라운드 작업 정착 후 1회.
- **읽음:** 세 번째 독립 시스템(방문 시각 + unseen completion pill + mark unread).
- **배지 2종:** (a) 라이브 OS 알림 수 기반 taskbar dot, (b) 사이드바 토글의 unread 스레드 수(9+ 캡).
- **불만:** 거짓 완료 알림, Windows 토스트 미표시, 토스트 중첩, 알림 홍수.

### 2.2 피어 도구 요약

| 도구 | 표면 | 상태 모델 | 진입점 | 인라인 액션 | 읽음 | 불만 |
|---|---|---|---|---|---|---|
| **cmux** | 4채널 동시: pane ring / 사이드바 배지 / 팝오버 / 데스크탑 알림 | `turn-complete`·`needs-permission`·`idle-reminder`·`pending`·`isSubagent` | 사이드바 배지 상시 + `⌘⇧U`(최신 unread로 점프), `⌥⌘U`(토글 unread), `⌃⌘U`(mark-oldest-unread-and-jump) | 점프·unread 토글 | **Received→Unread→Read(열람 시)→Cleared** — 가장 깔끔 | 공식 문서 간 단축키 불일치(`⌘I` vs `⌘⇧I`) |
| **Warp** | 진짜 mailbox(벨, 우상단) + 코너 토스트 | Complete / Request / Error, 탭 아이콘 working·blocked·completed·errored | 벨 상시 + 탭 배지 + 데스크탑 알림 | 필터(All/Unread/Errors), mark-all-read, ↑↓ Enter | 탭 이동 시 자동 read | Windows 알림 미등록(#10187), 에이전트 15종 중 3종만 커버 |
| **Orca** | 헤더 벨 + Dock 배지 + 워크트리 칩 | working→idle 전이 시 발화. 구조화 채팅 성공은 unread만(토스트 없음) | 벨 상시 | 점프, 우클릭 mark unread | unread = "안 봤음" | #22181 소음, #12577 키보드 경로 없음, #7819 거짓 unread, #229 글리프 모호, #9509 크로스프로젝트 인박스 요구 |
| **Herdr** | 사이드바 = 대시보드, pane→tab→workspace 롤업 | `idle·working·blocked·done·unknown`, 기호 채널 옵션 | 사이드바 상시 | 포커스·attach | "볼 때까지 유지" | #318 정렬이 attention 우선 아님 → `herdr-attention` 플러그인 |
| **Nimbalyst** | Agent Attention List(배지) + 메뉴바 함대 + Session Kanban | `Awaiting input / Running / Unread` | nav 아이콘 배지 상시 | 세션 열기 | mark all read가 **승인/응답은 아님**을 명시 | 공개 불만 미발견 |
| **Conductor** | 사이드바(워크스페이스 4상태) + Dock 배지 | backlog/in progress/in review/done | 사이드바 + `⌘K` + Next Workspace | open·archive·pin | 수동 mark unread | changelog에 유령 알림·unread 미증가 수정 이력 |
| **Vibe Kanban** | **칸반 보드가 인박스** | 카드 위치 = 상태 | 보드 상시 | 카드 이동 | 없음 | 원격 접속 시 알림 미동작(#1634), 서비스 종료 |
| **[CC] agent view** | 전체화면 TUI 리스트 | `Pinned / Ready for review / Needs input / Working / Completed` | 프롬프트 푸터 `← 2 agents` + 탭 제목 | **peek 패널에서 인라인 응답**(번호 선택, Tab 제안 채우기, `!`로 bash), `Ctrl+X` 중지, `Ctrl+T` 핀 | 턴 경계 기반 | 공개 불만 미발견 |
| **Codex** | Activity 뷰(사이드바 벨) | Running / Needs input / Ready / Blocked | `Cmd+Option+U` | 필터(Work/Chat/Pinned/Scheduled), mark all read | 있음 | #13478 턴 경계 누락(plan 대기가 무알림) |

**주목할 패턴:** [CC]의 **행별 한 줄 요약**(Haiku급 모델이 working 중 15초마다, 턴 종료 시 재작성) — 막힌 행이 *실제 질문*을 보여준다. Warp의 **자식 에이전트 메일박스 제외**, cmux의 `suppressSubagentNotifications` 기본 on.

### 2.3 알림 센터 통합 UX 결론

- **단일 durable 목록 + transient는 그 투영.** 8개 중 6개가 정확히 2계층, VS Code/Xcode는 도메인 목록(Problems/Issue)을 *추가로* 두되 인박스와 섞지 않는다.
- **섹션으로 나누고 평탄 목록으로 합치지 않는다.** Linear(우선순위 vs 나머지), JetBrains(Suggestions vs Timeline), Slack(필터 탭).
- **배지는 unread durable만, 카운터는 하나.** 토스트는 절대 카운트를 올리지 않는다. Apple HIG: "badging이 essential info의 유일한 수단이면 안 된다."
- **액션이 있는 항목은 자동 소멸 금지.** NN/g: 5초 만에 사라진 토스트 때문에 사용자가 5분을 허비한 관찰 사례.
- **해결된 항목은 "Resolved" 상태를 거쳐 사라진다.** 조용한 수확 금지.
- **접근성:** 전 구간 키보드 도달, Esc로 포커스 복귀, 라이브 리전은 도착 전에 존재, 상태를 색에만 싣지 않는다.

---

## 3. 권고 설계

### 3.1 표면 (하나로 통합)

```
사이드바 하단/상단 상시 노출 "Attention" 버튼 (배지 = actionable 수)
  └─ 팝오버/패널 (기존 NotificationCenterPopover 확장, 새 모달 신설 금지)
       ├─ 섹션: Needs you (waiting / needs-input)   ← 최상단, 기본 펼침
       ├─ 섹션: Finished, unseen (done && !seen)
       ├─ 섹션: Working (접힘 기본)
       └─ 필터: All / Unread / Needs you   (Warp 방식)
```

- 기존 `notificationCenterStore`를 그대로 쓴다. 인박스는 **별도 저장소를 만들지 않는다**.
- 원격 호스트 항목은 `unavailableHosts`/`completeness`로 정직하게 표시(스코프 문서 결정 유지).

### 3.2 배지 의미 (스코프 문서 결정 유지)

- **배지 = actionable만** = `waiting + 안 읽은 done`. working은 배지에 넣지 않는다(상시 점등 → 신호 가치 상실).
- **목록 = working 포함 전부**, 단 섹션으로 분리해 기본 접힘.
- 카운터는 하나(`useNotificationCenter.unreadCount` 파생). 토스트는 카운트에 관여하지 않는다.

### 3.3 진입점 (호버 폐기)

- 상시 노출 버튼 1개(사이드바) + **단축키** + **명령 팔레트 등록**.
- 기존 `AttentionInboxDialog`의 호버 전용 벨 버튼과 네이티브 메뉴 항목은 제거하거나 상시 버튼으로 대체.
- 단축키 후보: `Cmd+Shift+A` (기존 레지스트리와 충돌 확인 필요).

### 3.4 상태 어휘 (색 + 기호 분리)

- `waiting`(amber, `◐`), `needs-input`(amber, `?`), `working`(sky, `✻`), `done-unseen`(green, `●`), `exited`(dim, `∙`), `failed`(red, `!`).
- 색만으로 의미를 싣지 않는다(Herdr `status_indicators="symbols"`, Orca #229 교훈).

### 3.5 항목 수명 (명시적)

```
Received  → 목록 진입 + (조건부) 토스트/OS 알림
Unread    → 배지 카운트에 포함
Read      → 해당 워크스페이스를 "열람"했을 때 (승인/응답과 무관)
Resolved  → 세션이 종료/해결됨. 잠깐 "Resolved"로 보인 뒤 제거 (조용한 수확 금지)
```

- **읽음 ≠ 승인**임을 UI 문구로 명시(Nimbalyst 방식: "mark all read does not approve").
- mark-all-read / dismiss / clear-all은 기존 스토어 API 재사용.

### 3.6 소음 제어 (필수)

- **provenance 구분**: 기계 생성(CLI·자동화) 항목은 기본 접힘 또는 저신호 섹션으로. Orca #22181 재발 방지.
- **자식/서브에이전트 완료는 목록에서 제외** (Warp·cmux 기본값).
- **토스트 억제 조건**: 창이 포커스되어 있고 해당 세션이 이미 보이면 토스트 금지(기존 `notificationCoordinator` 동작과 합치).

### 3.7 인라인 액션

- 최소: 행 클릭 → 해당 세션 포커스(기존 `handleNotificationTarget` 재사용).
- 다음: `waiting` 항목에 **peek**(최근 출력 몇 줄 + 질문) — [CC] peek 패널의 축소판. 전체 응답 입력은 후속.

### 3.8 기존 배선 처리

- `AttentionInboxDialog.tsx`(모달)·`Sidebar`의 호버 벨 버튼·네이티브 메뉴 항목 → **제거**하고 통합 표면으로 흡수.
- `desktopInventory.ts`(순수 빌더)와 `isUnreadAgent`/`resolveLocalSessionKey`는 **유지 가치 있음**: 알림 센터 항목에 인벤토리 유래 행을 합칠 때 재사용.
- `ferryx_scope/control/`은 이 표면과 무관(원격/자동화용 API). 별건 유지.

---

## 4. 결정 필요 사항 (사용자)

1. **통합 범위**: 알림 센터를 확장해 하나로 갈지(권고), 아니면 인박스를 폐기하고 알림 센터만 개선할지.
2. **working 노출**: 목록에 working 섹션을 넣을지(스코프 문서는 포함 결정), 아니면 actionable만 남길지.
3. **단축키 배정**: `Cmd+Shift+A` 사용 가능 여부 및 충돌 확인 후 확정.
4. **peek(인라인 미리보기) 범위**: 이번에 넣을지, 후속으로 미룰지.
5. **기존 인박스 모달**: 즉시 제거인지, 통합 표면 완성 시 제거인지.

---

## 5. 출처

- T3 Code: [Sidebar.logic.ts](https://github.com/pingdotgg/t3code/blob/main/apps/web/src/components/Sidebar.logic.ts) · [thread-sidebar.md](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md) · [ThreadNotificationCoordinator.tsx](https://github.com/pingdotgg/t3code/blob/main/apps/web/src/components/ThreadNotificationCoordinator.tsx) · [agentAwareness.ts](https://github.com/pingdotgg/t3code/blob/main/packages/shared/src/agentAwareness.ts) · [mobile-notifications.md](https://github.com/pingdotgg/t3code/blob/main/docs/user/mobile-notifications.md) · PR #6652 · PR #10077 · PR #12903
- cmux: [README](https://github.com/manaflow-ai/cmux) · [notifications docs](https://cmux.com/docs/notifications)
- Warp: [agent notifications](https://docs.warp.dev/agents/capabilities/agent-notifications/) · [관리 패널](https://docs.warp.dev/platform/managing-cloud-agents/) · [#10775](https://github.com/warpdotdev/warp/issues/10775) · [#10187](https://github.com/warpdotdev/warp/issues/10187)
- Orca: [notifications docs](https://www.onorca.dev/docs/notifications) · [#22181](https://github.com/stablyai/orca/issues/22181) · [#12577](https://github.com/stablyai/orca/issues/12577) · [#7819](https://github.com/stablyai/orca/issues/7819) · [#229](https://github.com/stablyai/orca/issues/229) · [#9509](https://github.com/stablyai/orca/issues/9509) · [#9944](https://github.com/stablyai/orca/issues/9944)
- Herdr: [agents docs](https://herdr.dev/docs/agents/) · [configuration](https://herdr.dev/docs/configuration/) · [#318](https://github.com/herdrdev/herdr/issues/318) · [herdr-attention](https://github.com/milkyskies/herdr-attention)
- Nimbalyst: [session management](https://docs.nimbalyst.com/session-management/agent-window-and-session-management.md) · [notifications](https://docs.nimbalyst.com/setup-nimbalyst/ai-provider-setup-and-notifications.md)
- Conductor: [changelog 0.35.0](https://www.conductor.build/changelog/0.35.0-workspace-status) · [releases CHANGELOG](https://github.com/meltylabs/conductor-releases/blob/main/CHANGELOG.md) · [0.36.4](https://www.conductor.build/changelog/0.36.4-next-workspace)
- [CC]: agent view docs (code.) · hooks reference
- Codex: [Notifications](https://learn.chatgpt.com/docs/notifications) · [#13478](https://github.com/openai/codex/issues/13478)
- Vibe Kanban: [settings](https://vibekanban.com/docs/settings/general) · [#1634](https://github.com/BloopAI/vibe-kanban/issues/1634)
- UX 근거: [NN/g — Indicators, Validations, Notifications](https://www.nngroup.com/articles/indicators-validations-notifications/) · [NN/g — 알림 피로](https://www.nngroup.com/articles/smart-home-notifications/) · [Apple HIG Notifications](https://developer.apple.com/design/human-interface-guidelines/notifications) · [Slack Activity](https://slack.com/help/articles/19693583638803-Get-your-work-done-from-the-Activity-view) · [Slack design — Activity 2.0](https://slack.design/articles/no-small-task-evolving-the-way-millions-catch-up-in-slack/) · [GitHub inbox](https://docs.github.com/en/github/managing-subscriptions-and-notifications-on-github/about-notifications) · [Linear Inbox](https://linear.app/docs/inbox) · [VS Code UX guidelines](https://code.visualstudio.com/api/ux-guidelines/notifications) · [JetBrains notifications](https://www.jetbrains.com/help/idea/notifications.html) · [WebAIM keyboard](https://webaim.org/techniques/keyboard/) · [accessible notifications](https://accessibility.build/guides/accessible-notifications)

**원문 리포트(전체):** 메모리 `notes/ferryx-agent-inbox-surface-research-2026-09-25.md`, `notes/ferryx-notification-inbox-surface-research-2026-09-25.md`
