# Exact platform syntax ledger - 2026-09-13

This is a source-location/disposition ledger, not a repeated domain audit,
execution receipt, frozen-tree approval or whole-program correctness claim.

## Reproduction

The complete extractor is embedded as `reproduction.python` in the JSON ledger,
with its SHA256 and invocation in `reproduction.command`. It reads source bytes
and installed parsers only; it never imports project code. Its only writes are
these two deliverables through `apply_patch`. Run from repository root with the
installed Python, Node, TypeScript and ast-grep; no dependency installation.
Subprocess output is captured without truncation or a fixed output cap. Parser
failures abort; explicit recovered errors/gaps are retained. Files are parsed
from captured bytes and hashed again afterward. Moving-source reproduction is
a new snapshot, not byte-identical historical evidence.

## Generated snapshot

Status: **parser-backed inventory delivered; exhaustive Windows semantic branch closure remains INCOMPLETE**. No tests, builds, project entry points, refs, worktrees, SSH or desktop operations ran.

- HEAD `da6eec06d65551f67bbc43f09910cde470c3478d`; index SHA256 `6229b36bc1d7d161d14a8adcc1c0ee43e436867824e80e612a2e2c16473efdd8`; stable during parse: True.
- Immutable census: 1854 file entries / 342 ownership sites, unchanged. Current index: 1862; nonignored untracked: 195; captured source/config files: 968.
- 23316 unique range-and-kind entries; 86 explicit per-file/site gaps. 49 paths differ from census hashes; 0 changed during parsing. This is not a frozen tree.
- Parsers: ast-grep 0.44.1 (built-in Rust/Bash tree-sitter grammars), TypeScript 5.9.3, Python stdlib AST/TOML and PyYAML 6.0.3. No dependency installation.

### Count derivation (disjoint syntax kinds, not a combined branch total)

| Kind | Count |
|---|---:|
| `config-platform-scalar` | 22 |
| `config-reference` | 3305 |
| `literal-reference` | 4405 |
| `python-structural-routing` | 48 |
| `rust-cfg` | 717 |
| `rust-cfg!` | 36 |
| `rust-cfg_attr` | 2 |
| `rust-include-reference` | 55 |
| `rust-module` | 355 |
| `rust-reference` | 493 |
| `shared-conditional` | 8784 |
| `shell-structural-routing` | 14 |
| `source-file-scope` | 968 |
| `ts-comment-reference` | 23 |
| `ts-dynamic-import` | 114 |
| `ts-if` | 431 |
| `ts-import` | 2232 |
| `ts-platform-call` | 110 |
| `ts-reference` | 628 |
| `ts-short-circuit` | 400 |
| `ts-switch` | 1 |
| `ts-ternary` | 173 |

Each count is `Counter(entries.kind)` over unique `(path, byteStart, byteEnd, kind)` IDs. Rust cfg, cfg_attr and cfg! are separate sites; module inclusion, imports, shared conditionals, literal references, configuration scalars and file scopes must not be summed as Windows branches. All TS shared conditionals are retained to expose alias gaps. Arrays have no caps or omitted tails.

### Representative exact entries

- `src-tauri/src/ferryx_scope/ssh/process.rs:266-286:rust-module` (line 7): `None`; **shared-caller**; SHA256 `65713ee808adbcaf7b426c57aefd0236d280e6cbeba9eb7060069c63fceda960`. Ancestor IDs/module routes and receipt links are in JSON.
- `src-tauri/src/native_terminal/renderer/mod.rs:129-158:rust-cfg` (line 7): `target_os = "windows"`; **windows-included**; SHA256 `fb3a4585c9b2913b1196d8891e6fd3e46654bc15ba434b450ce6e4f831303cc9`. Ancestor IDs/module routes and receipt links are in JSON.
- `src-tauri/src/native_terminal/renderer/coretext_font.rs:92-119:rust-cfg` (line 3): `target_os = "macos"`; **windows-excluded-local-predicate**; SHA256 `ca30f938c6b1058cc952125ddc1001dddb7536f01d6c8576bb665dd214764bd2`. Ancestor IDs/module routes and receipt links are in JSON.
- `src-tauri/src/main.rs:77-143:rust-cfg_attr` (line 2): `cfg_attr(not(debug_assertions), windows_subsystem = "windows")`; **conditional-attribute-not-body-exclusion**; SHA256 `7ffbf6aaae22c9ff79c5fe03a70926ee6b584dd8fa975a6e8fc457c3d9c0b221`. Ancestor IDs/module routes and receipt links are in JSON.
- `src-tauri/src/native_terminal/renderer/color_glyph.rs:74-101:rust-cfg` (line 3): `target_os = "macos"`; **windows-excluded-local-predicate**; SHA256 `2d542b4b7c298ef9260c29356f48696de14de86d69969e40064289985eab5301`. Ancestor IDs/module routes and receipt links are in JSON.
- `scripts/lib/release-platforms.mjs:1436-1500:ts-if` (line 39): `!os.ok`; **platform-conditional-unresolved**; SHA256 `26aef8557231bca9a4c8f7e33ac9f588fabe4b490bfce1439451fa3d57f60822`. Ancestor IDs/module routes and receipt links are in JSON.

### Remaining obligations, not hidden completion claims

- No complete-branch claim: unsupported formats, opaque Rust macros/include!/cfg_attr expansion, Cargo roots and symbolic feature/test/architecture cfgs remain explicit.
- TS all structural conditional nodes are retained, but platform selection through imported aliases, function return values, properties, callbacks and data tables is not a complete interprocedural analysis. shared-conditional includes these unresolved candidates.
- TS local alias fixed point is lexical declaration dataflow, not binding/type resolution; shadowing may overselect. No alias is evaluated as a host fact. Imports are shared caller edges, not complete call-graph edges.
- Rust cfg! is an expression site, not an enclosing if/match arm count. Its false result is negative/default reachable; associated runtime arms are not enumerated separately.
- Reference-only fixture/comment entries are syntax strings/comments, not execution evidence. Embedded source strings are opaque and may contain unparsed programs.
- Shell entries count whole AST if/case nodes, not individual arms. Workflow scalar routes are configuration selectors, not AST program branches; matrix and expression expansion unresolved.
- Active P27/P32/P33 paths are marked not frozen even when unchanged. Active ownership sets conservatively include shared immediate callers; task owner final allocation is authoritative.
- All indexed and nonignored untracked paths are accounted; ignored build/vendor/runtime artifacts are outside this Git census. No dependency source or generated macro expansion scanned.
- Accepted evidence links preserve historical source scopes; changed-source hashes do not promote those receipts to approval of new behavior. No domain bug review or runtime testing repeated.

`files` accounts the entire index/untracked union with source hashes, parser disposition, census drift, accepted receipt links and active-owner flags. `snapshot` preserves baseline-to-working and index-to-working name/status deltas without editing the moving census. `gaps` enumerates each unsupported format, parse error, unresolved attribute/module/include case and post-capture mutation. Final owner-freeze reconciliation remains with the lead.
