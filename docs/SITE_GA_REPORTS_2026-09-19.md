# Ferryx acquisition and download reporting

Saved exploration (reloaded to verify persistence):

https://analytics.google.com/analytics/web/#/analysis/a162818114p555083020/edit/_gQvNVcXTF6pN1rnFI81IA

Name: **Ferryx Acquisition and Downloads**, property `555083020`.

## Tabs

- **Acquisition**: rows are session source/medium, session campaign, and landing
  page. Values are active users, sessions, key events, and session key-event rate.
- **Downloads**: rows are the custom `platform`, `link_location`, `asset_id`, and
  session source/medium. Values are event count and key events. Event-name filter
  is exactly `download_click`.
- Both tabs filter hostname to exactly `ferryx.dev`, excluding local-host QA.
- Default date range is the last 28 days, ending yesterday. New data and newly
  registered custom dimensions may not appear immediately. An empty initial
  table is not evidence that collection is broken; use Realtime/DebugView for that.

The acquisition key-event rate covers all configured key events. Currently the
site-specific intent event is `download_click`; revisit that interpretation if
additional key events are introduced. A download click is not an installation.

## Test traffic and counting

The account setup worker changed `download_click` to **once per session** and
reopened the dialog to verify persistence. Raw event count still measures every
click; key events measure sessions with download intent.

A developer-traffic exclusion filter was created in **Testing** state. It matches
`debug_mode`/`debug_event`, not client IP. DebugView recognized the local QA
`page_view` and `download_click` at 21:57 KST. The site sends debug mode only on
loopback hosts or an explicit `analytics_debug=1` URL after analytics consent.

Testing filters label rather than discard traffic. Production QA visits remain
in normal reports until a verified filter is activated or excluded explicitly
in analysis. Historical setup events were not deleted. Do not treat them as
customer adoption. The hostname filter already removes loopback QA from this
saved exploration without changing raw data.

## Developer-traffic filter activation (2026-09-19, 22:51-22:54 KST)

The filter is no longer in Testing. It is now **Active** on property `555083020`
only, at
`https://analytics.google.com/analytics/web/#/a162818114p555083020/admin/datapolicies/datafilters`.
The admin UI renders in Korean; the exact strings observed are quoted below.

Inspected definition before any change (`01-filter-detail-testing.png`):

- Filter type (`필터 유형 선택`): `개발자 트래픽` — Developer Traffic, selected.
  `내부 트래픽` (Internal Traffic) and `웹 호스트 이름 트래픽` were not selected.
- Data filter name (`데이터 필터 이름`): `Developer Traffic debug_mode`.
- Filter operation (`필터 연산`): `제외` — Exclude.
- Summary (`요약`): `debug_mode 또는 debug_event의 값이 채워진 이벤트를 제외합니다.`
  — excludes events where `debug_mode` or `debug_event` is populated. It matches
  the debug parameters, not an IP range, so ordinary visitors are unaffected.
- Filter state (`필터 상태`): `테스트` — Testing.

Activation: selected `사용중` (Active), pressed `저장` (Save), and confirmed the
irreversibility dialog `필터를 활성화하시겠습니까?` by pressing `필터 활성화`
(`02-activation-confirm-dialog.png`). Google's own warning in that dialog states
the change is not retroactive, which matches the decision not to delete history.

Persistence was verified by a full page reload, not just the post-save view
(`03-filter-list-active-after-reload.png`):

| 이름 | 필터 유형 | 작업 | 현재 상태 |
| --- | --- | --- | --- |
| Internal Traffic | 내부 트래픽 | 제외 | 테스트 |
| Developer Traffic debug_mode | 개발자 트래픽 | 제외 | 활성 |

Reopening the saved filter after the reload shows type `개발자 트래픽` and state
`사용중` checked, `테스트` and `비활성` unchecked (`04-filter-detail-active-after-reload.png`).

Scope and non-actions, stated exactly:

- Only `Developer Traffic debug_mode` changed. `Internal Traffic` was left in
  `테스트` and was not edited, activated, or deleted.
- No property was created, no historical data was deleted, no IP-based exclusion
  was configured, and no other GA property was touched.

What this filter removes going forward: only hits that carry debug parameters.
Per `site/src/lib/analyticsRuntime.ts`, the site sets `debug_mode: true` on the
gtag `config` only when `location.hostname` is `localhost`, `127.0.0.1`, or
`[::1]`, or when the URL carries `analytics_debug=1` — and only after analytics
consent is granted. Ordinary `ferryx.dev` visitors never send `debug_mode`, so
they keep being collected. QA that needs to be invisible to reports must run on
loopback or with `?analytics_debug=1`.

Activation is forward-only. Events already collected while the filter was in
Testing remain in the data; exclude them in analysis (the saved exploration's
hostname filter already does this for loopback QA).

Screenshots: `docs/evidence/ga-developer-traffic-filter-20260919/`.
