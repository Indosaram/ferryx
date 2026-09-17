# Ferryx DAG Viewer Full Information Rendering Architecture & Design

**Document Version:** 1.0.0  
**Author:** OmO Coding Agent  
**Date:** 2026-09-17  
**Status:** Proposal / Research  

---

## 1. 개요 및 배경 (Context & Objectives)

OMO가 실행하는 DAG(Directed Acyclic Graph)는 `.omo/senpi-task/dag/` 아래에 실행 그래프, 실시간 이벤트, 노드별 지시문(`prompt`), 산출물(`resultArtifact`), 상세 실행 통계(`runStats`), 동적 수정 이력(`amendHistory`) 등 풍부한 런타임 데이터를 기록합니다.

그러나 현재 Ferryx의 DAG 뷰어(`DagGraphView`, `DagNodeCard`, `DagPaneBadge`)는 전체 데이터의 약 25%만 렌더링하고 있으며, 특히 노드가 수행한 구체적 작업 내용, 결과물 텍스트, 실패 에러 상세, 소모 토큰 및 비용이 완전히 누락되어 있습니다.

본 연구는 **"그래프 캔버스의 시각적 간결함과 직관성"**을 유지하면서도 **"OMO가 제공하는 모든 세부 데이터에 즉시 접근하고 탐색할 수 있는 계층적 렌더링 아키텍처"**를 제시합니다.

---

## 2. 디자인 원칙 및 제약 조건 (Design Principles)

1. **정보 계층화 (Information Hierarchy - Progressive Disclosure)**
   - 캔버스 카드에 모든 텍스트를 욱여넣으면 그래프 전체 구조를 조망할 수 없게 됩니다.
   - **1단계(캔버스 오버뷰)**: 전체 토폴로지, 노드 상태, 진행률, 핵심 경로, 병목, 소요 시간 요약.
   - **2단계(노드 카드 요약)**: 상태, 라우팅, 실행 시간(Duration), 토큰 요약, 태스크 태그, 에러 칩.
   - **3단계(노드 인스펙터 드로어)**: 클릭 시 열리는 우측 슬라이드 패널에서 프롬프트 전문, 결과물 마크다운, 토큰/비용/속도 상세, 에러 스택 확인.
   - **4단계(런 메트릭스 & 수정 이력)**: 상단 헤더에서 런 전체 통계 합계와 동적 그래프 수정 이력 타임라인 제공.

2. **Ferryx 디자인 시스템 및 테마 준수**
   - **색상 원칙**: 활성(`running: indigo-500`), 오류(`failed: rose-500`), 일시중지(`amber-500`)에만 유채색 강조. 완료된 작업은 뉴트럴(`text-foreground/60`, `border-border`) 처리 (녹색 사용 배제).
   - **라이트/다크 테마 호환**: CSS 변수 기반 시맨틱 토큰(`bg-card`, `bg-background`, `border-border`, `text-foreground`, `text-muted-foreground`) 엄수.
   - **모바일/작은 창 호환**: 모바일 리모트 클라이언트에서도 뷰포트 반응형 처리.

3. **수명 주기 보존 (Lifecycle Retention)**
   - DAG가 완료(`completed`)되거나 실패(`failed`)하는 즉시 배지가 사라지는 결함을 해결하고, 최근 완료된 런을 유지하여 사후 분석을 가능하게 함.

---

## 3. UI/UX 계층별 상세 렌더링 방안

### [A] 상단 헤더 & 런 메트릭스 바 (Run Header & Metrics Bar)

현재 헤더는 단순 텍스트("{completed}/{total} done, {running} running")만 표시합니다. 이를 메트릭 요약 바로 고도화합니다.

- **런 상태 뱃지 & 식별자**:
  - 상태 뱃지: `RUNNING` (인디고 펄스), `COMPLETED` (뉴트럴 차분한 뱃지), `FAILED` (로즈 경고 뱃지), `PAUSED` (앰버).
  - 런 이름 및 런 ID: 클릭 시 `dag_...` 클립보드 복사 툴팁.
- **실시간 소요 시간 (Duration)**:
  - 실행 중: 실시간 타이머 `03:42` (초 단위 갱신).
  - 완료 시: 총 소요 시간 `Duration: 4m 12s` (`completedAt - startedAt`).
