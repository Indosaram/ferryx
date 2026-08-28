# Ferryx 리모트 pane 노출 — 핸드오프 3건 원인 분석 및 해결 계획

**작성일:** 2026-08-27
**대상:** 이전 세션 핸드오프의 미해결 3건 (pane 활성화 테스트 실패 / 라이브 발행 / 기기 취소)
**성격:** 분석 전용 — 이 문서는 코드 변경을 동반하지 않음. 모든 결론은 실제 워킹트리 코드 `read`로 확정.

---

## 1. pane 활성화 테스트 실패 — 근본 원인 **확정** (계측 불필요)

### 핸드오프 문서의 오류 정정

핸드오프는 두 가지를 "완료됨"으로 서술했으나 워킹트리에 **존재하지 않음** (git diff hunk 전수 확인):

| 핸드오프 주장 | 실제 상태 |
|---|---|
| "`isTerminalTabInWorktree`가 leaf 주소를 이해하도록 수정" | **수정 흔적 없음.** `ui/src/App.tsx:321-331`은 여전히 raw 탭 id 조회 |
| "`handleRemoteSelectionRequested`와 `pendingRemoteSlug` 이펙트 모두 `activateRemoteEntry` 사용" | 핸들러만 migrate됨. `pendingRemoteSlug` 이펙트(`App.tsx:968`)는 여전히 `activateTab(requestedTabId)` 호출, deps(`:972`)에도 `activateRemoteEntry` 없음 |

App.tsx를 한 번 파괴했던 read→write 왕복 사고 때 `isTerminalTabInWorktree` 수정분이 유실됐을 가능성이 높음.

### 실패 경로 (정적으로 증명됨)

현재 구현 (`ui/src/App.tsx:321-331`):

```ts
function isTerminalTabInWorktree(state, worktree, tabId) {
  if (!tabId) return true;
  const tab = state.layout.tabs.find((c) => c.id === tabId);   // ← "tab-1::leaf-1b"는 어떤 탭 id와도 일치하지 않음
  if (!tab || tab.kind === "browser") return false;             // ← 무조건 여기서 false
  ...
}
```

실패 테스트가 `tab-1::leaf-1b`로 선택을 요청하면:

1. `handleRemoteSelectionRequested` → `requestedEntryId = "tab-1::leaf-1b"`
2. `isTerminalTabInWorktree(...)` → `tabs.find(id === "tab-1::leaf-1b")` → `undefined` → **false**
3. 핸들러 라인 989 (`if (!isTerminalTabInWorktree(...)) return;`)에서 **early return**
4. `activateRemoteEntry` 미도달 → `activateTab` 0회, `focusPane` 0회 → 관측된 에러와 정확히 일치

`parseRemotePaneId`가 정상(프로브 확인)이므로 파싱 문제가 아니며, **가드가 composite id를 파싱 없이 raw 조회하는 것이 유일한 원인**. 통과 테스트(`:2980`)는 plain id(`tab-2`)라 find가 성공하므로 이 가드를 통과할 뿐임.

### 이미 실패한 접근 3가지가 전부 실패한 이유

스토어 교체/브랜치 매칭/통과 테스트 모방 모두 **테스트 하네스 조작**이었으나, 차단 지점은 프로덕션 가드 함수 자체. 가드를 고치지 않는 한 어떤 하네스 조정도 통과 불가 — 세 번의 실패는 필연이었음.

### 해결 방법 (구현 시)

**(a) `isTerminalTabInWorktree`를 entry-aware로 수정** — 핵심이자 유일 필수 수정:

```ts
function isTerminalTabInWorktree(state, worktree, entryId: string | null): boolean {
  if (!entryId) return true;
  const { tabId, leafId } = parseRemotePaneId(entryId);
  const tab = state.layout.tabs.find((c) => c.id === tabId);
  if (!tab || tab.kind === "browser") return false;
  const sessionId = leafId
    ? state.layout.layoutsByTabId?.[tabId]?.sessionIdsByLeafId?.[leafId]
    : tab.sessionId;
  if (!sessionId) return false;
  const session = state.sessions[sessionId];
  return (session?.worktreePath ?? session?.cwd) === worktree.path;
}
```

