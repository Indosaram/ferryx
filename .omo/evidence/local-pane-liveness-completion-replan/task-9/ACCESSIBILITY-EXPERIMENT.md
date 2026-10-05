# Task 9 — accessibility-route experiment (bounded), on `42fba06f`

Verifier: sole remote verifier. Date: 2026-10-05 (+0900).
Host: **maho-win** (`sook@100.126.171.58`), Windows 10.0.26200.9457.
**Revision: every measurement below is on `42fba06f`** (the `beb80b72` fix touches only the reporting path
and the `CharSet` on window-text imports; neither affects the UIA search, so the staging was left as-is).
Binary for all runs: `6ad63c5afdcbb8348fccd0253d0cdb0066fc054bda99b41387b5bc5fbeb5b042`, 106 915 840 B.
All measurement runs execute **inside the interactive console session** (`mySession: 1`,
`interactive: true`) via the same `schtasks /create /it` mechanism the runner uses.
Harness: `accessibility-experiment/` (`exp-accessibility-probe.mjs`, `exp-session1.ps1`, `exp-run.ps1`,
`exp-tree.ps1`, `exp-oleacc.ps1`); raw results `exp-result-{baseline,env,oleacc,withui}.json`.

## HEADLINE

**The accessibility hypothesis is REFUTED, and the real root cause is different and earlier than UIA.**
With the app's own UI actually loaded, the webview's DOM **is fully exposed to UIA with no activation at all**
(33 named elements, including real app controls). The `Split pane right` button is **still not findable** — but
not for an accessibility reason: **the app is showing its empty state (`"No open tabs"`), so there is no pane
to split.** The scenario never creates a GUI pane.

| Route | Result | Needed? |
|---|---|---|
| **Baseline** (no extra env) | tree = Chromium **`ERR_CONNECTION_REFUSED`** page; 29 nodes; no app UI | — |
| **1. `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`** | ran; tree unchanged (`ERR_CONNECTION_REFUSED`, 29 nodes, `matchCount 0`) — **cannot attribute the null** (the page was an error page); **unnecessary**, see below | **NO** |
| **2. Legacy `AccessibleObjectFromWindow`** | probe **threw** (`hr: -9999`, `gotObject: false`) — **inconclusive instrument**, stated as such; moot | **NO** |
| **3. Product-side `additionalBrowserArgs`** | **not run — unnecessary**, and it would require editing a throwaway copy | **NO** |
| **`withui`** (decisive, my control) | UIA tree **fully populated**: 33 named app elements | — |

**Decision rule: the third branch is NOT reached.** The affordance is **not** unreachable by UIA — it is
reachable, and simply **absent because no pane exists**. **No product accessibility change is required.**

---

## 1. Baseline — and the root cause it exposed

`exp-run.ps1 -Route baseline`. Launch: the staged QA binary with `buildIsolatedEnv`'s exact env
(`FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR`/`FERRYX_QA_BARRIER_DIR`/`FERRYX_QA_RUN_ID`/`FERRYX_QA_OPERATION_ID`/
`FERRYX_QA_FIXTURE_KINDS`), visible window in **611 ms**, 3 visible windows.

The driver's own builders reported exactly what pass 7 reported — `SPLIT_RIGHT_NOT_FOUND`,
`candidateCount: 0`, `inventory.matchCount: 0`, `inspectedCount: 29` — **so the harness faithfully reproduces
the runner's finding** (my control passes).

But the **unfiltered tree dump** (no name filter) showed what those 29 nodes actually are:

```
name=""                                     [Pane]      cls="WRY_WEBVIEW"
name="127.0.0.1"                            [Pane]      cls="Chrome_WidgetWin_1"
name="127.0.0.1 - 네트워크 오류 - 웹 콘텐츠"  [Pane]      cls="BrowserRootView"
name="127.0.0.1"                            [Document]  id="RootWebArea"
name="흠… 이 페이지에 연결할 수 없습니다."     [Text]
name=" 연결을 거부했습니다."                  [Text]
name="ERR_CONNECTION_REFUSED"               [Text]
name="새로 고침"                             [Button]    id="reload-button"
name="Microsoft Edge"                       [Text]
```

