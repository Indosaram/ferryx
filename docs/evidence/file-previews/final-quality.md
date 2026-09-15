# F2: Code Quality Review

## Architecture & Security Review

### 1. File Authority & Capability Isolation
- **Path Traversal Protection**: Relative paths are resolved against the originating terminal session's current working directory (`session.cwd` or `session.worktreePath`).
- **Capability Tokens**: Every opened document is assigned a 256-bit cryptographically random token. Direct filesystem path exposure is prevented in URLs.
- **Child Capabilities**: Markdown relative images and links can only spawn child capabilities under the parent's directory boundary (capped at 64 child handles per document).

### 2. Descriptor & Memory Lifetime
- **No Leaked Handles**: Local files are opened on demand on background worker threads (`spawn_blocking`); file descriptors are closed immediately after reading or range extraction.
- **LRU & TTL Cleanup**: The range server maintains an LRU eviction cache of 10 documents and a 15-minute TTL background expiration sweep.
- **Client Unmount Cleanup**: Component unmount and modal close dispatch `cmd_file_preview_release` to release server-side resources immediately.

### 3. Range Request Safety
- **Single-Range Enforcement**: Satisfiable single byte ranges (`bytes=start-end`, `bytes=start-`, `bytes=-suffix`) return `206 Partial Content` with accurate `Content-Range` headers.
- **Overflow & Multi-Range Handling**: Multi-range requests and out-of-bounds start offsets fail closed with `416 Range Not Satisfiable` carrying `Content-Range: bytes */{total_size}`.

### 4. Markdown & HTML Trust Safety
- **No HTML Injection**: Rendered markdown uses standard AST components from `react-markdown` and `remark-gfm`. `rehypeRaw` is intentionally omitted. Script tags, raw iframe tags, and inline event handlers are completely ignored.
- **URL Sanitization**: Non-http/https/local capability links and images are sanitized or routed through safe capability bridges.

### 5. Dependency Audit & Build Cleanliness
- **Pinned Dependencies**: `react-markdown@10.1.0` and `remark-gfm@4.0.1` with zero peer dependency conflicts.
- **Type Checking & Build**: `tsc && vite build` passes with exit code 0. Zero TypeScript errors. Zero unresolved imports.
- **Rust Diagnostics**: `cargo check --manifest-path src-tauri/Cargo.toml` passes with exit code 0.

## Review Verdict: APPROVED
Zero security vulnerabilities, unbounded resources, or unchecked memory lifetimes detected.