- 호출처가 2곳(`:964` pendingSlug 이펙트, `:989` 핸들러)뿐이라 함수 수정 하나로 둘 다 해결됨
- 알 수 없는 leaf → false → 원격 선택 무시(안전한 방향)

**(b) `pendingRemoteSlug` 이펙트 migrate** — 크로스 프로젝트 경로의 잠재 버그 별도 수정:
- `:968` `activateTab(requestedTabId)` → `activateRemoteEntry(requestedTabId)`
- `:972` deps: `activateTab` → `activateRemoteEntry`
- 현 상태로는 크로스 프로젝트 선택에서 composite id가 그대로 `activateTab`에 전달되어 탭을 찾지 못하고 조용히 no-op됨

**(c) 수정 후 실패 테스트가 통과함이 정적으로 증명됨:**
parsed lookup → `sess-1b` (`cwd: "/repo/main"` === worktree.path) → 가드 통과 → activeWorktreePath 동일(mock 기본값 `/repo/main`, `:3015` 테스트가 `/repo/feature`로 바꿔서 쓰는 것으로 역산) → 동기 분기에서 `activateRemoteEntry` → `act()` 안에서 `activateTab("tab-1")` + `focusPane("tab-1", "leaf-1b")` 호출.

### 검증 절차

```bash
cd ui && bunx vitest run src/App.test.tsx   # 기대: 79 passed
cd ui && bunx tsc --noEmit                  # exit 0 유지
```

---

## 2. 라이브 발행이 데몬에 도달하지 않음 — 메커니즘 분석

### 발행 경로 (코드로 확정)

```
WorkspaceApp useEffect (App.tsx:797-799)
  → publishFocusedTerminal (tauri.ts:595-610)
  → cmd_remote_set_active_selection (ipc/remote.rs:354)
  → RemoteGatewayManagerInner::Daemon → daemon (lib.rs:230-233, GUI는 항상 from_daemon)
```

**프로덕션에서 발행자는 GUI 프로세스 단 하나.** 다른 `SetActiveSelection` 호출처(`daemon/client.rs:1455`, `daemon/server.rs:1493`)는 전부 `#[cfg(test)]`. 데몬 자체는 selection을 생산하지 않음.

### `null`의 합법적 원인 3가지 (진단 시 반드시 구분)

1. **GUI 프로세스 없음** — PTY는 데몬에 생존하지만 발행자가 부재. 핸드오프가 구분 못 했던 바로 그 경우이며, 모바일 스피너는 이 상태에서의 **설계된 대기 동작**(Active Desktop Lock).
2. **포커스가 비터미널 탭** — 브라우저 탭 포커스 시 `deriveFocusedTerminal`이 `null`을 반환(`App.tsx:233`) → `publishFocusedTerminal(null)` → cmd가 all-null 요청을 `None` selection으로 매핑(`ipc/remote.rs:358-365`) → 데몬 selection이 None으로 **능동 초기화**됨. 즉 GUI가 정상 동작 중이어도 `null`은 유효 상태일 수 있음.
3. **데몬 재시작 후 재발행 공백 (잠재 버그)** — 발행 이펙트 deps는 `[focusedTerminalPayload, reportRuntimeError]`(`:799`)뿐. 데몬이 재시작되면 서버 측 selection이 날아가지만, 포커스/탭 변경이 없으면 이펙트가 재발화하지 않아 **다음 탭 전환까지 영구 null** 유지. 발행 파이프라인의 실제 갭이며 pane 노출 기능의 라이브 검증도 가릴 수 있음.

### 판별 사실