**The app's webview was displaying Chromium's connection-refused error page.** The root cause is in the
configuration, not in accessibility:

| Fact | Evidence |
|---|---|
| `src-tauri/tauri.conf.json` has `"devUrl": "http://127.0.0.1:5173"` and `"frontendDist": "../ui/dist"` | config read |
| A **debug** build uses `devUrl` (release uses `frontendDist`) | the running binary is `src-tauri\target\debug\ferryx.exe` |
| **Nothing was listening on 5173** during any scenario run | `Get-NetTCPConnection -LocalPort 5173` → `NOTHING_LISTENING_ON_5173` |
| The project **has** a frontend starter, but the runner never starts it | `scripts/dev-frontend.mjs` exists; `grep` of the runner path shows no invocation |

So: **the runner launches a dev-mode binary with no frontend server, and the app renders an error page.**
That alone explains pass 7's empty inventory — and it means the empty inventory was **not** evidence about
accessibility at all.

---

## 2. Route 1 — `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (run)

Launch env added `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-renderer-accessibility` (recorded in the
result as `extraEnv`). Result, verbatim:

```
visibleWindowCount=3   psFailure=SPLIT_RIGHT_NOT_FOUND   candidateCount=0
inventory={"inspectedCount":29,"matchCount":0,"truncated":false,...}
tree: hwnd=2297012 total=29  name="127.0.0.1 - 네트워크 오류 - 웹 콘텐츠" ... name="ERR_CONNECTION_REFUSED"
ACCEPTANCE nameFoundAnywhere=false
```

**Reporting it honestly, as the dispatch required:** this is **not** a refutation of route 1. The page being
measured was an **error page**, so a null result here is uninformative — I cannot tell whether wry/Tauri
suppressed the variable or whether it applied and had nothing to expose. **What I can say definitively is that
route 1 is unnecessary**, because §4 shows the tree fully accessible with **no** such variable.

## 3. Route 2 — legacy `AccessibleObjectFromWindow` (run; instrument inconclusive)

`exp-oleacc.ps1` calls `AccessibleObjectFromWindow(hwnd, OBJID_CLIENT, IID_IAccessible, …)` on each visible
window, then the split inventory is re-measured. Verbatim:

```json
{"probe":"oleacc-activate","pid":5116,"visibleWindows":3,
 "results":[{"hwnd":32114698,"hr":-9999,"gotObject":false,"name":null,"role":null,"childCount":null}, …×3]}
```

`hr: -9999` is **my probe's own catch**, i.e. the call **threw** instead of returning an HRESULT (the
`[MarshalAs(UnmanagedType.Interface)] out object` marshalling needs an explicit interface declaration). **So
this route is inconclusive as instrumented — I am stating that rather than reporting it as a refutation.**
It is also **moot**: §4 proves the tree needs no legacy activation.

## 4. The decisive control — serve the UI, then re-measure

`ui/dist` already exists in the staging tree, so instead of debugging `bun scripts/dev-frontend.mjs` (its first
action is a full UI build; a first attempt did complete and left Vite on 5173, and a second attempt was killed
as redundant) I served the built assets on `127.0.0.1:5173` with a ~15-line static server
(`serve-dist.ps1`; **harness-only, no product file touched**). Verified:

```
LISTENING pid=22056   HTTP=200 bytes=3536   HAS_ROOT_DIV=True
```

Then `exp-run.ps1 -Route withui`. **The UIA tree became fully populated** — `hwnd=11865304 total=72`,
**33 named elements**, including genuine app UI:

```
"Ferryx"                       [Document] id="RootWebArea"
"Hide sidebar"                 [Button]
"Add project"                  [Button]
"Inbox"                        [Button]
"Expand Strawberry"            [Button]
"Strawberry"                   [Button]
"Past conversations"           [Button]
"Remove project Strawberry"    [Button]
"Settings"                     [Button]
"Resize sidebar"               [Separator]
"No open tabs"                 [Text]
"Open a terminal or browser tab to get started."  [Text]
"New Terminal"                 [Button]
"New Browser Tab"              [Button]
"Collapse toolbar"             [Button]
"Notifications alt+T"          [Group]
"Close toast" / "Got it" / "Dismiss all notifications"   [Button]
```

