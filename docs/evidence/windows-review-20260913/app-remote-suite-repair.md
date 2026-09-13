# App remote fixture repair

Task st_01a099d4, C002 registered disjoint allocation, 2026-09-13.

## Outcome

Only ui/src/App.remote.test.tsx and ui/src/App.remoteHostShortcuts.test.tsx changed. The identical focused command passed all 23 cases in one post-fix run (2.76s, exit 0), compared with lead-owned bash61 RED: 2 failed, 21 passed, 3.72s, exit 1. No production edits, full suite/build, test disabling, timeout increases or selector shims.

```sh
CI=1 bun run --cwd ui test src/App.remote.test.tsx src/App.remoteHostShortcuts.test.tsx --reporter=verbose
```

## Root causes and corrected contracts

### SSH restore

sessionPersistence.ts retains persisted SSH backend IDs even when listTerminalSessions does not list them; list absence is not evidence of remote process death. workspaceStore/startSshRecovery queries authoritative remote status without spawning a replacement. This matches docs/evidence/ssh-process-survival/ui-state.md and daemon-routing.md.

The stale fixture asserted a replacement spawn and left getTerminalRemoteStatus unmocked, causing missing Tauri internals. It now controls that real IPC boundary with a typed deferred response, registered before mount/registration resolution, and awaits that exact response in React act. It asserts status queried for old-backend, no ordinary or detailed replacement spawn, restored saved-tab DOM, preserved SSH target, and saved frontend/backend identity plus cwd. App, serialization and SSH recovery remain real. The remote status descriptor models the original remote process rather than manufacturing a new shell.

### Remote-host shortcuts

The fixture omitted mandatory TabPaneLayout.sessionIdsByLeafId and expandedLeafId, carried an invalid terminal lifecycle, and bypassed model checking. Its state now satisfies WorkspaceState with valid terminal, worktree and pane fields. App.handleCloseActiveSurface intentionally calls closePane(tab-1, leaf-1); workspaceStore.closePane handles whole-tab closure when that is the only leaf. This test mocks the store boundary, so expecting its independent closeTab spy to be called by App was stale.

The local close assertion now requires exactly one closePane call with the active tab/leaf and no direct closeTab call. Remote-mode menu and keyboard suppression explicitly assert closePane is not called, in addition to all previous closeTab/open/split suppression checks. The local add action uses async act with an immediate assertion instead of its old polling wait. No new timer wait or polling was added. Existing untouched waits elsewhere were not expanded.

## Verification

Both changed test files: LSP reports no diagnostics. git diff --check passed. The focused execution exercised the real App surface and all 23 original scenarios. No missing-Tauri reconciliation error appears in GREEN output. No monitor tool is exposed to this child; execution was bounded foreground bash, with raw output retained below. No aggregate/full-suite claim.

## Raw results and final diff

Appended directly from parent/child session tool results and git diff below; RED is preserved without rerunning it.

### RED raw tool receipt

