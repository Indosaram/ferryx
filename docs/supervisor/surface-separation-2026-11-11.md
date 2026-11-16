# 수퍼바이저 / 앱 / 크론 표면 분리 결정 (2026-11-11)

## 전제

**수퍼바이저는 Ferryx 와 별개 제품이다.** Ferryx 는 그 제품이 쓰는 클라이언트/데몬 런타임 중 하나이고,
`/api/v1/supervisor/*` 는 그 제품의 원격 표면을 앱이 노출하는 형태다. 수퍼바이저의 존재 이유를
Ferryx 앱의 기능으로 설명하지 않는다.

이 문서는 "무엇이 어디에 붙는가"만 고정한다. 구현은 별건이다.

## 층 분리

| 층 | 정체 | 표면 | 판정 |
|---|---|---|---|
| 수퍼바이저 | 별도 제품. 목표·요청·디스패치 판단 | Ferryx 앱 원격 API (`/api/v1/supervisor/*`) | 제품 경계 — 여기로 간다 |
| 3개 앱 | instinct / vooy / pickle | 각자 자기 앱 | 슬랙에서 뗀다 |
| 허브 크론 | 정시 리포트 19건 | 슬랙 채널 4개 | 슬랙 유지 |

전송(창구)은 분리, 저장(기록)은 공유. 셋 다 `~/.ferryx/hub/hub.sqlite` 의 `sessions`(308행) /
`messages`(7030행)를 거치므로 기억은 이미 한 곳에 있다.

## 실측 근거

- `supervisor_channel_bindings` **0행** — 수퍼바이저는 슬랙 채널에 하나도 묶여 있지 않다.
  `chat/slack_supervisor.rs` 는 바인딩이 있을 때만 도는 잠든 경로다. 즉 분리는 이미 사실이다.
- `chat_outbox` **496행 전부 platform=slack**.
- `scheduled_jobs` 31건 중 19건이 슬랙 배달: `#work`(C0CATRWNX3J) 12, DM(D0C9HNVUK5M) 4,
  `#omo-home`(C0C9XG5FP8W) 2, `#aside-ideas`(C0CA1C16N49) 1.
- `schedule.rs` `deliver_run_output` 의 전송 분기는 `slack` / `discord` 둘뿐이고 나머지는 에러.
  web·앱 배달 경로는 없다.
- Ferryx 앱 푸시 `PushKind` 는 `NeedsInput` / `Finished(Duration)` **두 종류뿐**이다.
  크론 리포트를 실어 보내는 통로가 아니므로, 리포트를 앱 푸시로 옮기는 발상 자체가 성립하지 않는다.
- `inbox_items` 13,713행 중 `source='cron'` **156행** — 크론 리포트는 이미 인박스에 적재된다.
- `supervisor_pending_wakes` **0행** — 수퍼바이저 웨이크는 goal 이벤트로만 발생하며 크론은 트리거하지 않는다.

## 3개 앱 처리

| 앱 | 현재 배선 | 끊는 법 |
|---|---|---|
| vooy | launchd `cc.ferryx.vooy-bridge` → 슬랙 DM `D0CFK3E6S3V` | `launchctl bootout`, plist `.disabled` |
| pickle | launchd `cc.ferryx.pickle-bridge` → 슬랙 DM `D0CFL1E3WKX` | 동일 |
| instinct | 브리지 없음. 서드파티 Slack 앱 `A0AB2JKV29L`(봇 `U0C9XFB2G9H`)이 DM `D0CAY3MBTCY` 에 직접 게시 | 워크스페이스에서 앱 제거 |

instinct 는 원래 iMessage·문자·WhatsApp 이 본체이고 자체 앱이 없다. 슬랙은 부가 채널일 뿐이라
빼도 잃는 것이 없다. DM 최근 기록은 대부분 `New Assistant Thread` 자동 메시지다.

## hark 제거 (완료)

사용자 지시("hark는 브릿지에서 버려")로 `harkAdapter` 를 코드째 제거했다.
`bridge.ts` 68줄, `main.ts` 의 import·env 설명·`harkSlots` 루프, `bridge.test.ts` 의 hark 테스트 3건과
픽스처 이름을 정리. 남은 앱은 **vooy / pickle 둘**. `bun test` 60 pass / 0 fail, `tsc --noEmit` 무오류.
`hark-login.ts`·`hark-bootstrap.ts`·`lane-hark/` 는 참조가 끊긴 채 보관하며 재도입하지 않는다.

## 크론 리포트가 수퍼바이저에 닿는가

**지금은 닿지 않는다.** 크론은 `report_gate::queue` 로 `inbox_items` 에 넣고 슬랙으로 배달할 뿐,
수퍼바이저 웨이크를 발행하지 않는다. 붙일 때 선택지는 둘이다.

