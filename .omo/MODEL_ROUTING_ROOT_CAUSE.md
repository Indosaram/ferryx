# 카테고리 모델 라우팅 실패 근본 원인과 수정 (2026-08-22)

## 증상
`task(category="quick")` 등 카테고리 위임이 항상 실패:
`No available model for category "quick" (attempted deepseek-v4-flash-free)`
호스트 프로세스를 재시작해도 동일했다.

## 규명 과정에서 배제된 것
- 레지스트리 정상: 클린 Node 재현에서 `ModelRegistry.getAvailable()`에
  `quotio/gemini-3.7-flash-high` 포함 23개 quotio 모델 존재.
- 로컬 프록시(Quotio 앱, 127.0.0.1:8317) 정상 구동 중.
- `~/.omo/agent/omo.json`의 categories 설정은 올바름(quick → quotio/gemini-3.7-flash-high).
- 프로세스 캐싱도 원인이 아니었다(새 프로세스에서도 재현됨).

## 진짜 원인 — 스테일 설정 그림자 레이어 2개

omo-task 확장의 설정 로더(`ia`/`Js`)는 다음을 순서대로 병합한다:

1. **user 레이어**: `~/.omo/omo.jsonc` (존재하면 `~/.omo/omo.json`보다 **우선**)
2. **project 레이어**: cwd부터 홈까지 각 조상 디렉토리의 `<dir>/.omo/omo.json(c)`
3. harness 섹션 병합: `[senpi]` 키가 최상위 `categories`를 **덮어씀**

### 원인 1: `/Users/indo/code/project/.omo/omo.json`
조상 디렉토리(`/Users/indo/code/project`)에 8월 14일자 `[senpi]` 섹션이 있었고,
`quick`/`unspecified-low`/`small-impl`을 접두사 없는 `deepseek-v4-flash-free`(해석 불가)로 지정.
→ 이것이 에러 메시지의 `attempted deepseek-v4-flash-free`의 출처.

**수정**: categories 블록 제거 (`agents`의 explore/librarian → quotio/gemini-3.7-flash-high 매핑은 유지).

### 원인 2: `~/.omo/omo.jsonc`가 `~/.omo/omo.json`을 가려냄
로더는 `.jsonc`를 우선 읽는데 jsonc(8월 20일)에는 categories가 없어서,
올바른 전역 설정이 담긴 omo.json(8월 21일)이 통째로 무시되고 있었다.
(jsonc 내용인 codegraph/migrations는 모두 omo.json에도 존재 → 손실 없음)

**수정**: `~/.omo/omo.jsonc` → `~/.omo/omo.jsonc.bak-was-shadowing` 으로 rename.

## 검증
- 수정된 파일들 JSON 파싱 통과.
- 로더와 동일한 병합 시뮬레이션으로 최종 확인:
  - `categories.quick` → `{"models":[{"model":"quotio/gemini-3.7-flash-high"}]}`
  - `categories.unspecified-low` → 동일
- 참고: omo-task 플래너는 호스트 프로세스 시작 시점에 설정을 1회 스냅샷하므로,
  디스크 수정 후에는 **탭/세션 재시작이 1회 필요**하다.

## 남은 조치 (사용자)
- 이 세션의 cmux 탭을 한 번 더 닫고 새로 열 것. 새 프로세스에서 quick 카테고리는
  `quotio/gemini-3.7-flash-high`로 라우팅된다.
