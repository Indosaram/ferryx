# Search Console indexing requests (2026-09-19)

Property used: **URL-prefix `https://ferryx.dev/`**, signed in as
`freedomzero91@gmail.com`. The domain property `sc-domain:ferryx.dev` is *not*
accessible to this account — Search Console answers
`이 속성에 액세스할 수 없습니다` ("you do not have access to this property") for
every `sc-domain:ferryx.dev` URL. All work below was therefore done in the
URL-prefix property, which is verified and functional. The console renders in
Korean; exact strings are quoted.

The property overview still reports `데이터를 처리하는 중이므로 며칠 후에 다시
확인해 보세요` ("data is still processing, check back in a few days"), so the
Performance and Indexing reports are not yet populated for this property.

## `/docs/facts/` inspection

`https://ferryx.dev/docs/facts/` — inspected before requesting anything
(`01-docs-facts-inspect.png`):

- Verdict: `URL이 Google에 등록되어 있지 않음` — URL is not on Google.
- Page indexing: `페이지 색인이 생성되지 않음: 발견됨 - 현재 색인이 생성되지 않음`
  — **Discovered – currently not indexed**.
- Discovery: Sitemaps `https://ferryx.dev/sitemap-index.xml`, referring page
  `https://ferryx.dev/sitemap-0.xml`.
- Crawl: last crawl `해당사항 없음` (N/A), crawl agent N/A, crawl allowed N/A,
  page fetch N/A, indexing allowed N/A.
- Canonical: both user-declared and Google-selected canonical are `해당사항 없음`.

So Google knows the URL from the sitemap but has never fetched it. There is no
crawl error, no robots block, and no canonical conflict to fix — it is a
scheduling backlog, which is exactly the case a manual request addresses. The
live URL served `200` over HTTPS when checked with curl at the same time.

## Indexing requests

All three requests returned the confirmation dialog `색인 생성 요청됨` with the
body `URL이 우선순위 크롤링 대기열에 추가되었습니다. 페이지를 여러 번 제출해도
대기열 위치나 우선순위가 변경되지 않습니다.` — added to the priority crawl
queue; resubmitting does not improve queue position.

| URL | State before request | Request result | Screenshot |
| --- | --- | --- | --- |
| `https://ferryx.dev/docs/facts/` | `URL이 Google에 등록되어 있지 않음` / 발견됨 - 현재 색인이 생성되지 않음 | `색인 생성 요청됨` | `02-docs-facts-requested.png` |
| `https://ferryx.dev/compare/` | `URL이 Google에 등록되어 있음` / `페이지 색인이 생성됨` | `색인 생성 요청됨` | `04-compare-requested.png` |
| `https://ferryx.dev/compare/tmux-git-worktree/` | `URL이 Google에 등록되어 있음` / `페이지 색인이 생성됨` | `색인 생성 요청됨` | `06-compare-tmux-git-worktree-requested.png` |

Both `/compare/` pages were already indexed and additionally reported
`HTTPS: 페이지가 HTTPS를 통해 제공됩니다` and `탐색경로: 유효한 항목 1개 감지됨`
(one valid breadcrumb item). They were resubmitted because their indexed copies
still carry stale license snippets; a recrawl is what replaces that cached copy.

## Prior activity in the same property today

The earlier verified results remain relevant:

- Google sitemap `/sitemap-index.xml`: Success, 16 pages discovered.
- Homepage and `/use-cases/parallel-ai-agents/`: indexed; live tests returned
  "URL can be indexed" and indexing requests were confirmed around 22:15 KST.
- Bing sitemap: Success, 16 URLs discovered, zero errors and zero warnings.
- Bing homepage and parallel-agent page: indexed successfully. The latter had
  one image-alt notice; the later live DOM audit found no images missing `alt`.

An earlier session in this same URL-prefix property already requested indexing
for the homepage and `/use-cases/parallel-ai-agents/`; its captures are in
`docs/evidence/site-indexing-20260919/`. That work is not repeated here. Its
`gsc-pages-report.png` shows the Page indexing report still empty with
`데이터를 처리하는 중이므로 며칠 후에 다시 확인해 보세요`, which is why per-URL
inspection, not the aggregate report, is the evidence used above.

## Limits and what was not done

- A request only queues a recrawl. Nothing here proves Google has crawled or
  re-indexed anything yet, and the queue is not given an ETA. Re-inspect these
  URLs in a few days to confirm the snippet actually changed.
- Recrawling `/compare/` pages refreshes whatever is live now. Content edits
  that have not been deployed will not appear until deploy plus a later crawl.
- No sitemap was resubmitted, no removal request was filed, no property was
  added or verified, and no new Conductor run was requested.

Screenshots: `docs/evidence/gsc-indexing-20260919/`.
