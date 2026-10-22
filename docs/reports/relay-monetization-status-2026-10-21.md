# 릴레이 유료화 계획 및 구현 상태 보고 (2026-10-21)

## 결론
- 계획: `.omo/plans/relay-monetization.md` (2026-09-30). 16개 todo, 6단계. Lemon Squeezy, 기기 대수 과금(Free 1 / Pro 10+5팩 / Team 좌석당 10, 최소 2좌석), 7일 유예 후 원격 완전 정지, 체험판 없음. 09-25 계획(`relay-licensing-trial-selfhost.md`)의 30일 체험과 Paddle 안은 폐기됨.
- 구현: `b056b010 feat(billing): implement relay monetization…` (2026-10-01, 70개 파일, +28.5k줄). 소스 기준 16개 중 13개 DONE, 2개 DEVIATED, 1개 PARTIAL.
- 머지 상태: **main에 없음.** `refs/archive/relay-monetization`, `refs/archive/cleanup-20261009/local/feat/relay-monetization`(5403dc94)에만 존재. 원래 worktree는 2026-10-09 정리 때 삭제됨.
- main에 들어온 것: UI 쪽 `PlanSection.tsx`와 `billingApi.ts`뿐(51cd3e85 wip, 이후 abc1262a 수정). 백엔드가 없어서 로그인한 사용자가 "Plan" 메뉴를 열면 404가 나고 메뉴가 사라짐.
- 운영: 2026-10-21 측정 결과 relay.checka.cc는 `/api/account/v1/health`, `/install.sh`, `/api/v1/health` 등 모든 경로에 404를 돌려줌(Cloudflare 경유). 상용 과금 릴레이는 배포된 적이 없음.

## todo별 상태 (소스 = 5403dc94를 읽어서 확인)
| # | 항목 | 상태 | 비고 |
|---|---|---|---|
| 1 | 호스트 프록시 소유권 게이트 | DONE (+보안 결함 D1) | relay_server.rs:1913-2008. 단, 소유권 검사 전에 타 계정 bearer를 대상 호스트로 보냄(:1922-1925, 테스트가 served==1을 고정 :8136) |
| 2 | 플랜/엔타이틀먼트/유예 도메인 | DONE | entitlement.rs:3-9 상수가 계획과 정확히 일치. GRACE_SECS=604800 |
| 3 | SQLite 스토어와 billing/조직 레코드 | DONE | store_sqlite/. WAL, BEGIN IMMEDIATE, JSON 1회 이관 후 rename |
| 4 | 배포 모드 명시 | DEVIATED | 모드가 없으면 기동 실패는 맞음. 하지만 503 BILLING_UNCONFIGURED는 checkout/quantity/webhook에만 적용되고, 계획이 요구한 enroll/grant/attach에는 없음(routes.rs:17-20, 153-162) |
| 5 | 요금/셀프호스팅 문서 | DONE (경미한 편차) | pricing.mdx. 가격을 pricing.json에 하드코딩함(계획은 빈 값) |
| 6 | LS 어댑터 | DONE | HMAC 상수시간 비교(lemonsqueezy.rs:311-330), subscription-item PATCH |
| 7 | billing 라우트와 에러 details | DONE | 웹훅 중복을 provider GET보다 먼저 차단(routes.rs:1563-1591), quantity 부분 실패를 축별로 정합(:1012-1068) |
| 8 | 발급 지점 강제(enroll/grant/attach/control) | DONE (503 조항 제외) | service.rs:1203-1240, 1352-1380, relay_server.rs:659-693, 1805-1849 |
| 9 | 정지 스위퍼 | DONE | relay_server.rs:2701-2901. commercial 모드에서만 60초 주기 |
| 10 | 호스트 24h 임대(direct QUIC) | DONE | direct_lease.rs, direct_api.rs:231-283. 호스트에 FERRYX_RELAY_ACCOUNT_PUBLIC_KEY가 없으면 모든 direct 연결이 거부됨 |
| 11 | 데스크톱 Plan UI | DONE | PlanSection.tsx |
| 12 | 원격 웹/모바일 402 UX | DONE (편차) | attachTunnel.ts:999의 close reason 정규식이 AGENTS.md 위반. 연결 중 유예 배너 없음 |
| 13 | CLI `account plan` | DONE | cli/billing.rs. 402일 때 exit 3 |
| 14 | 알림 메일 4종 | DONE (D4 의심) | notices.rs. routes.rs:586-599의 prev_status 추정이 over_limit 사용자에게 잘못 적용될 가능성 |
| 15 | 운영 런북 | PARTIAL | docs/billing-operations.md. FERRYX_RELAY_ACCOUNT_PUBLIC_KEY, RESEND_API_KEY, FERRYX_MAIL_FROM, FERRYX_MAIL_WEBHOOK_* 누락 |
| 16 | 실서버 E2E | 기록상 PASS만 있음 | 메모리 기록에 Staging74 A-E PASS(maho-win)가 있으나 실제 LS가 아니라 loopback mock 사용. 증거 파일은 삭제됨 |
| F1-F4 | 최종 검증 wave | 미승인 | F1이 root에게 2회 반려된 채 종료(메모리 2026-10.md). 계획 파일 체크박스는 전부 비어 있음 |

## 검증된 적 없는 것
1. 실제 Lemon Squeezy 결제와 웹훅, quantity PATCH, 환불(LS 환경변수 없음, mock만 사용)
2. 상용 릴레이와 계정 서비스의 운영 배포(relay.checka.cc)
3. 실제 메일 발송, 초대 메일 수락
4. 실시간 7일 유예와 24h 임대 만료(테스트는 clock offset 사용)
5. 실제 NAT 환경의 direct QUIC 차단
6. 테스트 통과 여부: 424 backend / 77 vitest는 2026-10-01 기록일 뿐이고 이번 감사에서 실행하지 않음

## main에 넣는 비용 (측정: `git merge-tree --write-tree --name-only main 5403dc94`)
충돌 14개 파일:
- 백엔드: service.rs, bin/cli.rs, lib.rs, relay_server.rs, server.rs, tests/support/machine_input_fixture.rs
- PlanSection(.test).tsx (add/add 충돌)
- UI: RemoteSection.tsx, tauri.ts, RemoteApp.tsx, accountSession.ts, attachTunnel.ts, useAccountWorktrees.ts

신규 모듈(billing/*, store_sqlite/*, direct_lease*, cli/billing.rs, PlanLimitNotice*, site pricing)과 store.rs, direct_host_task.rs, relay.rs는 깨끗하게 병합됨. main이 분기 후 807개 커밋만큼 앞서 있음(server.rs 48개, RemoteApp.tsx 52개, relay_server.rs 14개). 그대로 머지하기보다 새 브랜치에 포팅하는 쪽을 권장.

## 권장 다음 단계
1. main 위에 새 브랜치로 포팅하고 위 충돌 14개를 해결
2. 포팅하면서 D1(bearer 선전송)과 attachTunnel 정규식을 고치고, 런북 env를 보강하고, 503 범위를 결정(계획을 수정하거나 코드를 수정)
3. LS sandbox 키로 실제 checkout과 웹훅 검증, 이어서 상용 릴레이 배포
4. 그 전까지 main의 Plan 메뉴를 숨김: 토큰 유무 대신 entitlement 응답 성공을 기준으로 노출
5. 별도 확인 필요: relay.checka.cc가 모든 경로에 404를 반환함