```text
{
  text: 'status: exited_1 exit_code: 1\n' +
    '$ vitest run --maxWorkers=1 src/App.remote.test.tsx src/App.remoteHostShortcuts.test.tsx "--reporter=verbose"\n' +
    '\n' +
    ' RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui\n' +
    '\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > shows progress throughout registration, restore, and first SSH tab creation 129ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > shows progress for manual first-tab creation and allows retry after failure 36ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > replaces registration failure with progress on explicit retry 29ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > does not leak a late SSH failure into the selected local workspace 28ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > focuses the existing SSH terminal addressed by its backend identity 38ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > loads a stored remote target without registering its path locally 23ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > returns from a local project to the requested existing SSH session 42ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > opens the registered SSH project from remote without a preexisting terminal 21ms\n' +
    " ✓ src/App.remote.test.tsx > App SSH project lifecycle > handles the chooser's registered remote project through actual App registration 48ms\n" +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > routes the chooser Settings CTA to SSH machines, not inbound Remote Access 17ms\n' +
    " ✓ src/App.remote.test.tsx > App SSH project lifecycle > adopts the server's canonical host-qualified ID and path before new tabs and splits 57ms\n" +
    'stderr | src/App.remote.test.tsx > App SSH project lifecycle > restores remote sessions after native startup and preserves their target in the next save\n' +
    'SSH session reconciliation failed: {\n' +
    "  code: 'UNKNOWN',\n" +
    `  message: "Cannot read properties of undefined (reading 'invoke')",\n` +
    '  details: {\n' +
    "    command: 'cmd_terminal_remote_status',\n" +
    `    raw: "TypeError: Cannot read properties of undefined (reading 'invoke')\\n" +\n` +
    "      '    at invoke (file:///Users/indo/code/project/orca-lite/ui/node_modules/@tauri-apps/api/core.js:202:39)\\n' +\n" +
    "      '    at invokeCommand (/Users/indo/code/project/orca-lite/ui/src/lib/tauri.ts:735:18)\\n' +\n" +
    "      '    at getTerminalRemoteStatus (/Users/indo/code/project/orca-lite/ui/src/lib/tauri.ts:494:10)\\n' +\n" +
    "      '    at /Users/indo/code/project/orca-lite/ui/src/lib/sshRecovery.ts:41:49\\n' +\n" +
    "      '    at Array.map (<anonymous>)\\n' +\n" +
    "      '    at /Users/indo/code/project/orca-lite/ui...'\n" +
    '  }\n' +
    '}\n' +
    '\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > selects inactive remote roots by workspace identity even when a local root has the same path 69ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > leaves a rejected host unavailable instead of spawning locally 29ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > ignores remote registration that resolves after switching to a local project 25ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > rejects malformed stored targets without converting them to local: {"kind":"ssh"} 7ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > rejects malformed stored targets without converting them to local: {"kind":"ssh","hostId":""} 7ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > rejects malformed stored targets without converting them to local: null 7ms\n' +
    ' ✓ src/App.remote.test.tsx > App SSH project lifecycle > rejects malformed stored targets without converting them to local: {"kind":"unknown"} 9ms\n' +
    ' × src/App.remote.test.tsx > App SSH project lifecycle > restores remote sessions after native startup and preserves their target in the next save 22ms\n' +
    '   → expected "spy" to be called with arguments: [ ObjectContaining{…} ]\n' +
    '\n' +
    'Number of calls: 0\n' +
    '\n' +
    ' × src/App.remoteHostShortcuts.test.tsx > Blocker H3: local workspace shortcut suppression when remote host is active > suppresses local shortcut and menu mutations when activeRemoteHost is active 119ms\n' +
    "   → Cannot read properties of undefined (reading 'leaf-1')\n" +
    ' ✓ src/App.remoteHostShortcuts.test.tsx > Blocker H3: local workspace shortcut suppression when remote host is active > suppresses createWorktree modal and creation when activeRemoteHost is active 41ms\n' +
    ' ✓ src/App.remoteHostShortcuts.test.tsx > Blocker H3: local workspace shortcut suppression when remote host is active > suppresses command palette menu and worktree selection mutations when activeRemoteHost is active 64ms\n' +
    ' ✓ src/App.remoteHostShortcuts.test.tsx > Blocker H3: local workspace shortcut suppression when remote host is active > dismisses open command palette and suppresses worktree selection when switching to remote host 34ms\n' +
    '\n' +
    '⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯\n' +
    '\n' +
    ' FAIL  src/App.remote.test.tsx > App SSH project lifecycle > restores remote sessions after native startup and preserves their target in the next save\n' +
    'AssertionError: expected "spy" to be called with arguments: [ ObjectContaining{…} ]\n' +
    '\n' +
    'Number of calls: 0\n' +
    '\n' +
    ' ❯ src/App.remote.test.tsx:375:34\n' +
    '    373|     expect(native.spawnTerminal).not.toHaveBeenCalled();\n' +
    '    374|     await act(async () => { registration.resolve(registered); await registration.promise; });\n' +
    '    375|     expect(native.spawnTerminal).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: remote.workspaceI…\n' +
    '       |                                  ^\n' +
    '    376|     expect(native.registerProject).not.toHaveBeenCalled();\n' +
    '    377|     expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!).find((project: RegisteredProject) => projec…\n' +
    '\n' +
    '⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/2]⎯\n' +
    '\n' +
    ' FAIL  src/App.remoteHostShortcuts.test.tsx > Blocker H3: local workspace shortcut suppression when remote host is active > suppresses local shortcut and menu mutations when activeRemoteHost is active\n' +
    "TypeError: Cannot read properties of undefined (reading 'leaf-1')\n" +
    ' ❯ src/App.tsx:1794:70\n' +
    '    1792|       if (activeRemoteHostRef.current) return;\n' +
    '    1793|       const currentState = stateRef.current;\n' +
    '    1794|       const sessionId = currentState.layout.layoutsByTabId?.[tabId]?.sessionIdsByLeafId[leafId];\n' +
    '       |                                                                      ^\n' +
    '    1795|       const activity = sessionId ? currentState.activityBySessionId?.[sessionId] : undefined;\n' +
    '    1796|       if (activity?.state === "working" || activity?.state === "waiting") {\n' +
    ' ❯ src/App.tsx:1817:9\n' +
    ' ❯ Object.closeMenuHandler src/App.tsx:2133:7\n' +
    ' ❯ src/App.remoteHostShortcuts.test.tsx:316:14\n' +
    ' ❯ node_modules/@testing-library/react/dist/act-compat.js:47:24\n' +
    ' ❯ act node_modules/react/cjs/react.development.js:2512:16\n' +
    ' ❯ Proxy.<anonymous> node_modules/@testing-library/react/dist/act-compat.js:46:25\n' +
    ' ❯ src/App.remoteHostShortcuts.test.tsx:315:5\n' +
    '\n' +
    '⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/2]⎯\n' +
    '\n' +
    '\n' +
    ' Test Files  2 failed (2)\n' +
    '      Tests  2 failed | 21 passed (23)\n' +
    '   Start at  17:31:56\n' +
    '   Duration  3.72s (transform 522ms, setup 235ms, collect 1.68s, tests 901ms, environment 653ms, prepare 71ms)\n' +
    '\n' +
    'error: script "test" exited with code 1'
}
  // Exited sessions have already reaped their daemon PTY and stream tasks; attaching would trigger SESSION_NOT_FOUND.
  const isExited = session ? session.backendSessionId === null || session.lifecycle === "exited" : false;
  const visible = interactive && !isExited;
  const targetSessionId = isExited
    ? null
    : session
      ? (session.backendSessionId ?? null)
      : (sessionId ?? null);
  const paneIdentity = session?.id ?? sessionId;
  const [presentation, setPresentation] = useState<{
    readonly paneIdentity: string | undefined;
    readonly backendSessionId: string;
  } | null>(null);
  const retainedPresentation = presentation?.paneIdentity === paneIdentity ? presentation : null;
  const surfaceSessionId = targetSessionId ?? (isExited && isMacShortcutPlatform() ? retainedPresentation?.backendSessionId ?? null : null);
  const bindingKey = targetSessionId
    ? `${targetSessionId}:${session?.daemonEpoch ?? ""}:${session?.remoteGeneration ?? 0}:${session?.remoteConnectionState ?? ""}`
    : null;
  const wheelPixelRemainderRef = useRef(0);
  useLayoutEffect(() => {
    wheelPixelRemainderRef.current = 0;
  }, [bindingKey, paneIdentity, visible]);
                  return (
                    <div
                      key={sysHost.id}
                      className="flex items-center justify-between rounded border border-border/40 bg-background/60 px-2.5 py-1.5 text-[12px]"
                    >
                      <div className="min-w-0 pr-2">
                        <div className="font-medium text-foreground truncate">{sysHost.label}</div>
                        <div className="font-mono text-[10px] text-muted-foreground truncate">
                          {formatSshTarget(sysHost)}:{sysHost.port ?? 22}
                        </div>
                      </div>

                      {alreadyAdded ? (
                        <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground shrink-0">
                          <Check className="size-3 text-emerald-500" /> Added
                        </span>
                      ) : (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={importingHostId === sysHost.id}
                          onClick={() => handleImportSingleSystemHost(sysHost)}
                          className="h-6 text-[11px] px-2 shrink-0"
                        >
                          {importingHostId === sysHost.id ? (

# TypeScript Programmer

Modern TypeScript. Type-strict, stack-first, async-correct.

## Philosophy

The compiler is your proof system. Make illegal states unrepresentable. Parse at boundaries. Every function has a contract; the type system enforces it.

## Hard rules

These are deliberate project choices. Violations are always wrong, not "style preferences".

### Tooling

| Category | Use | Never |
|---|---|---|
| Runtime | Bun (native TS, single binary) | ts-node, tsx |
| Package manager | `pnpm` | npm, yarn (unless workspace requires it) |
| Linter + formatter | Biome | ESLint, Prettier |
| Type checker | `tsc --noEmit` with strict config | skip type checking |
| Web framework | Hono | Express |
| Validation | Zod | joi, yup, class-validator |
| Testing | `bun test` or vitest | jest |
| ORM | Drizzle | TypeORM, Prisma (unless already in project) |

### The iron list

1. **Readonly by default** — all `type`/`interface` properties are `readonly`. Arrays are `readonly T[]`. Mutable only when mutation is the documented purpose.
2. **Branded types for distinct IDs** — `type UserId = Brand<string, "UserId">`. Never pass raw `string` where a branded type exists.
3. **Exhaustive switch** — every `switch` on a discriminated union ends with `default: assertNever(x)`. No fall-through.
4. **No any** — `any` is banned in annotations, returns, and parameters. Use `unknown` and narrow.
5. **No type assertions** — `as any`, `as unknown` banned. `as const` and `satisfies` are fine.
6. **No non-null assertion** — `x!` is banned. Use narrowing or optional chaining (`x?.y`).
7. **No @ts-ignore / @ts-expect-error** — fix the type.
8. **No enum** — use `as const` objects + literal union types.
9. **Zod at boundaries** — external input (API, user, file) → Zod schema. Internal → plain types.
10. **Typed errors** — Error subclasses with typed fields. No `throw new Error("bare string")` for domain errors. Use Result for expected failures within 1-2 call levels; throw for propagation across many layers.
11. **as const for constants** — module-level constant objects and arrays use `as const`.
12. **import type** — type-only imports use `import type`. Enforced by `verbatimModuleSyntax`.
13. **Named exports only** — no `export default`. Exception: framework requirement (Next.js pages, etc.).
14. **No empty catch, no catch-and-swallow** — every `catch` block must either (a) narrow the error with `instanceof` and handle each case, or (b) re-throw. Empty catch blocks and `catch (e) { console.error(e) }` without narrowing or re-throw are banned — they hide bugs. At top-level boundaries (CLI entry, HTTP handler), opt out with `// no-excuse-ok: catch`.

