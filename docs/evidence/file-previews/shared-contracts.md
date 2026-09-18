# file-previews — shared preparation handoff (plan task 6, preparation segment)

Worktree `/Users/indo/code/project/orca-lite-wt/preview`, branch `preview`, base
`d0aee651` ("chore(preview): import uncommitted terminal file-link prerequisite").
Machine: darwin arm64 (Apple M4 Max), 2026-09-15. Source repo
`/Users/indo/code/project/orca-lite` was **read only**; nothing in it was touched.

This segment delivers **contracts only**. No renderer, modal, capability server, CSP,
`App.tsx` or invoke registration work is included — those remain plan tasks 1-6.
Wave A (tasks 1-5) may start against the signatures below.

## Ownership map (do not cross these lines)

| File | Owner | State |
| --- | --- | --- |
| `ui/src/lib/filePreviewTypes.ts` + `.test.ts` | task 6 preparation (this segment) | **landed, frozen** |
| `src-tauri/src/ipc/file_preview_contract.rs` + `_tests.rs` | task 6 preparation (this segment) | **landed, frozen** |
| `src-tauri/src/ipc/mod.rs` (module registration) | task 6 | `pub mod file_preview_contract;` added |
| `src-tauri/src/ipc/browser.rs` (shared resolution seam) | task 6 | `resolve_session_cwd` now delegates |
| `ui/package.json`, `ui/bun.lock` | task 6 | react-markdown 10.1.0 / remark-gfm 4.0.1 pinned |
| `src-tauri/src/ipc/file_preview.rs`, `file_preview_tests.rs` | **task 1** | not created here |
| `ui/src/lib/filePreview.ts` + test | **task 2** | not created here — name deliberately avoided |
| `ui/src/components/FilePreviewDialog.tsx` + test | **task 2** | not created here |
| `ui/src/components/FilePreviewText.tsx`, `ui/src/lib/previewMarkdown.ts` + tests | **task 3** | not created here |
| `ui/src/components/FilePreviewImage.tsx` + test | **task 4** | not created here |
| `ui/src/components/FilePreviewVideo.tsx` + test | **task 5** | not created here |
| `App.tsx`, `linkRouting.ts`, `NativeTerminalPane.tsx`, `lib.rs`, `tauri.conf.json` | **task 6 integration** | untouched |

## Locked dependencies

`ui/package.json` pins exact versions (no caret): `"react-markdown": "10.1.0"`,
`"remark-gfm": "4.0.1"`. `ui/bun.lock` carries both with integrity hashes
(`sha512-qKxVopLT…` / `sha512-1quofZ2RQ9EWdeN34S79…`). No CDN, no raw-HTML plugin.
`bun install --frozen-lockfile` → **exit 0**, "Checked 472 installs across 537 packages
(no changes)". A temporary jsdom smoke render of a GFM table through both packages
passed (**exit 0**, `task-6-prep-markdown-deps.log`); that temporary test file was
deleted after the run — task 3 owns the real Markdown tests.

## Frozen TypeScript API — `ui/src/lib/filePreviewTypes.ts`

Declarations only: no React import, no `@tauri-apps/api` import, no request state.

