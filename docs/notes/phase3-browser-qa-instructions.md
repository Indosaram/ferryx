# Phase 3 (CMUX Browser Parity) QA 지시문 — Run 2

> 자동화 한계: eval/wait/console/screenshot/cookies/storage는 실제 WKWebView가 필요해 유닛테스트로 검증 불가(에러 경로·순수 로직만 GREEN). 아래 수동 QA가 최종 관문입니다.

## 준비
1. `bun tauri dev` (유일 허용 데스크톱 실행 경로)
2. 프로젝트 열고 브라우저 탭 1개 + 터미널 탭 1개 확보

## CLI 위치
`cargo run --manifest-path src-tauri/Cargo.toml -- browser <subcommand>` (개발 빌드)

## 1. eval
- 브라우저 탭 열어 `browser open --url https://example.com --workspace <ws-id>` → 브라우저 id 메모
- `browser eval --browser-id <id> --script "1+1"` → `2` 출력 확인
- 큰 값: `--script "'x'.repeat(100000)"` → `(truncated)` stderr 표시 확인

## 2. wait
- `browser wait --browser-id <id> --url-contains example.com` → `waited` 출력
- timeout: `--function "return false"` (기본 15s 후 `BROWSER_WAIT_TIMEOUT` 에러 코드 확인)

## 3. console
- 브라우저에서 에러 유발 후: `browser console --browser-id <id>` → JSON 엔트리 출력
- `--errors` 필터, `--clear` 후 재조회 시 비었는지 확인

## 4. screenshot (macOS 전용)
- `browser screenshot --browser-id <id> --out /tmp/shot.png` → 출력 경로에 PNG 실제 생성 확인(이미지 뷰어로 열어 내용 일치)
- Windows/Linux는 `BROWSER_SCREENSHOT_FAILED`(Unsupported) 예상 — 이번 검증은 macOS만

## 5. focus / cookies / storage
- `browser focus --browser-id <id>` → `focused` 출력
- `browser cookies --browser-id <id> get` → JSON 배열
- `browser storage --browser-id <id> local set k1 v1` → `browser storage ... local get k1` = `v1`

## 6. open env 기본값
- 터미널에서 `FERRYX_WORKSPACE_ID=<ws-id> FERRYX_WORKTREE_PATH=<path> ferryx browser open --url https://example.com` → `--workspace/--worktree-path` 플래그 없이 열림 확인

## 7. FERRYX_WORKSPACE_ID export
- 워크스페이스 컨텍스트 터미널에서 `echo $FERRYX_WORKSPACE_ID` → 워크스페이스 ID 출력
- 컨텍스트 없는 세션에서는 미출력(빈값) 정상

## 8. 회귀
- 기존 open/navigate/close/identify/url/title/snapshot/click/fill/keypress 정상 동작 재확인