### Data modeling — which construct, when

| Situation | Use |
|---|---|
| User input, API request/response | Zod schema + `z.infer` |
| Internal value object | `type` with `readonly` properties |
| Function with multiple outcomes | Discriminated union (`kind` field) |
| Contract for implementations | `interface` |
| Fixed constants | `as const` + literal union |
| Distinct primitive (UserId vs OrderId) | Branded type |
| Key-value map | `Record<K, V>` or index signature |

**The one rule**: data crosses trust boundary → Zod. Everything else → plain `type` with `readonly`.

Load `data-modeling.md` for the full decision flowchart and comparison.

### When readonly does not apply

- **Framework state** (React `useState`, signals) — managed by framework.
- **Builder / accumulator** — object exists to be mutated (buffer, cache). Document why.
- **ORM mutations** — Drizzle insert/update objects.

### Why empty/unhandled catch is banned

In TypeScript, every `catch` receives `unknown`. The language gives you no type safety in catch blocks — you must earn it with `instanceof`. A bare `catch (e) { console.error(e) }` swallows `TypeError`, `RangeError`, and your domain errors identically. When a new error type appears, nothing warns you.

```typescript
// BANNED — empty catch
try { await fetchData() } catch {}
try { await fetchData() } catch (e) { /* will fix later */ }

// BANNED — catch-and-swallow (no narrowing, no rethrow)
try {
  const data = await api.get("/users")
} catch (e) {
  console.error("failed", e)
}

// GOOD — narrow with instanceof
try {
  const data = await api.get("/users")
} catch (e) {
  if (e instanceof HttpError) {
    logger.warn(`API ${e.status}: ${e.message}`)
    return fallback
  }
  throw e  // unknown errors propagate
}

// GOOD — top-level boundary (only place catch-all is acceptable)
async function main(): Promise<void> {  // no-excuse-ok: catch
  try {
    await run()
  } catch (e) {
    console.error("unhandled:", e)
    process.exit(1)
  }
}
```