- 데몬은 GUI 종료 후에도 마지막 selection을 **메모리에 유지**(disconnect 시 clear하는 코드 없음 — `remote/state.rs:148-168`는 setter뿐). 따라서 데몬 가동 중 GUI가 한 번이라도 발행했다면 GUI 사망 후에도 `null`이 아니라 **stale 값**이어야 함.
- 역산: `null` 관측 = "데몬 시작 이후 한 번도 발행 없음" 또는 "비터미널 포커스로 None 발행됨" 또는 "데몬 재시작 후 재발행 없음".

### 다음 라이브 절차 (GUI 확보 후, 순서대로)

1. `cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx` (낡은 바이너리 함정 회피) → 앱 재시작
2. GUI가 터미널 탭에 포커스된 상태에서 `RemoteGetActiveSelection` 프로브
3. 여전히 `null`이면 GUI 측 `reportRuntimeError` 토스트 유무 확인 (`.catch(reportRuntimeError)`가 이미 있으므로 IPC 실패는 표면화됨)
4. 발행 확인 후 탭 전환 → selection 갱신 확인 (pane 주소 `tab::leaf` 포함 여부까지)

### 해결 옵션 (구현 단계에서 선택)

- **(권장) 재연결 재발행:** DaemonClient 재연결/앱 포커스 회복 시 마지막 payload 재발행 → 원인 3 제거
- **(선택) staleness 표면화:** selection에 published-at 타임스탬프를 싣고 모바일이 "데스크탑 미연결"을 스피너 대신 표시 → 원인 1의 UX 개선 (Active Desktop Lock 약화 아님)
- **(선택) 원인 2 문서화:** 비터미널 포커스 시 None 발행은 의도 동작임을 wire 계약 문서에 명시

---

## 3. 페어링 기기가 계속 취소됨 — 코드 사실과 분리된 두 증상

### 코드 사실 (전수 grep 기준)

`revoked: true`를 설정하는 경로는 `auth.rs:157 revoke_device`가 **유일**하며, 그 호출처는 프로덕션에 정확히 3곳:

1. `POST /api/v1/devices/{id}/revoke` (`remote/server.rs:739-752`)
2. 데몬 `RemoteRevokeDevice` (`daemon/server.rs:748`)
3. SettingsDialog Revoke 버튼 → `cmd_remote_device_revoke` (`SettingsDialog.tsx:1509-1512`, 2-click confirm)

다음은 **존재하지 않음**을 확인:

- pairing exchange 시 자동 revoke (`auth.rs:92-126`: 매번 새 uuid device를 `revoked: false`로 생성, 동명 device 병합/정리 없음)
- 토큰 만료 (`validate_token:128-151`에 TTL 검사 없음)
- 모바일/웹 클라이언트의 revoke 호출 (ui/src 내 revoke 호출처는 SettingsDialog뿐)

### 증상의 분리

| 증상 | 메커니즘 |
|---|---|
| 서버 측 `revoked: true` (4/5 기기) | 위 3경로 중 하나를 통한 **명시적 호출**만 가능. 코드에 자동화된 경로가 없으므로 사람/스크립트의 호출이 유일한 설명 |
| 같은 기기 재접속 시 PIN 재요구 | (확인 후: 위 revoke의 직접 결과로 설명됨) 원래 분석: **별개 메커니즘일 수 있음**: 클라이언트는 401("Invalid or revoked token") 수신 시 localStorage 토큰을 스스로 삭제(`remoteClient.ts:16-21`). 서버 device가 revoked가 아니어도 브라우저 프로파일/오리진 변경, 스토리지 클리어로 동일 증상 발생. 이 경우 재페어링은 device만 추가 적재 (실제로 "Browser Device" 3개 공존 — exchange가 병합하지 않는다는 코드와 일치) |

### 관측값 정합성

- QA iPhone(created→lastSeen ≈ 2분), Mobile Device(≈ 22초): 짧은 pair→use→revoke 주기는 과거 QA/스크립트 검증 활동 패턴과 일치
- 유일한 생존 device(Browser Device, lastSeen 최신)가 현재 세션과 일치