- **누적 리소스 소모 합계 (Run Stats Aggregation)**:
  - 총 토큰: `142.5k tokens` (Input 120k / Output 22.5k).
  - 총 턴/도구 호출: `18 turns · 12 tool calls`.
  - 총 비용: `$0.24` (USD).
- **수정 이력 뱃지 (Amend Count)**:
  - `amendCount > 0`일 때: `Amend 2회` 버튼 표시 -> 클릭 시 변경/무효화된 노드 타임라인 팝오버 오픈.
- **진단 경고 뱃지 (Diagnostics)**:
  - 진단 이슈 존재 시 노란색 경고 아이콘 표시 -> 클릭 시 세부 경고 목록 노출.

---

### [B] 캔버스 및 웨이브 컬럼 (Canvas & Wave Columns)

- **웨이브(단계) 헤더 복원**:
  - 기존의 `sr-only`를 제거하고, 컬럼 상단에 세련된 반투명 헤더 마커 배치.
  - 마커 표시: `WAVE 1 · 10 tasks`, `WAVE 2 · 1 task (merge)`
  - 병렬 실행 단계와 순차 병합 단계가 한눈에 구분됨.
- **간선 레이어 (DagEdgeLayer) 가독성 강화**:
  - 크리티컬 패스 간선: 굵은 인디고/바이올렛 스트로크 및 부드러운 애니메이션 대시.
  - 완료된 간선: 부드러운 뉴트럴 실선.
  - 대기/미실행 간선: 옅은 점선.

---

### [C] 개선된 노드 카드 (Enhanced NodeCard, 240x88px)

카드가 지나치게 커지지 않도록 가로 240px, 세로 88px의 정밀한 그리드로 구성합니다:

1. **1행 (헤더: 22px)**:
   - 좌측: 상태 글리프 (▶ / ✓ / ✗ / ◌) + 노드 라벨/ID (볼드 12px, 호버 시 툴팁).
   - 우측: 서브에이전트 태스크 태그 `st_01a0` (모노 9px, 연한 배경) + 재시도 태그 `x2`.
2. **2행 (바디: 24px)**:
   - 좌측: 실행 라우팅 칩 (예: `quick` / `hephaestus · gemini-3.8-flash`).
   - 우측: 노드 실행 시간 (`1m 15s` 또는 라이브 `00:45`).
3. **3행 (푸터: 22px)**:
   - 좌측: 토큰/비용 칩 (예: `11.2k tok` / `3 turns`).
   - 우측: 병목 배지 (`blocks 2`) 또는 에러 표시 배지 (`task_error` 클릭 유도).
4. **인터랙션 효과**:
   - 마우스 호버 시: 미세한 확대(`scale-[1.02]`), 그림자 상승, 테두리 강조.
   - 클릭 시: 활성 포커스 링(`ring-2 ring-indigo-500`) 부여 및 우측 인스펙터 자동 오픈.

---

### [D] 우측 슬라이딩 노드 인스펙터 (Node Detail Inspector Drawer)

노드 카드를 클릭했을 때 캔버스 우측에서 420px~480px 너비로 부드럽게 나타나는 상세 정보 패널입니다. (ESC 키 또는 외부 클릭 시 닫힘).

- **인스펙터 상단**:
  - 노드 이름, 상태 뱃지, 태스크 ID 풀스트링 (`st_01a04db8` 복사 버튼 포함).
  - 시작 시각, 완료 시각, 총 소요 시간.
  - 이전/다음 노드 이동 버튼 (키보드 좌우 방향키 탐색 지원).
- **탭 1: 산출물 (Result & Deliverable)**:
  - 성공한 노드의 결과 텍스트(`.omo/senpi-task/dag/results/.../<nodeId>.txt`) 본문 뷰어.
  - 마크다운 및 코드 블록 자동 렌더링.
  - 산출물 메타데이터: 파일 크기, SHA256 해시, 클립보드 복사 버튼.
- **탭 2: 프롬프트 (Prompt)**:
  - OMO가 해당 노드에 지시한 원본 프롬프트(TASK, SCOPE, DELIVERABLE, VERIFY).
  - 긴 텍스트도 스크롤 및 복사가 가능하게 모노스페이스 포맷으로 제공.