### Libraries

| Domain | Library | Why |
|---|---|---|
| HTTP framework | Hono | Lightweight, multi-runtime, middleware, OpenAPI |
| Validation | Zod | Runtime validation + type inference |
| ORM | Drizzle | Type-safe SQL, no codegen |
| HTTP client | `ky` | Thin fetch wrapper (5KB); auto-throw on non-2xx, retry, timeout, hooks, prefixUrl. Browser + Node + Bun + Deno |
| HTTP client (perf) | `undici` (direct API) | When a Node backend needs connection pooling, HTTP/2, or pipelining |

> **HTTP client rule** - production code must not use bare `fetch()`. It has no retry, timeout, or error-handling policy and causes silent failures during incidents. Install **`ky`** by default, and use the **`undici`** direct API when a Node backend needs high-volume requests, connection pooling, HTTP/2, or pipelining. ~~`axios`~~ is forbidden after the supply-chain compromise (2026-03). `node-fetch` is unnecessary because Node 18+ includes built-in fetch.
| Testing | `bun test` / vitest | Fast, ESM-native |
| Logging | `pino` | Structured JSON, fast |
| CLI | `@clack/prompts` + `commander` | Interactive + parsing |

## tsconfig — the one true config

Scaffold a new project with all strict defaults pre-configured:

