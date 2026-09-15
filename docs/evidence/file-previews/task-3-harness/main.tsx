/**
 * Isolated browser harness for the read-only text/Markdown preview renderer
 * (plan task 3). It mounts the real `FilePreviewText` with the real shipped
 * token layer and a capability implementation that mimics the backend child
 * handle: relative images are resolved over loopback, everything else is
 * refused by the renderer itself.
 *
 * Driven by `docs/evidence/file-previews/task-3-harness/run-qa.mjs`.
 */
import { useCallback, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";

import "@/index.css";
import { FilePreviewText } from "@/components/FilePreviewText";
import type {
  FilePreviewChildAsset,
  FilePreviewFailure,
  FilePreviewMarkdownCapability,
  FilePreviewPayload,
  FilePreviewTarget,
} from "@/lib/filePreviewTypes";

type HarnessEvent = { kind: string; value: string };

declare global {
  interface Window {
    __harnessEvents?: HarnessEvent[];
    __pwned?: unknown;
  }
}

window.__harnessEvents = [];
function record(kind: string, value: string) {
  window.__harnessEvents?.push({ kind, value });
}

const ORDINARY_MARKDOWN = `# Release notes — 릴리스 노트

A read-only preview of an ordinary document. Text wraps inside the modal, and
long identifiers such as \`a9f3c1d7e5b2a8c40f6e1d93b7a5c2e80d4f6a1b93c7e5d20\` stay contained.

## Checklist

- [x] Bounded document read
- [ ] Remote images (never)
- Nested bullets keep their rhythm
  - 한글 본문도 동일한 행 간격을 씁니다

## Handle budget

| Field | Value | Notes |
| --- | --- | --- |
| \`handle\` | \`h-7f2a\` | opaque, no path |
| \`byteLength\` | 4,214 | bounded by 2 MiB |
| \`encoding\` | utf-8 | BOM UTF-16 also decodes |

> Anchors never navigate the WebView: an http(s) link is an explicit external
> open, and a relative document becomes a new preview request.

\`\`\`ts
// Fenced code is literal, even when it contains markup.
const danger = "<script>alert('never executes')</script>";
\`\`\`

![Architecture diagram](./fixtures/diagram.png)

Links: [ferryx.app](https://ferryx.app/docs), [sibling document](./notes.md),
[jump to the checklist](#checklist).
`;

const HOSTILE_MARKDOWN = `# Hostile fixture

<script>window.__pwned = "block-script"; document.title = "pwned";</script>

<img src="x" onerror="window.__pwned = 'img-onerror'">

<iframe src="https://tracker.example/frame"></iframe>

<a href="javascript:window.__pwned='raw-anchor'">raw anchor</a>

Inline <b onclick="window.__pwned='inline'">raw html</b> is shown literally.

[javascript scheme](javascript:window.__pwned='js-link')
[data scheme](data:text/html,<script>window.__pwned='data-link'</script>)
[file scheme](file:///etc/passwd)
[custom scheme](vscode://file/etc/passwd)
[protocol relative](//tracker.example/beacon)
[absolute path](/etc/passwd)
[traversal document](../../secret.md)
[tab-escaped scheme](java	script:window.__pwned='tab')

![remote tracker](https://tracker.example/pixel.png)
![encoded traversal](%2e%2e/%2e%2e/secret.png)
![data image](data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==)
![missing local asset](./fixtures/does-not-exist.png)

\`\`\`html
<script>alert("fenced code is literal text")</script>
\`\`\`

An unbreakable token must not overflow the modal:
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
`;

const PLAIN_TEXT = [
  "# 배포 메모 (release memo)",
  "",
  "1  literal markup: <script>window.__pwned = 'plain-text'</script>",
  "2  emoji columns: 👩‍💻👩x — the caret counts Unicode scalars, not code units",
  "3  CRLF lines decode identically",
  "4  한글 텍스트는 같은 행 번호 규칙을 따릅니다",
  "5  search matches are selected, never rewritten: hello Hello HELLO",
  "6  a long unbroken path must wrap inside the viewport:",
  "7  /Users/indo/code/project/orca-lite/src-tauri/src/ipc/file_preview_contract_with_a_very_long_name.rs",
  "8  tabs\tare\tpreserved",
  "9  trailing whitespace is preserved   ",
  "10 the last line is intentionally empty",
  "",
].join("\r\n");

type FixtureName = "text" | "markdown" | "hostile";

const FIXTURES: Record<FixtureName, { displayName: string; kind: "text" | "markdown"; text: string }> = {
  text: { displayName: "release-memo.txt", kind: "text", text: PLAIN_TEXT },
  markdown: { displayName: "RELEASE.md", kind: "markdown", text: ORDINARY_MARKDOWN },
  hostile: { displayName: "hostile.md", kind: "markdown", text: HOSTILE_MARKDOWN },
};

function payloadFor(name: FixtureName): FilePreviewPayload {
  const fixture = FIXTURES[name];
  const bytes = new TextEncoder().encode(fixture.text).length;
  return {
    handle: `h-${name}`,
    displayName: fixture.displayName,
    kind: fixture.kind,
    byteLength: bytes,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text: fixture.text,
    lineCount: fixture.text.split(/\r\n|\r|\n/).length,
    target: null,
  };
}

const capability: FilePreviewMarkdownCapability = {
  requestImage: async (relativePath: string): Promise<FilePreviewChildAsset> => {
    record("requestImage", relativePath);
    const response = await fetch(`/capability/resolve?path=${encodeURIComponent(relativePath)}`);
    if (!response.ok) {
      const body = (await response.json()) as { reason?: string; message?: string };
      throw {
        code: "INVALID_PATH",
        message: body.message ?? "child asset unavailable",
        details: { reason: body.reason ?? "MissingFile" },
      };
    }
    return (await response.json()) as FilePreviewChildAsset;
  },
  requestDocument: (relativePath: string) => record("requestDocument", relativePath),
  openExternalUrl: (url: string) => record("openExternalUrl", url),
  remainingChildHandles: 32,
};

function readQuery() {
  const params = new URLSearchParams(window.location.search);
  const fixture = (params.get("fixture") ?? "markdown") as FixtureName;
  const targetLine = Number(params.get("line") ?? "0");
  const targetCol = Number(params.get("col") ?? "0");
  const target: FilePreviewTarget | null =
    targetLine > 0 ? { line: targetLine, col: targetCol > 0 ? targetCol : null } : null;
  return {
    fixture: fixture in FIXTURES ? fixture : ("markdown" as FixtureName),
    target,
    initialSourceMode: params.get("mode") === "source",
  };
}

function Harness() {
  const { fixture, target, initialSourceMode } = useMemo(readQuery, []);
  const [sourceMode, setSourceMode] = useState(initialSourceMode);
  const [failure, setFailure] = useState<FilePreviewFailure | null>(null);
  const payload = useMemo(() => payloadFor(fixture), [fixture]);
  const onFailure = useCallback((next: FilePreviewFailure) => {
    record("failure", next.message);
    setFailure(next);
  }, []);

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4 sm:p-8">
      {/* Stand-in for the task 2 modal frame: geometry only, no behavior. */}
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`${payload.displayName} preview`}
        data-testid="harness-dialog"
        className="flex h-[min(88vh,44rem)] w-full max-w-4xl flex-col overflow-hidden rounded-lg border border-worktree-sidebar-border bg-card shadow-2xl"
      >
        <header className="flex h-9 shrink-0 items-center gap-2 border-b border-worktree-sidebar-border px-3">
          <span className="truncate text-[12px] font-medium text-foreground">{payload.displayName}</span>
          <span className="shrink-0 text-[11px] text-muted-foreground">
            {`${payload.kind} · ${payload.byteLength} B · ${payload.encoding}`}
          </span>
        </header>
        <div className="min-h-0 flex-1">
          <FilePreviewText
            payload={payload}
            generation={1}
            onReload={() => record("reload", payload.handle)}
            onExternalOpen={() => record("externalOpen", payload.handle)}
            onFailure={onFailure}
            target={target}
            sourceMode={sourceMode}
            onSourceModeChange={setSourceMode}
            markdown={payload.kind === "markdown" ? capability : null}
          />
        </div>
        {failure ? (
          <footer
            data-testid="harness-failure"
            className="shrink-0 border-t border-worktree-sidebar-border px-3 py-1.5 text-[11px] text-status-warning"
          >
            {failure.message}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Harness />);