- **탭 3: 실행 통계 (Run Stats & Performance)**:
  - 토큰 상세 그리드:
    - Input Tokens / Output Tokens / Total Tokens
    - Cache Read Tokens / Cache Write Tokens (캐시 적중률)
  - 대화 및 속도 그리드:
    - Turns 수, Tool Calls 수
    - Generation Time, Tokens Per Second (TPS)
    - 발생 비용 ($ USD)
- **탭 4: 에러 및 진단 (Error & Diagnostics - 실패 노드 시 기본 활성화)**:
  - 에러 코드 (`task_error`, `timeout`, `cancelled` 등).
  - 에러 메시지 전문 및 스택 트레이스 (붉은색 경고 컨테이너).
  - 실패 시각 타임스탬프.

---

### [E] 런 수명 주기 및 접근성 개선 (Lifecycle Retention)

- **`DagPaneBadge` 완료 런 유지**:
  - `run.status === "running"` 필터를 확장하여, 완료/실패된 최신 런도 최소 5분간(또는 사용자가 닫기 전까지) 배지를 유지.
  - 배지 스타일: 실행 중(인디고 펄스 글로우) -> 완료됨(차분한 뉴트럴 글리프) -> 실패함(로즈 글리프).
- **다중 런 히스토리 선택 드롭다운**:
  - 여러 번의 DAG가 실행되었을 때, 모달 상단 탭에서 과거 완료된 런들을 선택하여 회고(Post-mortem) 가능.

---

## 4. 백엔드 및 데이터 아키텍처 구현 설계

### 1) Rust 백엔드 구조체 확장 (`src-tauri/src/dag/journal.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeRunStats {
    pub runtime_ms: Option<u64>,
    pub turns: Option<usize>,
    pub tool_calls: Option<usize>,
    pub output_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub generation_ms: Option<u64>,
    pub tokens_per_second: Option<f64>,
    pub cost_usd: Option<f64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagResultArtifact {
    pub relative_path: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeSnapshot {
    pub id: String,
    pub label: Option<String>,
    pub prompt: Option<String>,                 // 추가
    pub state: DagNodeState,
    pub depends_on: Vec<String>,
    pub attempt: usize,
    pub route: DagRoute,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<DagNodeError>,
    pub task_id: Option<String>,
    pub run_stats: Option<DagNodeRunStats>,      // 추가
    pub result_artifact: Option<DagResultArtifact>, // 추가
}
```

### 2) 결과 산출물 읽기 IPC 커맨드 추가 (`src-tauri/src/ipc/dag.rs`)

```rust
#[tauri::command]
pub async fn dag_read_node_artifact(
    project_path: String,
    relative_path: String,
) -> Result<String, IpcError> {
    // .omo/senpi-task/dag/results/... 경로 안전 검증 후 파일 본문 반환
}
```

### 3) 프론트엔드 컴포넌트 분할 구조 (`ui/src/components/dag/`)

- `DagGraphView.tsx`: 캔버스 뷰포트, 팬/줌, 선택된 노드 상태 관리, 인스펙터 레이아웃.
- `DagNodeCard.tsx`: 3단 정보 요약 카드 (소요 시간, 토큰 요약, 태스크 태그, 에러 칩).
- `DagNodeInspector.tsx`: 우측 슬라이드 패널 (산출물, 프롬프트, 통계, 에러 4개 탭).
- `DagRunMetricsBar.tsx`: 상단 런 타이머, 누적 토큰/비용 요약 칩, 수정 이력 뱃지.
- `DagAmendHistoryModal.tsx`: 동적 수정 이력 타임라인 팝오버.

---

## 5. 기대 효과 (Expected Impact)

1. **완벽한 관측 가능성(Full Observability)**: 에이전트 서브태스크가 무슨 작업을 받았고, 무엇을 산출했으며, 어디서 왜 에러가 났는지 즉시 클릭 한 번으로 파악 가능.
2. **개발 생산성 극대화**: 터미널 콘솔 로그나 JSONL 파일을 직접 뒤질 필요 없이 Ferryx GUI 안에서 완료 보고서와 코드를 즉시 검토 및 복사 가능.
3. **비용 및 리소스 투명성**: DAG 단위 및 노드 단위의 토큰 소모량과 API 비용을 실시간으로 확인하여 비효율적인 프롬프트나 루프 조기 감지.