```bash
bun run ../../scripts/typescript/new-project.ts my-api
bun run ../../scripts/typescript/new-project.ts my-api --path ./projects
```

Creates: `package.json` (Hono + Zod + Biome), `tsconfig.json` (ultra-strict), `biome.json`, `src/index.ts`, `.gitignore`. Works on macOS, Linux, Windows.

For manual setup: `bunx tsc --init`, then load `tsconfig-strict.md` for the full strict config.

Key flags beyond `"strict": true`:

| Flag | What it catches |
|---|---|
| `noUncheckedIndexedAccess` | `arr[0]` is `T \| undefined`, forces check |
| `exactOptionalPropertyTypes` | `{ x?: string }` ≠ `{ x: string \| undefined }` |
| `verbatimModuleSyntax` | Forces `import type` for type-only imports |
| `noFallthroughCasesInSwitch` | Forgotten `break` / `return` |
| `noPropertyAccessFromIndexSignature` | `.key` on index sig → bracket notation |

## Reference loading

Load on demand — not all at once.

| Need | Load |
|---|---|
| Strict tsconfig + Biome config | `tsconfig-strict.md` |
| Type patterns (branded, as const, satisfies, narrowing, assertNever) | `type-patterns.md` |
| Data modeling (type vs interface vs Zod, readonly, parse-don't-validate) | `data-modeling.md` |
```

### GREEN raw tool receipt

```text
$ vitest run --maxWorkers=1 src/App.remote.test.tsx src/App.remoteHostShortcuts.test.tsx "--reporter=verbose"

[1m[46m RUN [49m[22m [36mv3.2.7 [39m[90m/Users/indo/code/project/orca-lite/ui[39m

 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mshows progress throughout registration, restore, and first SSH tab creation[32m 115[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mshows progress for manual first-tab creation and allows retry after failure[32m 30[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mreplaces registration failure with progress on explicit retry[32m 26[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mdoes not leak a late SSH failure into the selected local workspace[32m 25[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mfocuses the existing SSH terminal addressed by its backend identity[32m 33[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mloads a stored remote target without registering its path locally[32m 19[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mreturns from a local project to the requested existing SSH session[32m 34[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mopens the registered SSH project from remote without a preexisting terminal[32m 18[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mhandles the chooser's registered remote project through actual App registration[32m 41[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mroutes the chooser Settings CTA to SSH machines, not inbound Remote Access[32m 15[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22madopts the server's canonical host-qualified ID and path before new tabs and splits[32m 36[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mselects inactive remote roots by workspace identity even when a local root has the same path[32m 53[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mleaves a rejected host unavailable instead of spawning locally[32m 26[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mignores remote registration that resolves after switching to a local project[32m 22[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mrejects malformed stored targets without converting them to local: {"kind":"ssh"}[32m 6[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mrejects malformed stored targets without converting them to local: {"kind":"ssh","hostId":""}[32m 5[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mrejects malformed stored targets without converting them to local: null[32m 5[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mrejects malformed stored targets without converting them to local: {"kind":"unknown"}[32m 6[2mms[22m[39m
 [32m✓[39m src/App.remote.test.tsx[2m > [22mApp SSH project lifecycle[2m > [22mreattaches remote sessions after native startup without replacement and preserves their target and identity in the next save[32m 15[2mms[22m[39m
 [32m✓[39m src/App.remoteHostShortcuts.test.tsx[2m > [22mBlocker H3: local workspace shortcut suppression when remote host is active[2m > [22msuppresses local shortcut and menu mutations when activeRemoteHost is active[32m 107[2mms[22m[39m
 [32m✓[39m src/App.remoteHostShortcuts.test.tsx[2m > [22mBlocker H3: local workspace shortcut suppression when remote host is active[2m > [22msuppresses createWorktree modal and creation when activeRemoteHost is active[32m 39[2mms[22m[39m
 [32m✓[39m src/App.remoteHostShortcuts.test.tsx[2m > [22mBlocker H3: local workspace shortcut suppression when remote host is active[2m > [22msuppresses command palette menu and worktree selection mutations when activeRemoteHost is active[32m 54[2mms[22m[39m
 [32m✓[39m src/App.remoteHostShortcuts.test.tsx[2m > [22mBlocker H3: local workspace shortcut suppression when remote host is active[2m > [22mdismisses open command palette and suppresses worktree selection when switching to remote host[32m 28[2mms[22m[39m

[2m Test Files [22m [1m[32m2 passed[39m[22m[90m (2)[39m
[2m      Tests [22m [1m[32m23 passed[39m[22m[90m (23)[39m
[2m   Start at [22m 17:35:48
[2m   Duration [22m 2.76s[2m (transform 419ms, setup 164ms, collect 1.22s, tests 761ms, environment 412ms, prepare 54ms)[22m


```

### Final scoped diff

```diff
diff --git a/ui/src/App.remote.test.tsx b/ui/src/App.remote.test.tsx
index cee305ab..d633e0c3 100644
--- a/ui/src/App.remote.test.tsx
+++ b/ui/src/App.remote.test.tsx
@@ -7,6 +7,7 @@ const native = vi.hoisted(() => ({
   registerProject: vi.fn(), registerRemoteProject: vi.fn(), listWorktrees: vi.fn(),
   spawnTerminalDetailed: vi.fn(), spawnTerminal: vi.fn(), watchDagProject: vi.fn(),
   loadSession: vi.fn(), saveSession: vi.fn(), isTauriRuntime: vi.fn(),
+  getTerminalRemoteStatus: vi.fn<typeof import("./lib/tauri").getTerminalRemoteStatus>(),
   remoteSelection: null as null | ((payload: import("./lib/tauri").RemoteSelectionRequestedPayload) => void),
   closeGuard: null as null | (() => Promise<void>),
 }));
@@ -354,8 +355,10 @@ describe("App SSH project lifecycle", () => {
     } finally { error.mockRestore(); }
   });
 
-  it("restores remote sessions after native startup and preserves their target in the next save", async () => {
+  it("reattaches remote sessions after native startup without replacement and preserves their target and identity in the next save", async () => {
     native.isTauriRuntime.mockReturnValue(true);
+    const status = deferred<import("./lib/types").RemoteSessionStatusResponse>();
+    native.getTerminalRemoteStatus.mockReturnValue(status.promise);
     const saved: import("./lib/types").PersistedWorkspaceSession = {
       version: 2, timestamp: 1, activeWorkspaceId: remote.workspaceId,
       workspaces: { [remote.workspaceId]: {
@@ -372,7 +375,25 @@ describe("App SSH project lifecycle", () => {
     await mount();
     expect(native.spawnTerminal).not.toHaveBeenCalled();
     await act(async () => { registration.resolve(registered); await registration.promise; });
-    expect(native.spawnTerminal).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: remote.workspaceId, cwd: remote.repoRoot }));
+    expect(native.getTerminalRemoteStatus).toHaveBeenCalledWith("old-backend");
+    await act(async () => {
+      status.resolve({
+        type: "remoteSessionDetailsOk", legacyDirectSsh: false,
+        details: {
+          state: "connected", generation: 2, failure: null, replayGap: null,
+          attempts: 0, pid: 1234,
+          descriptor: {
+            backendSessionId: "old-backend",
+            target: { hostId: "build", ownerId: "owner", backendSessionId: "remote-process", epoch: "1" },
+            config: {}, clientRequestId: "original-request", remoteCursor: "0", cols: 80, rows: 24,
+          },
+        },
+      });
+      await status.promise;
+    });
+    expect(native.spawnTerminal).not.toHaveBeenCalled();
+    expect(native.spawnTerminalDetailed).not.toHaveBeenCalled();
+    expect(screen.getByTestId("active-tab")).toHaveTextContent("saved-tab");
     expect(native.registerProject).not.toHaveBeenCalled();
     expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY)!).find((project: RegisteredProject) => project.workspaceId === remote.workspaceId).target).toEqual(remote.target);
     expect(native.closeGuard).not.toBeNull();
@@ -380,5 +401,8 @@ describe("App SSH project lifecycle", () => {
     expect(native.saveSession).toHaveBeenCalled();
     const latest = native.saveSession.mock.calls.at(-1)![0];
     expect(latest.workspaces[remote.workspaceId].target).toEqual(remote.target);
+    expect(latest.workspaces[remote.workspaceId].terminalSessions["saved-session"]).toMatchObject({
+      localSessionId: "saved-session", backendSessionId: "old-backend", cwd: remote.repoRoot,
+    });
   });
 });
diff --git a/ui/src/App.remoteHostShortcuts.test.tsx b/ui/src/App.remoteHostShortcuts.test.tsx
index a86328c2..444d68d0 100644
--- a/ui/src/App.remoteHostShortcuts.test.tsx
+++ b/ui/src/App.remoteHostShortcuts.test.tsx
@@ -1,6 +1,7 @@
 import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
 import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
 import { remoteHostStore } from "./state/remoteHostStore";
+import type { WorkspaceState } from "./state/workspaceStore";
 
 const native = vi.hoisted(() => ({
   registerProject: vi.fn(),
@@ -130,12 +131,14 @@ const workspace = {
     workspaceId: "local",
     activeWorktreePath: "/local",
     layout: {
-      tabs: [{ id: "tab-1", kind: "terminal", label: "Terminal 1", title: "Terminal 1", sessionId: "session-1" }],
+      tabs: [{ id: "tab-1", kind: "terminal", label: "Terminal 1", sessionId: "session-1" }],
       activeTabId: "tab-1",
       layoutsByTabId: {
         "tab-1": {
           root: { type: "leaf", leafId: "leaf-1" },
           activeLeafId: "leaf-1",
+          expandedLeafId: null,
+          sessionIdsByLeafId: { "leaf-1": "session-1" },
         },
       },
     },
@@ -145,13 +148,15 @@ const workspace = {
         backendSessionId: "backend-1",
         cwd: "/local",
         worktreePath: "/local",
-        lifecycle: "alive",
+        workspaceId: "local",
+        worktree: null,
+        lifecycle: "running",
       },
     },
-    worktrees: [{ path: "/local", branch: "main" }],
+    worktrees: [{ path: "/local", branch: "main", head: "", bare: false, detached: false, locked: null, prunable: null }],
     unreadTabIds: {},
     unreadWorktreePaths: {},
-  },
+  } satisfies WorkspaceState,
 };
 
 const workspaceStoreModule = await import("./state/workspaceStore");
@@ -274,6 +279,7 @@ describe("Blocker H3: local workspace shortcut suppression when remote host is a
     // None of these menu actions should mutate the local workspace
     expect(workspace.openTab).not.toHaveBeenCalled();
     expect(workspace.closeTab).not.toHaveBeenCalled();
+    expect(workspace.closePane).not.toHaveBeenCalled();
     expect(workspace.splitPane).not.toHaveBeenCalled();
     expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
     expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
@@ -291,6 +297,7 @@ describe("Blocker H3: local workspace shortcut suppression when remote host is a
     // Still no local workspace mutations
     expect(workspace.openTab).not.toHaveBeenCalled();
     expect(workspace.closeTab).not.toHaveBeenCalled();
+    expect(workspace.closePane).not.toHaveBeenCalled();
     expect(workspace.splitPane).not.toHaveBeenCalled();
 
     // 3. Switch back to local machine
@@ -304,21 +311,17 @@ describe("Blocker H3: local workspace shortcut suppression when remote host is a
     });
 
     // Now menu actions and shortcuts work again for the local workspace!
-    act(() => {
+    await act(async () => {
       native.newTerminalMenuHandler?.();
     });
-
-    await waitFor(() => {
-      expect(workspace.openTab).toHaveBeenCalledOnce();
-    });
+    expect(workspace.openTab).toHaveBeenCalledOnce();
 
     act(() => {
       native.closeMenuHandler?.();
     });
 
-    await waitFor(() => {
-      expect(workspace.closeTab).toHaveBeenCalledWith("tab-1");
-    });
+    expect(workspace.closePane).toHaveBeenCalledExactlyOnceWith("tab-1", "leaf-1");
+    expect(workspace.closeTab).not.toHaveBeenCalled();
   });
 
   it("suppresses createWorktree modal and creation when activeRemoteHost is active", async () => {
```