**Conclusions from this one measurement:**

1. **Accessibility was never the blocker.** WebView2's DOM **is** exposed to UIA by default — no
   `--force-renderer-accessibility`, no `AccessibleObjectFromWindow`, no product change. **The pass-7
   hypothesis ("WebView2 content is not surfaced to UIA unless accessibility is activated") is REFUTED.**
2. **The empty inventory in pass 7 was caused by the missing frontend server** (§1), not by UIA.
3. **`Split pane right` is still not findable — and now I can say exactly why:** the app is showing its
   **empty state**. The tree says `"No open tabs"` and `"Open a terminal or browser tab to get started."`
   with only `"New Terminal"` / `"New Browser Tab"` as affordances. **There is no pane, so there is no
   pane toolbar and no split button to find.**

`ACCEPTANCE: nameFoundAnywhere=false` — the element named `Split pane right` **does not exist in the tree**,
because its parent pane does not exist.

---

## 5. What this means for the decision rule

- The rule's **product branch does not apply**: the affordance is not "unreachable by UIA"; it is reachable and
  simply **not instantiated**.
- The rule's third branch (fall back to keyboard navigation because UIA cannot see it) **does not apply**
  either — UIA sees the webview fine.
- **The actual next gate is harness-side and is the pass-4 risk, now confirmed:** the scenarios must
  **create a terminal tab/pane through the UI** before the split affordance can exist. In pass 4 I reported
  "daemon-sessions-vs-GUI-panes is not the blocker" — that was correct *then* (runs failed earlier), and
  **now it is the blocker**. The fixtures are daemon sessions; **nothing binds them to a GUI pane**, and the
  app boots to the empty state.
- **Two harness requirements fall out of this measurement**, both scripts/frontend lane:
  1. **Serve the frontend** (the debug binary needs `127.0.0.1:5173`; `ui/dist` + a static server is enough,
     or run `scripts/dev-frontend.mjs`), or run a **release** binary that embeds `ui/dist`.
  2. **Open a terminal pane through the UI** (`"New Terminal"` is present and named) before `driver.split`.

**One question, explicitly a question and not a finding:** should the app **restore panes from the daemon's
existing sessions** on boot? If it did, the daemon fixtures the harness already creates might produce a pane
without a UI step. I did not test that, and it is a product-behaviour question for the lead.

---

## 6. Two smaller observations from the same evidence

- **The window `title`/`className` are still 1 character on `42fba06f`** (`"F"`/`"T"`, and `"P"` for a third
  window) — expected, since the `CharSet = CharSet.Unicode` fix is in `beb80b72`. The dispatch asks me to
  confirm the real strings next time; on this revision they are still truncated, which is consistent.
- **A third visible owned window appeared** (`className "P"`, `total=0`) that pass 7 did not see — the app's
  window set varies run to run, which the multi-window enumeration now handles.

---

## 7. Teardown

| Resource | Teardown | Receipt |
|---|---|---|
| the static UI server on 5173 (`bun`, PID 22056) | killed by exact PID | `TEARDOWN` receipt in `accessibility-experiment/` |
| the Vite server left by the earlier `dev-frontend.mjs` attempt | killed by exact PID | same |
| app processes from all four experiment runs | `taskkill /T /F` on each run's own `appPid` (the script does this itself) | per-run `outcome: MEASURED` implies teardown ran |
| experiment scheduled tasks (`ferryx-exp-*`) | deleted after each run | `exp-run.ps1` deletes by name |
| isolation roots (`exp-baseline`, `exp-env`, `exp-oleacc`, `exp-withui`) | removed per route at start; final sweep below | `TASK_OWNED_ALIVE_COUNT=0` |
| **candidate tree** | **never edited** | tree clean at `42fba06f`; the probe lives in the staging tree only |
| foreign trees/processes | **untouched** | — |
