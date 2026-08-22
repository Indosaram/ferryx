# 새 프로젝트 추가 UI/UX 파리티 — 원인 및 수정 계획

> 작성일: 2026-08-21
> 요구: 새 프로젝트 추가 플로우를 원본 Orca와 동일하게 맞춘다
> 기준선: `ui/original-dist/assets/AddProjectFromFolderDialog-D7l1NUGY.js` (canonical)

## 현재 불일치

| 항목 | 원본 Orca | rorca 현재 (`ui/src/components/ProjectDialogs.tsx`) |
|---|---|---|
| 진입 | "+" 클릭 → 네이티브 OS 폴더 피커 즉시 오픈 | 텍스트 폼 다이얼로그 |
| 프로젝트 이름 | 폴더명에서 자동 파생(타이핑 없음) | "Workspace id" 수동 입력 |
| 경로 선택 | 피커에서 선택 | "Repository path" 수동 입력 |
| 확인 단계 | 선택 경로를 mono 박스로 표시하는 확인 다이얼로그("Add this folder as a separate Orca project.") | 없음 |
| non-git 폴더 | 별도 확인 모달 안내 | 백엔드 에러 문자열 노출 |

## Orca 참조 동작 (AddProjectFromFolderDialog)

1. 피커에서 `folderPath` 수신 → 확인 다이얼로그: 제목 "Add Project",
   설명 "Add this folder as a separate Orca project.", 경로를
   `break-all font-mono` 박스에 표시.
2. Footer: Cancel(outline) / Add Project(FolderPlus 아이콘, 진행 시 LoaderCircle 스피너).
3. 확인 시 `addRepoPath(folderPath)` → 성공 후 worktree fetch + 기본 체크아웃.
4. git 저장소가 아니면 `confirm-non-git-folder` 모달로 분기.

## rorca 구현 계획

1. **네이티브 폴더 피커**: `@tauri-apps/plugin-dialog`(npm) +
   capabilities에 `dialog:allow-open` 추가(Rust 플러그인은 이미 등록됨, `lib.rs:73`).
2. **플로우 교체**: "+" → (Tauri) 피커 오픈 → 취소 시 다이얼로그 닫음 →
   선택 시 확인 다이얼로그(Orca 문구·레이아웃 재현) → 등록.
   비-Tauri(웹 미리보기)는 기존 수동 폼 유지.
3. **workspaceId 자동 파생**: 폴더 basename을 slugify(공백/안전하지 않은 문자→`-`,
   registry 검증 규칙 준수, `registry.rs:13`)하고 기존 프로젝트와 충돌 시 `-2`, `-3` 접미사.
4. **테스트**: `ProjectDialogs.test.tsx` 갱신 — 피커 mock, 파생/접미사 케이스,
   확인 다이얼로그 스냅샷 수준 단언. 게이트: `bun run --cwd ui test`, `bun run --cwd ui build`.

## 검증

- [x] "+" 클릭 → 네이티브 피커 오픈
- [x] 폴더 선택 → Orca와 동일 문구의 확인 다이얼로그
- [x] 등록 후 사이드바에 폴더명 파생 프로젝트 표시
- [x] 같은 이름 폴더 2개 추가 시 접미사 처리
- [x] bun test / bun build GREEN
