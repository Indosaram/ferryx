# P29 implementation: Windows tooling test path portability

Owner: st_01a09a0d. Date: 2026-09-13. Status: scoped implementation and Darwin verification complete; native Windows acceptance remains pending with runtime owner st_01a099f8. This is not aggregate objective completion.

## Scope and registration

Only `scripts/macos-dev-runner.test.mjs` and `site/src/seo.test.ts` changed, plus this packet's evidence. Both source files were clean at intake and their diffs were checked before edits. Foreign dirty files were not modified. Root AGENTS.md and programming, debugging, and ulw-loop skill instructions were read. Packet criteria came from repair-packets.md, gap-packet-addendum.md P29, dag-remaining-register.md P29, and closure-tooling.md TOOLING-GAP-03/04.

Before repair, official `executeAgentToolkit` was called with repo-bound resolveCwd and parent resolveSessionId `01a0983f-c995-753d-afa9-593f6d118788`, operation steer/kind revise_criterion. C002's complete current scenario was preserved and P29 commands/binary conditions appended. `p29/registration.json` contains the accepted response (`ok: true`). No goal status was marked complete.

## Changes and mechanism

### Signing fixture

The test supplied native filesystem paths to a POSIX shell: stripping only `/debug` left Windows `\\debug` intact, then the runner appended a second debug component. Native drive-letter PATH assembly could also defeat tool interception. POSIX executable mode bits did not establish Windows execution semantics.

- Replaced slash-specific target stripping with the selected path implementation's dirname; added POSIX and win32 single-debug assertions through that same fixture helper.
- Start provisioned Bash in the owned fixture cwd, with no profile/rc and empty child-only BASH_ENV/ENV. Bash derives its own absolute target path from `$PWD` and a POSIX relative target. No native PATH string construction or global environment edits.
- Bash functions intercept uname/cargo/codesign and invoke explicitly named owned tool files. Missing fixture files fail instead of falling through to a real cargo or signer. The production shell is copied byte-for-byte and sourced in this controlled shell.
- Retained success=0/failure=7 and fixture-launch assertions, strengthened exact signing argv and tool-call receipt assertions. Fixture paths deliberately contain spaces. The only launched executable is a shell sentinel, not Ferryx.
- Removed the host executable-bit assertion: actual Bash execution now verifies the relevant contract without a platform skip. Added child timeout and finally kill-if-live/await-exit before temporary root removal.

### SEO fixture

The real-built HTML walker used the native path suffix as a route key. Windows backslashes therefore failed the slash-key required-page/content assertions even with a successful build.

- The walker now uses a shared relative-path-to-URL-key helper (`relative`, split by filesystem separator, join by URL slash).
- POSIX/win32 projection assertions consume actual fresh built-page routes through that helper.
- An owned copy of real built HTML first passes the same walker, then removing `docs/introduction/index.html` must throw from required-page containment. Only this temporary copy is removed, not a production/source page.
- Kept all original required-page, metadata, asset, internal-link and root-origin assertions. The suite still performs both real Astro builds and propagates build failure.

## Exact verification

Both RED commands executed on Darwin arm64 with Bun 1.4.0. The new assertions were staged with original separator behavior before repair. These are deterministic path-projection REDs, not native Windows execution claims.

| Command | RED | GREEN |
| --- | --- | --- |
| `bun test scripts/macos-dev-runner.test.mjs` | exit 1; 3 pass/1 fail; expected `src-tauri\\target\\debug`, received `src-tauri\\target\\debug\\debug` | exit 0; 4 pass/0 fail; 10 assertions; fixture subprocess exits 0 and 7 |
| `bun test --cwd site src/seo.test.ts` | exit 1; 22 pass/1 fail; expected `/index.html`, received `\\index.html`; both real builds otherwise passed | exit 0; 24 pass/0 fail; 1020 assertions; both fresh builds and removed-required-page rejection pass |

Logs: `p29/signing-red.log`, `p29/signing-green.log`, `p29/seo-red.log`, `p29/seo-green.log`. Each command was run once in each phase, not retried to obtain green. Same path assertions changed from RED to GREEN. Signing acceptance assertions were strengthened, not relaxed.

LSP diagnostics on both changed source files reported `No diagnostics found` before GREEN. `git diff --check` passed. `p29/verification.log` records host/tool provenance, HEAD, SHA256 hashes, diff-check exit, and checks that each GREEN temporary root is absent. `p29/scoped.patch` captures the scoped source changes. Actual affected runnable surfaces were the copied production debug shell with fake tools and real Astro build/test commands; no desktop app, release runner, real signer, real Cargo build, daemon, install, branch/worktree operation, commit, push, or Windows mutation was performed.

## Cleanup and ownership

The signing children exited before their roots were removed; logs record exit and root. SEO's owned removed-page copy was removed in finally. Independent post-run existence checks verified all three GREEN roots absent. The suite's pre-existing output locations `site/dist` and `site/node_modules/.cache/seo-root-dist` contain freshly rebuilt artifacts and were retained rather than deleting shared build outputs. No long-lived process was created. Changes remain uncommitted in the shared tree and are subject to concurrent drift; the recorded hashes bind this evidence to the tested bytes.

## Exact native handoff / remaining prerequisite

Parent must relay to runtime owner **st_01a099f8** (no child task-send tool is available):

1. In the owner's approved isolated native Windows repository fixture, with existing native Bun, Bash/POSIX tools, and site build dependencies, apply the two scoped files from the recorded patch. Do not install dependencies or launch Ferryx/release tooling for this packet. Missing Bash/build prerequisites are BLOCKED, not RED or skipped tests.
2. Execute `bun test scripts/macos-dev-runner.test.mjs` and `bun test --cwd site src/seo.test.ts`. Require respectively 4 and 24 discovered passing tests, exits 0, exact fake signing argv/tool receipts, signing children reaped before roots removed, and fresh base/root-origin builds plus removed-page rejection. Record native tool paths/versions and source hashes; verify logged temporary roots absent.
3. Native original-source RED remains outstanding. In that isolated fixture only, retain the new assertions and controlled fake-tool boundary, revert `targetRootFor` to `return target.replace(/\/debug$/, "");`, and routeKey to `return full.slice(root.length) || "/";`; run the same commands and retain intended duplicate-debug / backslash-route failure output. Restore the repaired helper bodies and rerun the identical commands. Do not count missing dependencies or zero-test execution as RED. The old inline expressions and repaired helpers are captured in `p29/scoped.patch`; the RED logs preserve the resulting assertion failures.

Native completion is narrowly blocked on that owner-controlled execution, not additional code approval or a repeat audit. Darwin path projections and real fixtures are verified, but do not prove Windows Bash namespace/tool behavior. No Cargo slot is needed for this packet.
