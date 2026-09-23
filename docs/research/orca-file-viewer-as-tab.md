STATUS: 핵심 경로는 핀된 체크아웃에서 확인했다. 수확 일부는 같은 경로를 뒷받침하고, 나머지가 비거나 실패한 칸은 아래 검증 파일이 닫은 것으로 적는다.

# Orca는 파일뷰어를 탭으로 연다

핀: [stablyai/orca](https://github.com/stablyai/orca) `3bb9a4e261f90f566e0b358e878ecbb16366dda2` (2026-09-22). 로컬 체크아웃: `/tmp/orca-src`. 조사 모델은 `xai/grok-4.7`이고, 이 환경에서 그 모델의 thinking level은 `xhigh`다. `:xhigh`를 모델 id에 붙이면 자식 카탈로그가 거절해서, 카탈로그 id를 썼다. 카테고리 라우팅은 쓰지 않았다. Ferryx 소스는 수정하지 않았다.

## 요약

Orca에서 터미널 파일 경로를 열면 팝업이 아니라 워크스페이스 **에디터 탭**이 열린다. 직접 열기는 Cmd(맥)·Ctrl(그 외) 클릭이다. `handleTerminalFileLink`(`terminal-file-link-actions.ts:32`)가 `isTerminalLinkDirectActivation`(`terminal-link-activation.ts:12`)일 때 `openDetectedFilePath`를 바로 부른다(`:35`). 수식자 없는 왼쪽 클릭은 팝오버를 띄우고, 그 기본 행동 "Open file"도 같은 `openDetectedFilePath`를 부른다(`:108`). 일반 파일은 `store.openFile(..., mode: 'edit')`로 간다. `openFile`은 `contentType: 'editor'`로 `openWorkspaceEditorItem`을 부르고, 그 함수가 같은 그룹의 기존 탭을 활성화하거나 `createUnifiedTab`으로 새 탭을 만든다.

`isPreview`는 모달이 아니다. 다음 단일 클릭에 교체되는 탭 플래그다. 터미널 열기 경로는 `preview: true`를 넘기지 않는다.

HTML만 다르다. 로컬 HTML은 `createBrowserTab`으로 브라우저 탭이 되고, 원격 HTML은 `openFileInBrowserTab`의 doc-preview 경로다. `file-preview.ts`는 일반 텍스트·이미지 뷰어가 아니라 이 HTML/문서 표면이다.

Ferryx는 같은 제스처를 싱글톤 `filePreviewController.open`으로 `FilePreviewDialog`(`role="dialog"`)에 연다. `WorkspaceTab`은 `terminal | browser`뿐이라 파일 탭 종류가 없다. 고칠 곳은 다이얼로그 스타일이 아니라 탭 종류와 핸들 수명이다.

## 터미널에서 파일을 여는 경로

`openDetectedFilePath`는 `src/renderer/src/components/terminal-pane/terminal-file-open-routing.ts:126`에 있다.

클릭이 `openDetectedFilePath`에 들어가는 경로는 두 개다.

- Cmd/Ctrl 클릭: `handleTerminalFileLink` `:32-38`. Shift가 같이 눌려 있으면 `openWithSystemDefault: true`.
- 수식자 없는 클릭: `isTerminalLinkActionActivation`(`terminal-link-activation.ts:21`) 후 팝오버. 기본 행동은 `openDetectedFilePath`(`terminal-file-link-actions.ts:108`). OS 열기는 alternate 행(`:71`, `:81`).
- 버퍼 위치 열기: `openFilePathLinkAtBufferPosition`(`terminal-file-link-hit-testing.ts:30`) 안 `:100`.

`openDetectedFilePath`(`terminal-file-open-routing.ts:126`) 안의 가르기:

| 조건 | 호출 | 표면 |
|---|---|---|
| `openWithSystemDefault`이고 OS가 열 수 있음 | `window.api.shell.openFilePath` (`:180`) | 앱 밖 |
| 디렉터리 | 같은 OS 열기 (`:188`) | 앱 밖 |
| 원격이라 OS가 못 염 | `downloadAndOpenRemoteTerminalFile` (`:196`, 정의 `terminal-remote-file-download-open.ts:11`) | 저장 위치 선택 뒤 OS 열기. 취소는 `result.canceled` (`:21`). 주석이 네이티브 저장 대화상자라고 부른다 (`:20`). 인앱 파일뷰어 팝업이 아니다 |
| 로컬 HTML | `openHtmlFileInBrowser` (`:35`, `:204`) → `createBrowserTab` (`:45`) | 브라우저 탭 |
| 원격 HTML이고 plan이 `doc-preview` | `openFileInBrowserTab` (`:212`) | 문서 프리뷰 탭 |
| 그 외 일반 파일 | `store.openFile` (`:256`), `mode: 'edit'` (`:262`) | 에디터 탭 |

일반 파일 호출은 `preview` 옵션을 넘기지 않는다. 두 번째 인자는 `{ forceContentReload: true }`뿐이다 (`:272`).

## 탭 모델

`openFile` (`src/renderer/src/store/slices/editor/actions/open-file-action.ts:15`)은 mode가 diff/conflict/check가 아니면 `contentType`을 `'editor'`로 둔다 (`:18-25`). 그다음 `openWorkspaceEditorItem`을 부른다 (`:31`). `isPreview`로 넘기는 값은 `options?.preview ?? false`다 (`:37`).

`openWorkspaceEditorItem` (`src/renderer/src/store/slices/editor/tabs/workspace-editor-item.ts:7`)은 대상 그룹에 같은 entity가 있으면 `activateTab` (`:26`)하고, 없으면 `createUnifiedTab` (`:30`)한다. 새 창이 아니다.

`TabContentType` (`src/shared/tab-types.ts:20-28`)에는 `'file'`이나 `'preview'` 종류가 없다. 파일은 `'editor'`다. `isPreview` 주석은 "preview tabs get replaced by next single-click open"이다 (`:69`). 같은 파일의 두 번째 열기는 `applyOpenFileToState`가 같은 owner·path의 기존 `openFiles` 항목을 찾아 재사용한다 (`open-file-apply.ts`의 `existing` 검색).

## HTML·문서 프리뷰는 별도 경로

`getWorkspaceFilePreviewPlan` (`src/renderer/src/lib/file-preview.ts`)의 상태는 `browser-tab`, `doc-preview`, `unsupported`다. `openFileInBrowserTab` (`:203`)은 `doc-preview`면 문서 탭을 열고, 아니면 `createBrowserTab` (`:217`)을 호출한다. `openFilePreviewToSide` (`:310`)는 오른쪽 스플릿 그룹에 같은 브라우저/문서 탭을 연다. 팝업이 아니다.

Ferryx의 텍스트·이미지·비디오 프리뷰에 대응하는 Orca 표면은 이 파일이 아니라 에디터 탭이다.

## 탭 본문

일반 파일 탭의 본문은 `EditorEditFileSurface`다. `fileContent.isImage`(`EditorEditFileSurface.tsx:113`)이면 같은 탭 안에 `ImageViewer`(`:115`)를 그린다. PDF는 별도 탭이 아니라 `ImageViewer`가 `mimeType === 'application/pdf'`일 때 그 자리에서 `PdfViewer`를 그린다 (`ImageViewer.tsx:68`, `:220`). `ImageViewerPopup`(`ImageViewer.tsx:365`)은 그 뷰어 안의 확대 오버레이다. 파일을 여는 표면이 아니다.

데스크톱 에디터 트리와 `mobile/src/files`에서 video 뷰어는 NOT FOUND다. Ferryx 모달의 비디오 재생에 대응하는 Orca 데스크톱 탭 본문은 이 핀에서 확인되지 않았다.

모바일은 워크스페이스 탭이 아니라 `MobileFilePreviewScreen` (`mobile/src/files/MobileFilePreviewScreen.tsx:36`)이다. 데스크톱과 같은 탭 종류로 보면 안 된다.

## 수명, 재사용, 복원

- 같은 경로를 다시 열면 새 모달이 기존 내용을 갈아끼우지 않는다. 그룹에 탭이 있으면 활성화하고, 없으면 탭을 만든다 (`workspace-editor-item.ts:26`, `:30`).
- 터미널 경로의 `isPreview`는 false다. 사이드바식 단일 클릭 프리뷰만 다음 열기에 교체된다 (`tab-types.ts:69`).
- 줄·열은 `openFile` 이후 `setPendingEditorReveal`로 그 탭에 넘긴다 (`terminal-file-open-routing.ts:290`).
- 탭은 `worktreeId`를 가진다 (`tab-types.ts`의 `Tab.worktreeId`). 열 때 `activateAndRevealWorkspace`로 그 워크트리를 앞으로 가져온다.

## Ferryx와의 차이

| | Orca | Ferryx |
|---|---|---|
| Cmd/Ctrl 클릭, 또는 팝오버의 Open file | `openDetectedFilePath` → `store.openFile` | `openTerminalToken` → `filePreviewController.open` (`ui/src/lib/linkRouting.ts:293`) |
| 표면 | 워크스페이스 에디터 탭 | `FilePreviewDialog` (`ui/src/components/FilePreviewDialog.tsx:170`, `role="dialog"` `:357`) |
| 동시 파일 | 탭마다 하나, 같은 경로는 재사용 | 싱글톤 `filePreviewController` (`ui/src/lib/filePreview.ts:399`). 두 번째 open이 첫 번째를 교체 |
| 탭 종류 | `contentType: 'editor'` | `WorkspaceTab = TerminalTab \| BrowserTab` (`ui/src/lib/types.ts:231`). `PersistedTab.kind`는 `"terminal" \| "browser"` (`:668`) |
| Shift | OS 기본 앱 (`shell.openFilePath`, `:180`) | 외부 에디터. 이 분기는 유지 |

## Ferryx에 옮기는 방법

다이얼로그를 탭처럼 보이게 바꾸지 않는다. 브라우저 탭을 만든 방식(`createBrowserTab` → `ADD_TAB_WITH_SESSION`)으로 `kind: "file"` 탭을 추가한다.

1. `WorkspaceTab`과 `PersistedTab.kind`에 `"file"`을 넣는다. 필드만 있으면 된다. 경로, `backendSessionId`, line, col, 표시 이름. PTY `sessionId`는 없다.
2. `openTerminalToken`의 프리뷰 분기가 싱글톤 `open()` 대신 탭을 추가하게 한다. 같은 워크트리·같은 경로면 새 탭 대신 그 탭을 활성화하고 줄만 옮긴다. Orca의 `findTabForEntityInGroup` + `activateTab`과 같다.
3. 탭마다 `createFilePreviewController()` 인스턴스를 둔다. 싱글톤을 탭 안에 넣으면 파일 하나만 열린다.
4. 비활성 탭은 언마운트되므로 핸들 해제는 페인 cleanup이 아니라 탭 close에서 한다. 브라우저 탭이 `closeBrowser`로 정리하는 자리와 같다.
5. `kind === "browser" ? … : tab.sessionId`로 터미널이라고 가정하는 분기에 file을 넣는다. 빠지면 `sessionId`를 찾다 깨진다.
6. 영속화는 핸들이 아니라 경로와 줄·열만 저장하고, 복원 때 `open()`을 다시 호출한다.
7. 기존 `FilePreviewText` / `Image` / `Video`는 탭 본문으로 옮긴다. 오버레이와 포커스 트랩은 버린다. Shift+클릭의 외부 에디터는 그대로 둔다.

원격 웹 클라이언트에는 프리뷰 명령이 없으므로 데스크톱만 대상이다.

## 근거

- Orca `terminal-file-link-actions.ts:32`, `:35`, `:108`
- Orca `terminal-link-activation.ts:3`, `:12`, `:21`
- Orca `terminal-file-link-hit-testing.ts:30`, `:100`
- Orca `terminal-file-open-routing.ts:126`, `:180`, `:204`, `:256`, `:262`, `:272`, `:290`
- Orca `open-file-action.ts:15`, `:18-25`, `:31`, `:37`
- Orca `workspace-editor-item.ts:7`, `:26`, `:30`
- Orca `tab-types.ts:20-28`, `:69`
- Orca `EditorEditFileSurface.tsx:113`, `:115`, `ImageViewer.tsx:68`, `:220`, `:365`
- Orca `MobileFilePreviewScreen.tsx:36`
- Orca `src/renderer/src/lib/file-preview.ts:203`, `:217`, `:310`
- Ferryx `linkRouting.ts:293`, `FilePreviewDialog.tsx:170`, `:357`, `filePreview.ts:399`, `types.ts:231`, `:668`

## 미해결

비디오 탭 본문은 이 핀의 에디터·모바일 파일 트리에서 NOT FOUND다. 그 외 열린 질문(클릭 제스처, 이미지·PDF, 모바일 화면, 원격 저장 대화상자, 같은 경로 재사용)은 체크아웃을 직접 읽어 닫았다. 빈 수확 `sibling-route`와 실패한 `click-gesture`는 그 직접 읽기로 대체했다. 검증 기록은 `.omo/ulw-research/20260923-011038/wave-1/verify-wave1.md`다.