```ts
const FILE_PREVIEW_KINDS = ["text", "markdown", "image", "video"] as const;
type FilePreviewKind = (typeof FILE_PREVIEW_KINDS)[number];

const FILE_PREVIEW_ERROR_REASONS = ["MissingFile", "PermissionDenied", "NotRegularFile",
  "RemoteUnsupported", "TooLarge", "UnsupportedEncoding", "UnsupportedFormat",
  "FileChanged", "ExpiredHandle"] as const;
type FilePreviewErrorReason = (typeof FILE_PREVIEW_ERROR_REASONS)[number];

const FILE_PREVIEW_ENCODINGS = ["utf-8", "utf-16le", "utf-16be"] as const;
type FilePreviewEncoding = (typeof FILE_PREVIEW_ENCODINGS)[number];

const FILE_PREVIEW_COMMANDS = {
  open: "cmd_file_preview_open",
  openChild: "cmd_file_preview_open_child",
  close: "cmd_file_preview_close",
} as const;

const FILE_PREVIEW_LIMITS = {
  textMaxBytes: 2_097_152, maxRenderedLines: 50_000,
  imageMaxBytes: 33_554_432, imageMaxPixels: 40_000_000,
  maxChildHandles: 32, maxConcurrentMediaRequests: 8, streamChunkBytes: 65_536,
} as const;
const FILE_PREVIEW_ZOOM = { min: 0.1, max: 8, step: 0.1 } as const;
const FILE_PREVIEW_EXTENSIONS = {
  markdown: [".md", ".markdown"],
  image: [".png", ".jpg", ".jpeg", ".gif", ".webp"],
  video: [".mp4", ".m4v", ".mov", ".webm", ".ogv"],
} as const;

type FilePreviewSource = {            // source focus identity, triad kept separate
  readonly leafId: string;            // visual pane
  readonly sessionId: string;         // FRONTEND focus identity — focus restore target
  readonly backendSessionId: string;  // DAEMON session — cwd resolution only
  readonly workspaceId: string | null;
};
type FilePreviewTarget = { readonly line: number; readonly col: number | null };

type FilePreviewOpenRequest  = { readonly path: string; readonly backendSessionId: string;
                                 readonly line: number | null; readonly col: number | null };
type FilePreviewChildRequest = { readonly parentHandle: string; readonly relativePath: string };
type FilePreviewCloseRequest = { readonly handle: string };
type FilePreviewHandle = string;

type FilePreviewPayload = {
  readonly handle: FilePreviewHandle;
  readonly displayName: string;                       // display only, never a path
  readonly kind: FilePreviewKind;
  readonly byteLength: number;
  readonly encoding: FilePreviewEncoding | null;      // text/markdown only
  readonly mediaType: string | null;                  // allowlisted MIME, media only
  readonly mediaUrl: string | null;                   // loopback capability URL, media only
  readonly text: string | null;                       // bounded document, text/markdown only
  readonly lineCount: number | null;                  // text/markdown only
  readonly target: FilePreviewTarget | null;          // clamped caret
};
type FilePreviewChildAsset = {
  readonly handle: FilePreviewHandle; readonly displayName: string;
  readonly kind: FilePreviewKind; readonly byteLength: number;
  readonly mediaType: string; readonly mediaUrl: string;
};

type FilePreviewErrorDetails = { readonly reason: FilePreviewErrorReason;
  readonly displayName?: string; readonly byteLength?: number; readonly limit?: number };
type FilePreviewFailure = { readonly reason: FilePreviewErrorReason | null;
  readonly message: string; readonly details: FilePreviewErrorDetails | null };

function filePreviewErrorReason(error: unknown): FilePreviewErrorReason | null;
function isFilePreviewErrorReason(value: unknown): value is FilePreviewErrorReason;

type FilePreviewMarkdownCapability = {   // task 2 provides it, task 3 consumes it
  readonly requestImage: (relativePath: string) => Promise<FilePreviewChildAsset>;
  readonly requestDocument: (relativePath: string) => void;
  readonly openExternalUrl: (url: string) => void;
  readonly remainingChildHandles: number;
};

type FilePreviewRendererProps = {
  readonly payload: FilePreviewPayload;
  readonly generation: number;                       // late events from older generations ignored
  readonly onReload: () => void;
  readonly onExternalOpen: () => void;               // ORIGINAL path/session/line/col
  readonly onFailure: (failure: FilePreviewFailure) => void;
};
type FilePreviewTextProps = FilePreviewRendererProps & {
  readonly target: FilePreviewTarget | null;
  readonly sourceMode: boolean;
  readonly onSourceModeChange: (sourceMode: boolean) => void;
  readonly markdown: FilePreviewMarkdownCapability | null;   // non-null iff kind === "markdown"
};
type FilePreviewImageProps = FilePreviewRendererProps;
type FilePreviewVideoProps = FilePreviewRendererProps;
```

`filePreviewErrorReason` returns `null` for any value outside the nine frozen reasons —
callers branch on the machine reason, never on `message` prose.

## Frozen Rust API — `src-tauri/src/ipc/file_preview_contract.rs`

```rust
pub enum FilePreviewKind { Text, Markdown, Image, Video }            // serde camelCase → "text"…
pub enum FilePreviewEncoding { Utf8, Utf16Le, Utf16Be }              // "utf-8" / "utf-16le" / "utf-16be"
pub enum FilePreviewErrorReason { MissingFile, PermissionDenied, NotRegularFile,
    RemoteUnsupported, TooLarge, UnsupportedEncoding, UnsupportedFormat,
    FileChanged, ExpiredHandle }
impl FilePreviewErrorReason { pub fn as_str(self) -> &'static str; pub fn code(self) -> IpcErrorCode; }

pub fn preview_error(reason: FilePreviewErrorReason, message: impl Into<String>,
                     extra: Option<serde_json::Value>) -> IpcError;   // details.reason + merged extras

pub mod limits {   // u64/usize mirrors of FILE_PREVIEW_LIMITS
    pub const TEXT_MAX_BYTES: u64 = 2 * 1024 * 1024;
    pub const MAX_RENDERED_LINES: usize = 50_000;
    pub const IMAGE_MAX_BYTES: u64 = 32 * 1024 * 1024;
    pub const IMAGE_MAX_PIXELS: u64 = 40_000_000;
    pub const MAX_CHILD_HANDLES: usize = 32;
    pub const MAX_CONCURRENT_MEDIA_REQUESTS: usize = 8;
    pub const STREAM_CHUNK_BYTES: usize = 64 * 1024;
}

#[serde(rename_all = "camelCase")] pub struct FilePreviewPayload {
    pub handle: String, pub display_name: String, pub kind: FilePreviewKind,
    pub byte_length: u64, pub encoding: Option<FilePreviewEncoding>,
    pub media_type: Option<String>, pub media_url: Option<String>,
    pub text: Option<String>, pub line_count: Option<usize>,
    pub target: Option<FilePreviewTarget>,
}
#[serde(rename_all = "camelCase")] pub struct FilePreviewChildAsset {
    pub handle: String, pub display_name: String, pub kind: FilePreviewKind,
    pub byte_length: u64, pub media_type: String, pub media_url: String,
}
pub struct FilePreviewTarget { pub line: u32, pub col: Option<u32> }

pub async fn resolve_local_session_cwd(
    daemon_client: Option<&std::sync::Arc<crate::daemon::DaemonClient>>,
    session_id: &str,
) -> Result<Option<std::path::PathBuf>, IpcError>;
```