| 방식 | 내용 | 판정 |
|---|---|---|
| A. 인박스 조회 | 수퍼바이저 턴 시작 시 `inbox_items where source='cron'` 를 읽어 문맥에 포함 | **채택** |
| B. 웨이크 발행 | 크론 완료 시 `enqueue_wake(cause='cron_result')` | 기각 — 하루 19회 수퍼바이저를 깨워 대부분 "특이사항 없음"으로 토큰만 소모 |

## 슬랙 대안 검토 결과 (기각)

| 후보 | 기각 이유 |
|---|---|
| Buzz | Nostr 기반 초기 단계. 카톡·슬랙·라인 통합 기능 없음 |
| Mattermost | 셀프호스팅 무겁고 메신저 통합 없음. 에이전트는 별도 봇 API |
| Zulip | 스레드 모델은 좋으나 메신저 통합 없음 |

셋 다 "슬랙을 대체하는 팀 채팅"이지 "여러 메신저를 한 화면에 모으는 허브"가 아니다.
후자는 이미 PirateTalk 이 하는 일이다.

## 방향: PirateTalk 에이전트 뷰

PirateTalk 웹에 에이전트 뷰가 이미 있다. `slackView.mode === 'agent'` 분기와
`slack/AgentParts.tsx`·`AgentPromptBar.tsx`·`AgentTaskPanel.tsx`, `AGENT_STATUS_ORDER` 기반
상태별 그룹핑까지 동작한다. 다만 슬랙에 묶여 있다 — `ChatInput.tsx:159`:

```ts
const agentComposer = Boolean(chatId?.startsWith('slack:')) && slackView.mode === 'agent'
```

`slack:` 조건을 플랫폼 무관으로 바꾸고, `piratetalk-rs` 에 `agent` 플랫폼 방 타입을 추가하면
슬랙 없이 에이전트 대화가 성립한다. 크론 배달도 그 방으로 옮길 수 있다.

## Wakes from adopted and handed-over panes

The wake scheduler no longer fires only for goals the supervisor dispatched. A pane the owner
already runs can become a goal in two ways, and both feed the same wake path:

- **Adoption.** A legacy lane that the cutover adopts is bound to a goal, and every lane event of
  a supervisor-owned lane goes to `wakes::report` keyed by the lane event id, so a re-run after a
  crash is the same wake (`lanes/dispatch.rs`, `hand_to_supervisor`).
- **Handover.** The owner hands a running pane over instead of asking for a new one. In the app
  chat that is one message containing "이 세션 맡아" or "take this session" and exactly one
  `@mention` of the pane (`supervisor/app_work.rs`); zero or several addressed panes ask the owner
  which one instead of guessing. The panel has the same action as `hand_to_supervisor`
  (`src-tauri/src/hub_runtime/panel_autonomy.rs`). The goal is admitted as observed
  (`supervisor/handover.rs`): it commits as running, is bound to the handed-over pane, and its
  initial dispatch intent is superseded in the same transaction, so nothing is typed into the
  pane. Handing the same session over twice is the same goal, and a handover the owner cancelled
  is never recreated. Both handover paths give the goal the same small turn budget of 5.

This is why the panel can show a pane the supervisor never started: the goal observes the pane,
it does not own its transport.

## Who decides nudge, stall and turn limit

`supervisor/ownership.rs` is the single decision point for an owned session, and it is pure: it
reads a snapshot (`OwnedSessionView`) and returns one action — nothing, nudge, escalate the
stall, continue, or stop because the turn budget is spent. It sends nothing and changes nothing;
whoever owns the pane transport acts on the answer. A paused pane, an unknown state or a pending
prompt is left alone. Silence is measured against `FERRYX_HUB_NUDGE_AFTER_MIN` (default 15
minutes): one nudge, then a stall escalation if another full threshold passes without a report.
A fresh report after a nudge means the pane is alive again, so the supervisor asks it to
continue.

## Every settled goal reports back where it was asked

`supervisor/goal_report.rs` closes the loop. A goal that reaches a settled run state —
completed, degraded (the failure) or awaiting owner (the question) — is reported in the
conversation the request came from. Desktop, phone, CLI and MCP origins get the report in the app
transcript; Slack and Discord origins with a channel binding get it queued into the same channel
and thread, through the ordinary origin reply path. The key `goal-report:<goalId>:<state>` makes
a goal settling one way report exactly once, so a replay writes nothing. The body is the
outcome, the goal title and the state detail with every credential-shaped line removed.

## 결론

1. 수퍼바이저는 별도 제품으로 분리된 채 Ferryx 앱 표면으로 간다. (이미 `supervisor_channel_bindings` 0행)
2. instinct·vooy·pickle 은 슬랙에서 뗀다.
3. 허브 크론 리포트 19건은 **슬랙에 그대로 둔다.** 앱 푸시는 "지금 봐줘" 신호이고 리포트 운반로가 아니다.
4. 수퍼바이저가 크론 결과를 봐야 하면 A 방식(인박스 조회)으로 붙인다.
5. 장기적으로는 PirateTalk 에이전트 뷰가 대화 표면을 흡수한다.