### 결론 — **종결 (2026-08-27, 사용자 확인)**

사용자가 본인이 직접 revoke했다고 확인. **버그 아님, 추가 조사 불필요.** "명시적 호출만이 유일한 경로"라는 코드 사실과 관측값이 정확히 일치함.

"같은 기기 PIN 재요구" 증상도 동시에 설명됨: revoke된 device → 401 `RevokedDevice` → 클라이언트가 localStorage 토큰 자가삭제(`remoteClient.ts:16-21`) → 재페어링 필요 → exchange가 새 uuid device 발급. 설계대로 동작한 결과이며 "Browser Device" 3개 공존도 이 사이클로 설명됨.

추적 계측 제안(`revoked_at` 메타데이터, revoke 로깅, 401 토큰 폐기 로그)은 **철회**. 다만 관찰된 사실 하나는 남겨둠: revoke는 비가역이고 device 목록에 삭제 액션이 없어 죽은 항목이 영구 누적됨(현재 5개 중 4개). 설정 목록이 거슬려지면 그때 정리 UI를 고려하면 됨 — 지금 할 일은 아님.

---

## 4. 우선순위 권고 (핸드오프 판단 재평가)

| 순위 | 건 | 근거 |
|---|---|---|
| 1 | **(a)+(b) 수정 후 테스트 녹색화** | 원인이 정적으로 완전 확정됐고 수정이 국소적. pane 노출이 이미 wire에 실리는 상태에서 활성화 경로가 막혀 있으면 모바일 탭 탭-투-포커스가 composite id에서 조용히 실패함. 라이브 확인 없이도 RED→GREEN으로 증명 가능 |
| 2 | 라이브 발행 판별 절차 수행 | GUI 필요 + 사용자 협조 필요. 대부분 "GUI 부재" 또는 "비터미널 포커스"로 설명될 가능성이 높으나, 원인 3(재연결 공백)은 실버그라 따로 추적 가치 있음 |
| — | ~~기기 취소~~ | **종결**: 사용자 본인 revoke로 확인됨(2026-08-27). 대응 불필요 |

핸드오프의 "라이브 발행 우선" 판단은 'pane 노출이 아무것도 안 뜬다'는 가정에 기반했으나, 실제로는 발행자(GUI)만 떠 있으면 pane 페이로드도 동일 경로로 실리므로 두 건은 독립적임. 차단 원인이 확정된 1번을 먼저 끝내는 것이 총량 기준 최단.

---

## 5. 구현 제약 재확인 (이어받는 세션용)

- git commit 금지, 스코프 밖 파일 무접촉 (워킹트리에 다른 세션 동시 편집분 존재)
- `App.tsx`/`App.test.tsx` 수정은 `edit` 도구 또는 `perl -0pi -e`만 사용 — `read`→`write` 왕복 금지 (이미 한 번 파괴 사고, 이번 트리아지에서도 당시 유실 흔적으로 보이는 수정 누락 확인)
- Active Desktop Lock 약화 금지, 실패 테스트 삭제/스킵 금지, 타입 에러 억제 금지
- 임시 계측(console.log) 계획은 **폐기** — 원인이 정적 증명으로 확정됨

---

## 6. 구현 및 검증 결과 (동일 세션, 2026-08-27)

1번 항목을 구현했고, 3번은 사용자 확인으로 종결(본인 revoke). 남은 건은 2번 하나이며 라이브 GUI가 필요함.

### 적용된 변경 (`ui/src/App.tsx`, `ui/src/App.test.tsx` 2파일만)

