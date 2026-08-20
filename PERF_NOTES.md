# ORCA Lite Frontend Performance Notes

Measured on 2026-08-21 from the production Vite build and the frontend lifecycle regression tests in this repository.

## Bundle split evidence

Before the Phase 8 terminal renderer split, `bun run --cwd ui build` produced one main JavaScript bundle and Vite emitted the `Some chunks are larger than 500 kB after minification` warning:

| Asset | Minified | Gzip |
| --- | ---: | ---: |
| `index-C8ZgLcLn.js` | 662.97 kB | 182.68 kB |
| `index-CoguJGw8.css` | 19.76 kB | 5.33 kB |

After lazy-loading xterm, FitAddon, xterm CSS, and the WebGL addon from the terminal view path, the same production build produced:

| Asset | Minified | Gzip | Load path |
| --- | ---: | ---: | --- |
| `index-DL1Qy0eE.js` | 217.94 kB | 68.17 kB | eager application shell |
| `xterm-BqvuqXEL.js` | 332.63 kB | 84.18 kB | lazy terminal runtime |
| `addon-webgl-BrQ0bpT6.js` | 111.96 kB | 30.52 kB | lazy renderer enhancement |
| `addon-fit-YJmn1quW.js` | 1.60 kB | 0.71 kB | lazy terminal runtime |
| `index-Cfx2PEwT.css` | 14.01 kB | 3.54 kB | eager application CSS |
| `xterm-9CEnUXvW.css` | 5.75 kB | 1.98 kB | lazy terminal CSS |

The eager JavaScript entry therefore fell from 662.97 kB to 217.94 kB, a reduction of 445.03 kB (about 67%). Its gzip size fell from 182.68 kB to 68.17 kB. No `>500 kB` chunk warning was emitted by the optimized build.

The split is behavioral rather than a manual chunk-size exception: `loadTerminalAssets()` dynamically imports xterm, FitAddon, and xterm CSS only when `TerminalPane` initializes, while `attachWebglRenderer()` performs a separate dynamic import of the WebGL addon. The regression test verifies that importing the renderer helper itself loads none of those runtime assets, that terminal initialization loads xterm/Fit/CSS, and that WebGL remains unloaded until renderer attachment.

## WebGL lifecycle evidence

`globalThis.__ORCA_WEBGL_LIFECYCLE__` publishes observable counters with the following fields: `created`, `active`, `disposed`, `contextLosses`, `loadFailures`, and `canvasFallbacks`.

The automated lifecycle test exercises these transitions:

| Scenario | Observable result |
| --- | --- |
| Successful WebGL attach | `created=1`, `active=1`, `disposed=0`, `contextLosses=0`, `canvasFallbacks=0` |
| WebGL context loss | `active` changes `1 -> 0`, `disposed` changes `0 -> 1`, `contextLosses=1`, `canvasFallbacks=1` |
| Component cleanup after context loss | WebGL disposer remains at one call; no double-disposal |
| WebGL attachment failure | `created=1`, `active=0`, `disposed=1`, `loadFailures=1`, `canvasFallbacks=1` |

These tests use a deterministic mocked WebGL addon so the counters and disposal transitions are reproducible in the headless test runner. They verify lifecycle accounting and Canvas fallback behavior; they are not a measurement of native GPU memory.

Terminal process ownership remains outside the renderer. `TerminalPane` allocates and disposes xterm/Fit/WebGL view resources only; PTY spawn/close ownership remains in the workspace session store. A split renders one xterm view per visible primary/secondary terminal session, while changing renderer availability does not create or destroy the backend PTY session.

## Verification output

The optimized revision was verified with:

- `bun --cwd ui test`: 8 test files, 25 tests passed.
- `bun run --cwd ui build`: TypeScript and Vite production build exited 0 with the optimized chunks listed above and no chunk-size warning.
- `bun run --cwd ui test -- src/lib/terminalRenderer.test.ts`: 3 renderer lifecycle/lazy-loading tests passed.

The installed Bun CLI treats the task-request spelling `bun --cwd ui run build` as a help invocation and exits 0 without executing the package script. For an actual production build in this environment, the equivalent functional command is `bun run --cwd ui build`; both invocations are captured in the task verification log.

Idle RSS, OS process count, and native GPU allocation were not measured by this headless unit/build harness, so no values for those metrics are claimed here.
