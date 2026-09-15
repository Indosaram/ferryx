# [RFC/PR] Session Lifecycle Management: Lazy Resume & Background Hibernation

## 1. 배경 및 문제 정의 (Background & Problem)
Ferryx는 여러 Senpi 세션을 탭과 워크스페이스로 관리하는 훌륭한 UI/환경을 제공합니다. 하지만 현재 앱 시작 시(특히 macOS 재부팅 후 창 자동 복원 시) **이전에 열려 있던 모든 탭의 백그라운드 세션 프로세스를 즉시 일괄 실행(Eager Spawn)**하는 구조입니다.

### 실측 현황
- 재부팅 직후 Ferryx 데몬(`ferryx --daemon`)이 이전 21개 세션을 동시에 스폰.
- 세션당 약 1.4GB씩 점유하여 **앱 구동 직후 순수 에이전트 프로세스만으로 31.14 GB의 RAM이 즉시 강제 점유**됨.
- 사용자는 당장 1~2개 세션만 작업하려 해도 나머지 19개 세션이 백그라운드에서 메모리를 계속 쥐고 있음.

---

## 2. 제안하는 변경 사항 (Proposed Changes)

### A. 세션 지연 복원 (Lazy Session Resume on Focus)
* **동작 방식**:
  - 앱 시작 시 세션 프로세스(`senpi ...`)를 일괄 스폰하지 않습니다.
  - UI 탭 목록에는 저장된 메타데이터(프로젝트 경로, 마지막 대화 요약, 세션 ID)만 렌더링하고 상태를 **`Sleeping` (또는 `Standby`)** 상태로 둡니다.
  - **사용자가 해당 탭을 클릭하여 포커스하거나 메시지를 보낼 때 비로소 프로세스를 스폰**(`senpi --resume --session <id>`)합니다.
* **Tauri / Rust 백엔드 구조**:
  ```rust
  pub enum SessionProcessState {
      Standby { session_id: String, cwd: PathBuf },
      Running { pid: u32, child: Child },
      Hibernated { session_id: String, last_active: Instant },
  }

  impl SessionManager {
      // 앱 시작 시에는 메타데이터만 로드
      pub fn restore_workspace(&mut self) {
          for session in self.load_saved_sessions() {
              self.sessions.insert(session.id, SessionProcessState::Standby { ... });
          }
      }

      // 사용자가 탭을 클릭하거나 활성화할 때 스폰
      pub async fn activate_session(&mut self, session_id: &str) -> Result<()> {
          if let SessionProcessState::Standby { .. } = self.sessions.get(session_id) {
              self.spawn_senpi_process(session_id).await?;
          }
          Ok(())
      }
  }
  ```

### B. 비활성 세션 자동 동면 (Auto-Hibernation / Suspend)
* **동작 방식**:
  - 백그라운드 탭에서 작업이 완료되고 일정 시간(예: 30분 기본값, 설정 가능) 동안 추가 입력이 없는 세션은 프로세스를 정상 종료(`graceful close`) 처리.
  - 세션 히스토리는 디스크(JSONL/DB)에 보존되어 있으므로 탭 상태를 `Hibernated` 아이콘(💤)으로 전환.
  - 사용자가 다시 해당 탭을 열면 백그라운드에서 투명하게 `--resume`으로 1~2초 내에 복원.

### C. 설정(Preferences)에 복원 정책 추가
* `Settings > General > Session Restore Policy`:
  1. **Lazy (권장, Default)**: 탭 목록만 표시하고 탭 진입 시 프로세스 구동
  2. **Active Only**: 재부팅 전 마지막으로 보고 있던 1개 탭만 실행
  3. **Eager (기존 방식)**: 모든 탭의 프로세스 즉시 실행

### D. 탭 컨텍스트 메뉴에 수동 제어 추가
* 탭 우클릭 메뉴:
  - **"세션 절전 (Hibernate Session)"**: 즉시 프로세스를 닫고 메모리 확보
  - **"세션 재시작 (Restart Session)"**: 누적된 메모리 누수 리셋

---

## 3. 기대 효과 (Expected Impact)
* **부팅 직후 초기 메모리 점유**: **31 GB → ~650 MB (Ferryx UI 자체 점유만 유지)**
* 사용자가 실제로 포커스한 1~2개 활성 세션(약 2~3GB)만 메모리를 사용하므로 **RAM 점유율 90% 이상 절감**.
* 대규모 멀티 세션(20~50개)을 띄워두고 작업하는 파워 유저 환경에서 메모리 압박 완전 해소.
