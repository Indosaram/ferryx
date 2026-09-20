# DAG Viewer의 SSH·Paired 원격 세션 지원 설계

- 기준일: 2026-09-19
- 상태: 구현 전 설계 제안
- 대상: Ferryx — Rust 기반 Tauri v2 multi-workspace terminal & worktree manager
- 문서 경로: `docs/DAG_VIEWER_REMOTE_SSH_PAIRED_DESIGN_2026-09-19.md`

## 1. 배경 및 현재 아키텍처

### 1.1 Journal에서 Desktop Viewer까지

omo agent extension은 실행한 session의 cwd를 기준으로 다음 위치에 DAG run journal을 기록한다. 등록된 project의 repoRoot와 실제 omo session cwd가 항상 같지는 않다.

```text
<omo session cwd>/.omo/senpi-task/dag/runs/*.json
```

Journal은 `camelCase` schema를 사용한다. 핵심 필드는 `runId`, `rootSessionId`, `status`, `nodes`, `waves[].nodeIds`, `edges`, `diagnostics`이며, 정상적으로 정의된 run status는 `running | completed | failed | cancelled | paused`이다. Rust parser에는 향후 알 수 없는 status를 위한 `Unknown` 처리가 있고, `DagRunSnapshot.rootSessionId`는 optional이다. 필드가 없으면 ownership 기반 badge를 표시할 수 없다. [근거: journal schema](../src-tauri/src/dag/journal.rs#L1-L221)

현재 local pipeline은 다음과 같다.

```text
omo session의 local journal
  -> src-tauri/src/dag/watcher.rs
  -> Tauri event "dag-run-updated": { projectPath, snapshot }
  -> dagStore.applySnapshot(projectPath, snapshot)
  -> dagStore.runsByProject[projectPath][snapshot.runId]
  -> DagPaneBadge
  -> DagGraphView modal
```

`watcher.rs`는 `notify` 기반 감시와 native watch를 사용할 수 없을 때의 1초 polling fallback을 사용한다. 시작 시 snapshot inventory를 읽고, 이후 변경된 snapshot을 전송한다. Directory 순회, journal 읽기와 JSON parsing은 `spawn_blocking`에서 처리하지만 감시 대상 자체는 언제나 해당 프로세스가 접근하는 **local filesystem**이다. 원격 경로 문자열을 넘기는 것으로 원격 감시가 되지 않는다. [근거: watcher](../src-tauri/src/dag/watcher.rs#L1-L280)

`dag_watch_project(projectPath)`는 local canonicalization을 시도하고, 실패하면 입력 경로를 유지한다. Canonical key별 watcher를 등록하고 `{ projectPath, runs }`를 반환하여 초기 hydration을 수행한다. 이후 watcher의 snapshot은 `{ projectPath, snapshot }` 형태의 `dag-run-updated` event로 전달된다. Remote daemon에서는 `AppHandle`을 요구하는 이 IPC를 호출하는 대신 watcher/parser와 daemon용 event adapter를 결합해야 한다. [근거: DAG IPC](../src-tauri/src/ipc/dag.rs#L1-L126)

### 1.2 Watch root 수집과 2026-09-19 변경

`ui/src/App.tsx`는 등록된 project repoRoot 중 `project.target?.kind !== 'ssh'`인 경로, worktree 경로, active workspace session cwd를 수집한다. Active workspace가 SSH이면 해당 workspace의 worktree/session cwd 수집을 비활성화한다. 이 조건은 `pairedDaemon`을 제외하지 않는다. [근거: App의 watch root 수집](../ui/src/App.tsx#L959-L1028)

2026-09-19의 uncommitted 변경에는 `dag_discover_watch_roots` IPC가 추가되었다. 실제 구현은 각 registered project root의 **parent 아래 sibling directory를 한 단계만** 살펴보고 `.omo/senpi-task/dag/runs`가 있는 경로를 수집한다. 전체 filesystem을 재귀 탐색하는 기능이 아니다. 이 변경은 agent session이 다른 cwd에서 resume되어 기존 watch root 밖에 journal을 기록하던 local 문제를 해결하기 위한 것이다. Remote filesystem 접근 기능이나 SSH transport를 추가한 것은 아니다. [근거: sibling discovery](../src-tauri/src/ipc/dag.rs#L128-L172)

### 1.3 Badge ownership과 진입점 제한

`DagPaneBadge.tsx`의 2026-09-19 uncommitted 변경은 모든 project bucket의 run을 검색하여 다음 조건으로 소유자를 찾는다.

```text
run.rootSessionId === session.providerSession?.id
```

이 matching은 **ownership-only, path-independent**이다. 기본값 `retainSettled = false`에서는 `status === 'running'`인 run만 badge 대상으로 선택한다. 따라서 올바른 remote snapshot과 provider session identity만 전달되면 synthetic project key에서도 기존 badge와 graph modal을 재사용할 수 있다. [근거: badge selector](../ui/src/components/dag/DagPaneBadge.tsx#L9-L93)

현재 제품에서 badge는 viewer modal의 유일한 진입점이며 `retainSettled`를 사용하는 제품 caller는 없다. 완료되거나 실패한 run을 다시 여는 진입점이 없고, `cancelled`와 `paused`도 기본 running-only filter를 통과하지 못한다. Store에 snapshot이 남아 있는 것과 viewer를 다시 열 수 있는 것은 별개다. Settled run 재진입은 본 설계의 transport 구현과 분리한다.

## 2. 문제 정의

### 2.1 SSH bridge session

`target.kind === 'ssh'`인 session은 `ferryx-remote-helper bridge --stdio`를 통해 원격 terminal을 사용한다. 이때 omo journal은 **remote machine filesystem**에 생성되지만 desktop watcher는 local filesystem만 읽는다.

현재 App은 SSH project를 watch root에서 명시적으로 제외한다. 기준일 소스의 `src-tauri/src/ssh/`와 `src-tauri/src/remote/`에는 DAG 관련 코드가 없으며, journal을 desktop으로 전달하는 transport도 없다. SSH session에서 실행한 DAG가 viewer에 나타나지 않는 원인은 badge의 경로 비교가 아니라 **remote journal 수집·전송 경로의 부재**다. SSH 제외 조건만 제거하는 것으로 해결되지 않는다.

### 2.2 Paired daemon session

`target.kind === 'pairedDaemon'`에서는 remote machine의 Ferryx daemon이 PTY를 소유한다. Journal 역시 remote machine에 기록된다.

그러나 paired project는 현재 local DAG watching에서 제외되지 않는다. Desktop은 `dag_watch_project(remoteRepoRoot)`를 호출하여 remote 경로를 자기 filesystem에서 읽으려고 한다. 보통 존재하지 않는 local 경로이므로 remote run을 얻지 못하며, 같은 문자열의 local 경로가 우연히 존재하면 오히려 무관한 local journal을 읽을 위험이 있다. [근거: App filter](../ui/src/App.tsx#L961-L1008), [근거: local 경로 해석](../src-tauri/src/ipc/dag.rs#L46-L90)

정상적으로 연결된 paired pane에는 badge component가 mount되지만, 해당 remote run snapshot이 store에 들어오지 않아 표시할 것이 없다. `paired-spawning` 또는 expired 상태의 pane은 badge mount 전에 early-return한다. 따라서 두 현상은 구분해야 한다. 전자는 데이터 경로 부재이고, 후자는 pane lifecycle 정책이다. 이번 작업은 정상 연결 pane에 필요한 데이터 경로를 우선 제공한다.

## 3. 목표와 비목표

### 3.1 목표

1. Paired session에서 시작한 running DAG를 desktop의 기존 badge와 graph modal에서 볼 수 있게 한다. SSH bridge session 지원은 2단계로 제공한다.
2. Snapshot과 provider session identity를 함께 전달하여 소유 pane에만 run을 표시하고, 다른 cwd에서 resume한 session도 적절한 remote watch root를 통해 수집한다.
3. 최초 연결·reconnect·desktop 재시작 이후 현재 snapshot을 rehydrate하고, DAG 처리 때문에 terminal input/output, resize, heartbeat가 대기하지 않도록 한다.
4. Local·paired·SSH 경로의 소유 machine을 구분하고, `RemoteApp`에 absolute path나 raw journal이 노출되지 않도록 한다.

### 3.2 비목표

DAG 실행·취소·수정 API, graph renderer 재작성, journal schema 전면 변경, 원격 파일 전체 동기화는 포함하지 않는다. `RemoteApp`의 직접 DAG rendering, remote node artifact 다운로드, settled run 재진입용 `retainSettled` caller, journal deletion/GC 정책은 별도 후속 작업이다.

Remote transport 지원을 “실행 이력 전체를 항상 재열람할 수 있음”으로 설명하지 않는다. 완료 snapshot은 store에 반영하더라도 현재 badge의 running-only 정책은 유지된다.

## 4. 설계안

### 4.1 핵심 결정: Paired-first

Paired 환경에는 이미 PTY를 소유하는 remote Ferryx daemon과 daemon protocol 연결이 있다. **파일이 존재하는 machine에서 watcher를 실행하고 snapshot을 기존 연결로 전달한다.** Desktop이 remote filesystem을 local처럼 읽도록 만들지 않는다.

```text
[remote machine]
omo journal
  -> remote Ferryx daemon의 local DAG watcher/parser
  -> workspace별 snapshot registry와 bounded event queue
  -> 기존 daemon protocol / paired 연결

[desktop machine]
인증된 paired transport receiver
  -> workspace mapping, epoch/sequence 검증, project key 변환
  -> Tauri "dag-run-updated" { projectPath: syntheticKey, snapshot }
  -> dagStore
  -> 기존 ownership badge + graph modal
```

UDS는 해당 machine 내부의 daemon 연결에, WS는 기존 remote/paired 연결 경로에 맞추어 사용한다. UDS 자체를 machine 간 transport로 간주하지 않는다. 기존 terminal 연결을 재사용하되 DAG 처리 queue와 terminal 처리의 backpressure를 분리한다.

### 4.2 필수 전제: providerSession identity의 end-to-end 보존

다음 조건이 성립하기 전에는 paired DAG 표시 기능을 완료로 판단하지 않는다.

```text
desktop paired pane의 session.providerSession.id
  === remote omo journal의 rootSessionId
```

Remote daemon은 agent-detection metadata인 `providerSession`을 session의 초기 payload, live metadata update, reconnect/reattach 시 재전송되는 payload에 포함해야 한다. PTY의 `sessionId`, `backendSessionId`, project path 또는 pane ID는 provider session ID의 대체값이 아니다.

이미 daemon protocol에는 `AgentProviderSession`, `AgentStateReport.provider_session`, `DaemonStreamMessage::AgentState.provider_session`이 정의되어 있다. 따라서 “타입이 전혀 없다”고 전제하지 않고, paired 연결에서 실제로 감지·전송·복원되어 UI session에 도달하는지를 검증한다. [근거: provider type](../src-tauri/src/daemon/protocol.rs#L25-L50), [근거: metadata frame](../src-tauri/src/daemon/protocol.rs#L523-L597)

Snapshot이 metadata보다 먼저 도착하면 store에 보관하고, 유효한 identity가 도착할 때 기존 selector가 badge를 표시하도록 한다. Identity가 없거나 다르면 임의의 path fallback으로 연결하지 않는다. Resume 또는 provider 교체 시 이전 identity가 새 session에 남지 않도록 한다.

**구현 전 조정이 필수다.** 이 metadata 경로는 다른 agent session의 in-flight remote 작업과 겹친다. 특히 `src-tauri/src/ssh/*`, `src-tauri/src/terminal/remote.rs`의 schema·detection·reconnect 변경 담당자와 payload 계약, generation 처리, 변경 소유권을 먼저 합의해야 한다. 현재 dirty 여부만으로 해당 파일에 병행 작업이 없다고 판단해서는 안 된다. 본 문서 작성 작업은 이 파일들을 수정하지 않는다.

### 4.3 Remote watcher와 root 소유권

Remote daemon은 자기에게 등록된 workspace repoRoot, worktree, 실제 PTY/omo session cwd를 기준으로 watcher를 관리한다. `src-tauri/src/dag/watcher.rs`와 journal parser를 재사용하되 `AppHandle`에 결합된 desktop IPC가 아니라 daemon event sink에 연결한다.

Root canonicalization은 **remote machine에서** 수행한다. Desktop은 remote path를 다시 local canonicalization하지 않는다. 같은 root를 여러 session이 사용하면 watcher를 공유하고, subscription 해제·workspace 제거·daemon 종료 시 watcher와 queue의 lifecycle을 정리한다.

다른 cwd에서 resume한 실행을 놓치지 않도록 remote session cwd를 포함한다. Local sibling-discovery의 아이디어를 재사용할 수 있지만, remote sibling 탐색은 해당 daemon이 허용한 workspace/root 범위 안에서만 수행해야 한다. Journal이 있는 모든 sibling을 remote client가 무제한 열람하게 만들지 않는다. Directory가 나중에 생성되면 polling fallback으로 수집 가능해야 한다.

Desktop의 local root 수집에서는 `ssh`뿐 아니라 `pairedDaemon`도 제외한다. 등록 project, active/inactive worktree, session cwd, sibling-discovery 입력 모두 동일한 target 구분을 적용한다. 이 변경은 remote snapshot 수신 경로와 함께 제공하며 local watching은 유지한다.

### 4.4 Event 계약과 synthetic store key

Daemon에서 desktop으로 전달하는 DAG event의 핵심 payload는 기존 event와 같은 `{ projectPath, snapshot }` 형태로 유지한다. 이때 transport의 `projectPath`는 **remote에서 canonicalize한 root**이다. Workspace 및 연결 identity는 인증된 subscription에 묶어 검증한다.

다음은 구현 시 확정할 **신규 계약 예시**이며 현재 존재하는 API가 아니다.

```typescript
type PairedDagRunUpdatedV1 = {
  type: "dagRunUpdated";
  remoteWorkspaceId: string;
  subscriptionId: string;
  epoch: string;
  sequence: number;
  projectPath: string; // remote canonical root
  snapshot: DagRunSnapshot;
};
```

Desktop receiver는 인증된 paired host와 `remoteWorkspaceId`를 desktop의 stable `workspaceId`에 mapping한 뒤 다음 key를 만든다.

```text
paired:<workspaceId>:<remotePath>
```

여기서 `<workspaceId>`는 desktop에 등록된 paired workspace의 ID이고 `<remotePath>`는 remote canonical root이다. Remote payload가 임의의 desktop workspace ID를 직접 선택하게 해서는 안 된다. 동일 remote path라도 다른 desktop workspace는 별도 bucket에 저장한다.

Desktop 전용 adapter는 다음 형태의 기존 Tauri event로 변환한다.

```typescript
{
  projectPath: `paired:${workspaceId}:${remotePath}`,
  snapshot
}
```

UI는 기존 `dagStore.applySnapshot(event.projectPath, event.snapshot)`을 재사용할 수 있다. Badge는 모든 bucket을 대상으로 ownership을 찾으므로 **badge의 UI 및 path-independent matching 변경은 필요하지 않다**. [근거: store upsert](../ui/src/state/dagStore.ts#L83-L100), [근거: badge ownership](../ui/src/components/dag/DagPaneBadge.tsx#L20-L31)

Synthetic key는 filesystem path가 아니다. Prefix와 workspace ID를 제외한 path suffix를 손실 없이 보존하고, 구분자를 포함한 path를 단순 `split(':')`로 해석하지 않는다. 연결 metadata로 host/workspace를 구분하며 key에서 보안 권한을 추론하지 않는다.

또한 remote root나 synthetic key를 local `dag_watch_project`, `dag_get_run`, `dag_read_node_artifact`에 넘기지 않는다. Graph snapshot 표시는 재사용하되 remote artifact 읽기는 지원 capability가 없으면 비활성화한다. 현재 badge가 `projectPath`를 graph에 전달하고 artifact IPC가 local filesystem을 읽으므로 이 경계를 반드시 검증해야 한다. [근거: graph 전달](../ui/src/components/dag/DagPaneBadge.tsx#L225-L245), [근거: artifact IPC](../src-tauri/src/ipc/dag.rs#L252-L300)

### 4.5 Reconnect 및 hydration

기존 watcher는 변경된 snapshot만 전송하므로 연결이 복구되었다고 해서 변하지 않은 journal이 자동 재전송된다고 가정할 수 없다. Remote daemon은 subscription 최초 연결과 reconnect마다 현재 inventory를 명시적으로 재전송해야 한다. [근거: snapshot cache 비교](../src-tauri/src/dag/watcher.rs#L113-L137)

구현 계약은 다음 순서를 보장한다.

1. 인증·workspace binding·DAG capability 확인 후 subscription과 새 connection epoch를 설정한다.
2. Inventory snapshot과 live event의 경계를 하나의 barrier/sequence 규칙으로 정하고, hydration 중 발생한 update를 buffer한다.
3. Inventory를 bounded batch로 전송한 뒤 barrier 이후 update를 순서대로 적용한다. Session의 `providerSession` metadata도 rehydrate한다.
4. 이전 subscription/epoch의 지연 frame은 거부하고, gap이나 buffer overflow가 발생하면 명시적으로 resync한다.

`dagStore.applySnapshot` 자체에는 epoch나 오래된 revision 거부 기능이 없으므로 desktop transport adapter가 순서를 검증한 뒤 호출한다. Journal의 `updatedAt`만으로 network ordering을 판단하지 않고, daemon이 관리하는 epoch/sequence를 사용한다.

네트워크 단절은 run의 `failed` 또는 `cancelled` 상태를 뜻하지 않는다. 마지막 snapshot은 유지하되 연결 freshness는 별도로 관리한다. 끊어진 동안 완료된 run도 reconnect inventory에서 최종 상태를 받아야 한다. 현재 UI 정책상 완료 후 badge가 사라지는 것은 transport 실패가 아니다.

### 4.6 Deletion·GC의 부재와 store retention

Journal watcher는 생성·변경 snapshot을 보내지만 run 삭제에 대한 tombstone/deletion event를 보내지 않는다. 현재 store에는 `removeRun`/`reset` API가 있으나 journal 삭제를 자동으로 연결하는 pipeline은 없다. 따라서 local도 remote도 자동 GC가 없으면 run이 store lifetime 동안 계속 축적된다. 이것은 disk에 영구 저장하는 기능을 뜻하지 않는다. [근거: watcher](../src-tauri/src/dag/watcher.rs#L113-L137), [근거: store API](../ui/src/state/dagStore.ts#L1-L139)

Reconnect inventory에 run이 없다는 이유만으로 삭제되었다고 판단하지 않는다. 부분 scan, 일시적인 권한 오류, subscription 범위 차이와 실제 삭제를 구분할 수 없기 때문이다. 1단계 hydration은 additive upsert로 정의한다.

TTL/LRU, workspace 제거 시 eviction, journal GC와 tombstone protocol은 후속 설계 대상이다. 다만 unbounded network queue나 무제한 snapshot reassembly를 허용하는 근거로 이 제한을 사용해서는 안 된다. Store retention과 transport memory bound는 별개다.

### 4.7 Terminal 우선의 비동기·작은 frame

Journal scan, serialization, snapshot inventory 전송을 PTY read/write loop 안에서 동기 수행하지 않는다. DAG용 bounded queue와 비동기 worker를 두고, terminal/control/heartbeat 처리를 DAG queue drain에 의존시키지 않는다.

`(workspace, root, runId)`별로 대기 중인 snapshot은 최신값으로 coalesce할 수 있다. Queue 한도 초과 시 pending resync를 기록하여 최신 inventory를 다시 전달한다. 최종 상태가 조용히 유실된 채 running snapshot으로 고정되어서는 안 된다.

Snapshot 전체가 작다고 가정하지 않는다. Node prompt, error, `diagnostics` 때문에 커질 수 있다. 협상된 최대 frame byte 수와 total snapshot byte 수를 정하고, 큰 snapshot은 bounded chunk 또는 batch로 나누어 terminal frame 사이에 공정하게 전송한다. Reassembly에는 chunk 수·총 byte·동시 snapshot 수·timeout 한도를 둔다. 정확한 수치는 구현 전 기존 transport 한도 및 부하 측정으로 확정한다.

한도를 넘는 snapshot을 몰래 잘라 graph를 손상시키지 않는다. 명시적인 DAG 오류 또는 resync 상태로 처리하고 terminal은 계속 동작하게 한다. “Async”라는 이유만으로 shared socket의 head-of-line blocking이 없어지는 것은 아니므로 작은 frame과 scheduling 정책을 함께 검증한다.

### 4.8 RemoteApp과 absolute path 보호

**Remote web client인 `RemoteApp`에는 absolute path를 전달하지 않는다.** Trusted desktop 내부의 synthetic key는 remote absolute path를 포함할 수 있으므로 이 key 역시 browser에 전송하면 안 된다.

현재 `safeContextText`는 절대 경로 또는 경로를 포함한 context text를 거부하는 방어 로직이다. 그러나 browser에서 수신 후 숨기는 것은 “경로를 전달하지 않는다”는 요구를 만족하지 못한다. `safeContextText`의 취지를 server-side outbound filtering으로 보장해야 한다. [근거: safeContextText](../ui/src/remote/RemoteSessionList.tsx#L77-L86)

1단계는 machine/desktop용 DAG subscription과 browser용 event fan-out을 구분하고, browser에 raw DAG frame을 보내지 않는다. 같은 WS 기반 연결이라는 이유로 모든 client에 broadcast하지 않는다. Subscription 권한은 client가 주장하는 문자열이 아니라 인증된 연결의 역할과 workspace 권한으로 검증한다.

향후 web DAG 기능을 추가할 때는 opaque root ID와 안전한 label만 포함하는 별도 allowlist DTO를 설계한다. `projectPath`뿐 아니라 `providerSession.transcriptPath`, node prompt/error, `diagnostics`, artifact metadata, log/error 문자열도 경로 노출 경로로 검토한다. UI에서 regex 한 번 적용하는 방식만으로 arbitrary journal text의 안전성을 보장하지 않는다.

### 4.9 SSH bridge 지원: 2단계

SSH에서는 remote Ferryx daemon의 DAG 서비스를 사용할 수 있다는 전제가 없다. 따라서 `ferryx-remote-helper`에 local journal watcher/parser와 DAG용 stdio protocol frame을 추가해야 한다.

Helper는 실제 remote PTY/session cwd와 허용된 project/worktree root를 감시하고, desktop SSH bridge는 terminal frame과 DAG frame을 구분하여 수신한다. Desktop에서는 paired와 동일한 snapshot adapter 원칙을 적용하되 예를 들어 `ssh:<workspaceId>:<remotePath>`처럼 target 종류가 구분되는 key를 사용한다. 정확한 SSH key 및 frame 이름은 2단계에서 확정한다.

Helper 배포·버전 호환성, agent identity 검출·전송, stdio framing, reconnect/resync, 대형 snapshot 처리까지 필요하므로 paired보다 변경 범위가 크다. Phase 1 완료를 SSH 지원 완료로 표시하지 않는다. DAG 기능을 지원하지 않는 기존 helper와는 capability negotiation으로 호환시키고, terminal 연결 자체는 유지한다.

## 5. 대안 검토

| 대안 | 장점 | 단점 및 판단 |
| --- | --- | --- |
| Remote daemon watcher + snapshot event | 파일 소유 machine에서 감시하고 기존 store/graph를 재사용한다. | Daemon protocol·metadata·reconnect 구현이 필요하다. Paired 1단계로 채택한다. |
| Journal 원격→로컬 mirroring | Local watcher의 입력 형태를 그대로 사용할 수 있다. | 동기화 지연, partial write, local cache 경로와 remote identity 혼동, 삭제 정책, 민감한 journal의 추가 복제 문제가 생긴다. 기본안으로 채택하지 않는다. |
| SSH/SFTP 주기적 조회 또는 remote filesystem mount | Helper 변경 전 임시 접근 경로가 될 수 있다. | 반복 I/O, 인증·mount 운영 부담, 경로 의미 차이, 대형 journal 비용이 남는다. 현재 desktop watcher가 이미 지원하는 기능으로 오해해서는 안 된다. |
| RemoteApp에서 직접 graph rendering | 향후 browser에서도 graph를 제공할 수 있다. | Desktop paired badge 문제를 직접 해결하지 않으며 별도 안전 DTO·browser store·privacy 검증이 필요하다. 독립 후속 기능으로 분리한다. |

SSH·paired를 동시에 완성하는 방안은 출시 단위가 커지고 in-flight remote 작업과의 충돌 범위를 확대한다. Paired에서 snapshot 계약과 운영 정책을 검증한 뒤 SSH transport를 추가한다.

## 6. 단계별 구현 계획

### 단계 0 — 병행 작업 조정과 계약 확정

Metadata 담당자와 `providerSession`의 초기·live·reconnect 전달 경로를 확정한다. Dirty source와 test는 foreign work로 취급하며 임의 수정·복구·정리를 하지 않는다. Daemon protocol version/capability 전략, authorized watch root, desktop workspace mapping, frame/queue 한도도 합의한다. 현재 provider type의 존재만으로 paired end-to-end 완료를 선언하지 않는다.

### 단계 1 — Paired end-to-end 수직 구현

Remote daemon에 watcher registry와 inventory/resync 기능을 연결한다. 기존 daemon/paired transport에 협상된 DAG event를 추가하고 desktop adapter가 synthetic key로 store를 hydrate하도록 한다. Session metadata 전제를 먼저 또는 함께 충족한다.

Desktop의 local watch root 수집에서 paired를 제외하고 browser fan-out을 차단한다. 기존 badge와 graph를 재사용하되 remote artifact를 local IPC로 읽지 않는 경계를 검증한다. Unsupported daemon에서는 DAG만 unavailable로 처리하며 terminal은 정상 동작해야 한다.

완료 조건은 remote 실행 → snapshot → paired transport → store → 올바른 pane badge → graph modal의 실제 end-to-end 증거다. Type 정의나 local unit test만으로 통과시키지 않는다.

### 단계 2 — SSH helper와 stdio transport

Helper watcher, frame 분리, version/capability 협상, provider metadata, reconnect inventory를 구현한다. Desktop SSH adapter에 같은 identity·ordering·bounded queue·privacy 규칙을 적용한다. 기존 helper 및 mixed-version 환경에서 terminal 회귀가 없는지 검증한다.

### 별도 후속 작업

Settled run을 다시 여는 history/`retainSettled` 진입점, journal/store GC와 tombstone, remote artifact 조회, `RemoteApp` 전용 DAG DTO·renderer를 각각 별도 범위로 관리한다.

### 구현 단계 검증 시나리오

| 시나리오 | 기대 결과 |
| --- | --- |
| Paired running run, 동일 provider ID | 소유 pane에만 badge가 나타나고 graph를 연다. |
| 동일 project의 다른 provider ID | 다른 pane에 run을 표시하지 않는다. |
| Snapshot 선도착, metadata 후도착 또는 identity 누락 | Identity가 일치할 때만 표시하며 path fallback으로 연결하지 않는다. |
| 다른 cwd에서 resume, journal directory 지연 생성 | 허용된 remote root에서 수집되고 local 경로 탐색에 의존하지 않는다. |
| Reconnect 중 update, duplicate, 이전 epoch frame | 현재 inventory로 복구되고 오래된 snapshot이 최신 상태를 덮어쓰지 않는다. |
| 연결 단절 중 run 완료 | 복구 후 completed snapshot을 적용하며 running badge가 계속 남지 않는다. |
| 대형 journal·느린 client·queue overflow | 메모리 사용을 제한하고 resync하며 terminal/control traffic이 DAG 작업에 묶이지 않는다. |
| Local·paired에서 같은 path 또는 runId 사용 | Target/workspace bucket을 혼동하지 않으며 identity 충돌 조건을 별도 검증한다. |
| Browser subscription 및 diagnostics 내 path | Raw frame·absolute path·synthetic path key가 browser outbound payload에 없다. |
| Journal 삭제 또는 settled run | 삭제 event가 없는 retention과 별도 settled-entry 제한을 기존 동작과 혼동하지 않는다. |
| 구버전 daemon/helper 및 unsupported capability | DAG 기능만 제한되고 terminal은 유지된다. |
| Paired-spawning/expired pane | 기존 early-return lifecycle을 깨지 않으며 정상 pane 수신 문제와 구분한다. |

이 표는 **향후 구현의 acceptance criteria**이며 본 문서 작성 중 실행한 테스트 결과가 아니다.

## 7. 리스크와 미해결 질문

### 7.1 Identity와 namespace

`providerSession.id`가 host/workspace를 넘어 충분히 유일한지 확인해야 한다. Store key 분리만으로 badge의 전역 ownership selector까지 격리되는 것은 아니다. 복제된 journal이나 동일 provider session을 여러 pane에서 resume하는 경우 duplicate 표시가 가능한지 검증한다. 충돌이 실재하면 target-scoped ownership은 별도 계약 조정이 필요하며, 이 경우 “badge 변경 불필요” 전제를 재검토한다.

Desktop workspace ID와 remote workspace ID의 수명, paired host 재등록·workspace 재생성 시 stale bucket 처리도 확정해야 한다.

### 7.2 Protocol 및 runtime 결합

추가 capability로 배포할지 protocol version 변경이 필요한지 합의해야 한다. 구버전 decoder가 새로운 event를 자동으로 무시한다고 가정하지 않는다. 기존 watcher의 async runtime·task handle·sink 종료 방식이 daemon lifecycle에 적합한지 검증하고, 필요한 adapter 분리를 최소 변경으로 수행한다.

### 7.3 일관성·용량·복구

Inventory/live barrier, daemon 재시작 epoch, sequence 범위, buffer overflow 복구 방식은 구현 전에 고정해야 한다. Snapshot과 run 수가 커질수록 parsing 비용과 store의 전역 ownership 검색 비용이 증가한다. Frame bound와 queue bound는 1단계 필수 사항이고, 장기 store eviction은 별도 정책이다.

### 7.4 권한·민감 정보

Remote cwd 및 sibling discovery 허용 범위, symlink canonicalization 이후 접근 권한, machine client와 browser client의 subscription 구분을 확정해야 한다. `diagnostics`는 arbitrary JSON이므로 field 이름만 검사하는 redaction으로 안전성을 단정하지 않는다.

### 7.5 사용자 경험과 후속 범위

연결이 끊겼지만 마지막 snapshot이 running인 상태를 어떻게 표시할지 freshness 정책을 정해야 한다. Expired pane과 settled run 재진입은 transport와 다른 문제다. Remote artifact 버튼의 unavailable 처리도 desktop graph의 local filesystem 오용을 막는 acceptance 항목으로 유지한다.

## 8. 문서 작업의 검증 기준

이 문서만 추가하는 DOC-ONLY 작업의 완료 조건은 다음과 같다.

1. 정확히 `docs/DAG_VIEWER_REMOTE_SSH_PAIRED_DESIGN_2026-09-19.md` 한 파일이 새로 존재하고 UTF-8 Markdown으로 읽힌다.
2. 배경 및 현재 아키텍처, SSH·paired 문제 정의, 목표와 비목표, paired-first 및 SSH 2단계 설계, 대안 검토, 단계별 구현 계획, 리스크와 미해결 질문이 모두 포함된다.
3. Source/test/configuration을 수정하거나 foreign dirty file을 restore·clean하지 않는다. 본 문서에 제안된 API나 테스트를 이미 구현·실행했다고 표현하지 않는다.
4. 작업 전후 `git status --short`를 비교하여 이 작업의 기여가 요청된 문서의 `??` 항목 하나뿐임을 확인한다. 기존 dirty file 및 병행 agent의 변경은 foreign/untouched로 별도 기록한다.

문서 구조 검증은 제품 build/test 통과를 대체하지 않으며, 본 설계는 기존 uncommitted sibling-discovery 및 ownership 변경이 기준일 상태대로 존재한다는 전제에서 작성되었다.