- **(a)** `isTerminalTabInWorktree`가 `parseRemotePaneId`로 entry id를 분해하고, leaf가 있으면 `layoutsByTabId[tabId].sessionIdsByLeafId[leafId]`로 해당 pane의 세션을 해석해 worktree를 비교 (`App.tsx:321-338`). 알 수 없는 leaf → false
- **(b)** `pendingRemoteSlug` 이펙트가 `activateTab(requestedTabId)` → `activateRemoteEntry(requestedEntryId)`로 migrate, deps도 교체 (`App.tsx:966-979`)
- **신규 락 테스트**: `activates the exact pane after a queued cross-project selection names a leaf entry` — 큐 경유(크로스 프로젝트) 경로의 leaf 활성화를 고정. 기존 테스트는 plain id만 덮고 있어 (b)에 커버리지가 없었음

### 검증 증거

| 검증 | 결과 |
|---|---|
| `bunx vitest run src/App.test.tsx` | **80 passed** (기존 실패 1건 해소 + 신규 1건 추가) |
| 신규 테스트 RED 증거 | 이펙트만 `activateTab`으로 임시 되돌리자 `expected "spy" to be called with [ 'tab-1', 'leaf-1b' ] / Number of calls: 0` → 즉시 복원 후 GREEN. 테스트가 (b)를 실제로 고정함이 증명됨 |
| `bunx tsc --noEmit` | exit 0 |
| `bun run test` (ui 전체 112파일 1006테스트) | 1004 passed / 2 failed — 실패는 전부 `ui/src/components/SettingsDialog.test.tsx` 1파일에 국한. 해당 파일과 `SettingsDialog.tsx`는 **다른 세션의 미커밋 작업분**이며 `App.tsx`를 import하지 않고 단독 실행에서도 재현 → 본 변경과 무관한 선행 실패 |

### 준수 사항

- 커밋하지 않음. 변경 파일은 `ui/src/App.tsx`, `ui/src/App.test.tsx` + 본 문서뿐 (다른 세션 작업분 무접촉)
- `read`→`write` 왕복 없이 `edit` 도구만 사용
- 실패 테스트 삭제/스킵 없음, 타입 에러 억제 없음

---

## 7. 포커스 종속 폐기 — 전체 탭/pane 목록 노출 (2026-08-28)

사용자 지시: **"전체 탭/페인을 리스트에서 선택할 수 있게"**. 기존 "터미널을 포커스해야 스트립이 뜬다"는 설명은 설계 그대로였고, 그 설계 자체가 거부됨.

### 왜 포커스가 필요했는가 (구 설계의 정확한 근거)

목록이 독립 인벤토리가 아니라 **"포커스된 터미널" 페이로드에 얹힌 필드**였음:

| 지점 | 구 동작 |
|---|---|
| `App.tsx:230` | `activeTabId` 없으면 `return null` → 페이로드 전체 소멸 = 목록도 소멸 |
| `App.tsx:233` | 활성 탭이 브라우저면 `return null` → 동일 |
| `App.tsx:268` | `if (!isFocusedPane && panePath !== foundWorktree?.path) return []` → 포커스된 pane과 **같은 워크트리**의 pane만 남김 |
| `RemoteSessionList.tsx:488` | `terminalTabs.length > 1` 미만이면 스트립 자체를 렌더 안 함 |

즉 리모트가 받던 건 "전체 탭 목록"이 아니라 "포커스된 터미널의 같은-워크트리 형제 pane 집합"이었음. 모바일에서 목록이 안 보인 건 UI 버그가 아니라 wire 설계.

### 적용된 변경