Error-code mapping used by `preview_error`: `MissingFile`/`NotRegularFile` →
`INVALID_PATH`, `PermissionDenied`/`FileChanged` → `IO_ERROR`, `RemoteUnsupported`/
`TooLarge`/`UnsupportedEncoding`/`UnsupportedFormat` → `UNSUPPORTED`, `ExpiredHandle` →
`INVALID_ARGUMENT`. Task 1 must construct every refusal through `preview_error`.

## Resolution-only extraction (characterization, not new behavior)

`ipc::browser::resolve_session_cwd` (the private helper behind `cmd_open_file_path`)
had its body **moved verbatim** into `file_preview_contract::resolve_local_session_cwd`;
`browser.rs` now holds a one-line delegating wrapper with the same signature and the
same call site. Order of checks is unchanged: paired-host relay refusal before any
daemon round trip → missing daemon client → `DescribeSession` (bypassing the UI cwd
cache) → `session_cwd_guard` → cwd cache refresh. Nothing about launching, argv
building or editor selection moved; `file_link.rs` is untouched.

Because this is behavior-preserving extraction, its proof is the existing
characterization suite, not a new RED: `cargo test --lib file_link` → **19 passed,
exit 0**, identical to the confirmed `d0aee651` baseline. Task 1 must call
`resolve_local_session_cwd` rather than re-implementing session resolution.

## Test evidence (logs under `docs/evidence/file-previews/task-6-prep/`)

New behavior observed failing for its intended assertion first, then passing:

| Lane | RED (intended assertion) | GREEN |
| --- | --- | --- |
| Rust wire shape | `task-6-prep-rust-red.log`, **exit 101** — `payload_serializes_camel_case_keys` and `child_asset_serializes_camel_case_keys` failed on real key values: left `["byte_length","display_name",…]` vs right `["byteLength","displayName",…]` (5 passed / 2 failed). Not a missing import: the module and both tests compiled and ran. | `task-6-prep-rust-green.log`, **exit 0** — 7 passed, after adding `#[serde(rename_all = "camelCase")]` to the two DTOs. |
| UI contract | `task-6-prep-ui-red.log`, **exit 1** — 9 passed / 2 failed: "never invents a reason from an unknown discriminant" (`filePreviewErrorReason({details:{reason:"SomethingElse"}})` returned the raw string instead of `null`) and the cross-language parity check that the Rust DTOs carry `rename_all = "camelCase"`. | `task-6-prep-ui-green.log`, **exit 0** — 11 passed, after routing the reader through `isFilePreviewErrorReason` and adding the Rust `rename_all` attributes. |

Regression / validation, all captured with explicit exit status:

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib file_preview_contract` | 7 passed; 1320 filtered out | **0** |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib file_link` | 19 passed; 1308 filtered out | **0** |
| `cargo check --manifest-path src-tauri/Cargo.toml` | finished, 19 pre-existing warnings, no new ones | **0** |
| `bun run --cwd ui test src/lib/filePreviewTypes.test.ts` | 11 passed | **0** |
| `bun install --frozen-lockfile` (in `ui/`) | no changes | **0** |
| `bun run --cwd ui build` (`tsc && vite build`) | built in 2.30s (pre-existing chunk-size warning) | **0** |

LSP diagnostics on `ui/src/lib/filePreviewTypes.ts` and `filePreviewTypes.test.ts`:
**no diagnostics**. No sleep-based or timing-dependent test was written. The UI suites
already verified at `d0aee651` (206 tests) were not rerun: none of their inputs changed.

`docs/evidence/file-previews/baseline-verification/` is the independently captured
verifier receipt for prerequisite `d0aee651` (UI 206 / Rust 19 / build, all exit 0),
produced by task `st_01a0a3cf`; it is preserved unmodified and committed alongside this
handoff for provenance.

## Scope boundaries honoured

Four read-only kinds only; no editor, save, LSP or IDE surface; local-only (no remote
host, no gateway); bounded reads, ranges and handle caps encoded as constants; native
codec policy unchanged (recognised containers are not a codec guarantee). No daemon or
app launch/restart, no changes to `main`, no foreign worktree or `:5173` server touched.