| 파일 | 변경 |
|---|---|
| `ui/src/App.tsx` | `deriveFocusedTerminal`이 인벤토리와 포커스를 분리. 브라우저 탭 포커스/무포커스에도 페이로드를 발행하고 포커스 필드만 null. 워크트리 필터 제거 → 활성 프로젝트의 **모든** 터미널 탭·pane 노출. 각 엔트리에 `worktreeSlug`/`worktreeLabel` 부착. `null` 반환은 터미널 탭이 0개일 때만 |
| `src-tauri/src/remote/protocol.rs` | `RemoteTerminalTabInfo`에 `worktree_slug`/`worktree_label` optional 필드 (serde default + skip_serializing_if → 하위 호환) |
| `src-tauri/src/ipc/remote.rs` | `sanitize_worktree_text` 신설. 브랜치 라벨은 `/`를 정당하게 포함(`orca/<ws-id>/<slug>`)하므로 슬래시 일반 거부가 아니라 **경로 유출 패턴만** 차단(`/`, `\`, `~/`, `~\`, `file:`, `X:` 접두 + 128자 상한) |
| `ui/src/remote/RemoteSessionList.tsx` | 정규화기가 새 필드 보존. 스트립 게이트 `> 1` → `> 0`. 포커스된 터미널이 없어도 목록 렌더. 엔트리 선택 시 **그 엔트리 자신의 worktree**를 실어 전송(현재 컨텍스트 추정 금지). 다른 워크트리 소속 엔트리는 접근성 이름과 칩에 워크트리 라벨을 병기해 동명 탭 구분 |

교차 워크트리 선택이 데스크탑 가드(`isTerminalTabInWorktree`)를 통과하려면 엔트리별 worktree 전송이 필수임 — 6절의 (a) 수정과 짝을 이룸.

### 검증

| 검증 | 결과 |
|---|---|
| `bunx vitest run src/App.test.tsx src/remote/RemoteUI.test.tsx` | **106 passed** |
| `bunx tsc --noEmit` | exit 0 |
| `cargo check --all-targets` | exit 0 (기존 경고만) |
| `cargo test --lib remote::` | **44 passed / 0 failed** |
| `bun run test` (ui 전체) | 1007 passed / 10 failed — 전부 `SettingsDialog.test.tsx`·`appearanceThemeContract.test.ts`. 두 파일 모두 다른 세션의 SettingsDialog 분할 리팩터링(`components/settings/*`) 진행분이며 본 변경과 무관 |
| 라이브 데몬 실측 | 변경 전 `remoteGetActiveSelection` → `null`. 재발행 후 `selection` 채워짐(구 번들 기준 1엔트리) |

계약 변경으로 기존 단정 9건을 새 계약으로 갱신(삭제·스킵 없음). 대표적으로 `publishes only same-worktree terminal tabs` → `publishes terminal tabs from every worktree with safe labels`, 브라우저 탭 시 `toBeNull()` → 인벤토리 유지 + 포커스 필드 null. 브라우저 탭 비노출과 절대경로 비유출 단정은 유지.

### 번들 이중 스테일 함정 (중요)

데스크탑과 모바일이 **서로 다른 번들**을 먹음:

- 데스크탑(`bun tauri dev`) → Vite dev 서버. 프론트 수정은 HMR/리로드 후 반영
- 모바일 리모트 → Axum이 `ui/dist`를 서빙(`server.rs:1235 resolve_dist_dir`). `ui/dist`는 gitignore된 빌드 산출물이라 **`bun run --cwd ui build` 전까지 옛 UI가 그대로 나감**

실측: `ui/dist`는 13:41 빌드, 리모트 소스 수정은 13:48 → 모바일만 구 UI. 재빌드 완료. 발행 페이로드가 구 코드 형태(엔트리 1개, `worktreeLabel` 없음)인지로 데스크탑 번들의 신선도를 판별할 수 있음.

### 남은 사용자 조치

데스크탑 앱 창을 리로드해야 새 발행 로직이 적용됨(실행 중 창은 여전히 구 번들). 그 뒤 모바일 새로고침 시 전체 탭/pane이 목록에 뜨고, 다른 워크트리 pane도 선택 가능.

## 8. "Waiting for Ferryx Desktop confirmation..." — why it never resolved (2026-08-28)

The message is the remote client's *pending* state: the phone sent the selection, the gateway
answered `200`, and the phone is now waiting for the desktop to **republish** an active selection
that matches what was requested (`modelConfirmsSelection`, `RemoteApp.tsx:161`). It never resolved
because of two independent defects.

### 8.1 Delivery: the desktop was never told (root cause)

The gateway runs inside the **daemon** process (`daemon/server.rs:355-370`), not the GUI. When a
remote client asks for a selection, `select_workspace` calls
`state.emit_desktop_event(REMOTE_SELECTION_REQUEST_EVENT, ...)` (`remote/server.rs:638`).
`emit_desktop_event` (`remote/state.rs:184`) forwards to `desktop_event_sink` when installed, and
otherwise only broadcasts to WebSocket clients.

`set_desktop_event_sink` had **zero production callers** — only three in `remote/tests.rs`. The
daemon protocol also had no server→client control push (`DaemonStreamMessage` carries only
`Output|Gap|Lagged|AgentState|Exit`, per attached session). So the GUI's listener
(`ui/src/lib/tauri.ts:648`) waited on an event nothing emitted.

Proven live: after a real phone tap, instrumentation at the top of `handleRemoteSelectionRequested`
logged **0 events**. Remote→desktop selection had never worked in the shipping configuration.

Fix — a control channel mirroring the existing attach stream:

| File | Change |
|---|---|
| `daemon/protocol.rs` | `SubscribeRemoteEvents` request, `SubscribeRemoteEventsOk` response, `DaemonRemoteEvent { event, payload }` frame |
| `daemon/server.rs` | installs `set_desktop_event_sink` at construction → broadcast; the subscribe arm streams events as newline JSON |
| `daemon/client.rs` | `subscribe_remote_events()` → handshake, subscribe, reader task feeding an `mpsc::Receiver` |
| `lib.rs` | `start_remote_event_bridge` re-emits daemon events as Tauri events, resubscribing when the daemon restarts |

### 8.2 Confirmation: an unconfirmed selection stranded the UI

`selectContext` set `pending` plus the waiting message with **no terminal state**, and
`RemoteSessionList` disables every chip while `pending !== null`. One unconfirmed tap therefore
disabled the whole picker until the page was reloaded — which is why retrying looked identical.

Fix: `CONFIRMATION_TIMEOUT_MS` (6s) armed on request and cancelled by `clearPendingSelection`;
on expiry the pending selection clears and the status reads
`Ferryx Desktop did not respond. Tap to try again.`

### 8.3 Server-side rejection worth knowing

`select_workspace` returns `400` *before emitting any event* when the workspace is not registered,
or when a `tabId` is not present in the desktop's **last published** selection
(`remote/server.rs:613-627`). A pane the desktop has not published cannot be selected remotely.

### 8.4 Evidence

- `daemon::server::tests::test_remote_select_request_reaches_a_subscribed_desktop_client` — real HTTP
  `POST /api/v1/workspace/select` with a paired Control device → gateway → sink → daemon UDS
  subscriber; bounded 10s so a regression fails instead of hanging.
- `daemon::server::tests::test_subscribe_remote_events_streams_desktop_directed_events` — frame contract.
- `RemoteUI.test.tsx > recovers from a desktop that never confirms so the picker stays usable` —
  captured RED (status stuck on "Waiting...") before the fix, GREEN after, fake timers, no sleeps.
- Full suite: 112 files / 1019 tests passed; `cargo test --lib daemon::` 39, `remote::` 44;
  `cargo check --all-targets` and `tsc --noEmit` clean.

### 8.5 Going live

The running daemon (pid 38127, started 13:29:55) predates this binary, so it still lacks the sink and
the subscribe verb. The rebuilt bundle is at `src-tauri/target/debug/Ferryx.app` (15:25). No launchd
agent owns the daemon, and the GUI respawns it from `current_exe` (`daemon/client.rs:337`), so
quitting the app, killing the stale daemon, and relaunching is sufficient. That restart terminates
running PTY sessions, so it is left to the operator to time.
