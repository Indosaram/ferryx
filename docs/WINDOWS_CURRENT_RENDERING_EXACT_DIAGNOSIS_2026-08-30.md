# Windows Current Rendering — Exact Code-Level Diagnosis

## Verdict

The current Windows rendering failure is caused by the custom programmatic Vite runner losing Tailwind configuration discovery.

`scripts/dev-frontend.mjs` starts Vite's Node API while the process working directory remains the repository root. Although it passes `root: <repo>/ui` and `configFile: <repo>/ui/vite.config.ts`, the PostCSS declaration loads `tailwindcss: {}` and Tailwind performs automatic `tailwind.config.js` discovery relative to the process working directory. It therefore does not load `ui/tailwind.config.js`.

Vite serves the HTML, TypeScript modules, React app, and source CSS successfully, but the generated stylesheet omits Tailwind utilities such as `.h-screen`, `.w-screen`, `.bg-background`, and `.flex`. React mounts correctly, yet the application root collapses to content height and loses all utility-driven layout and colors.

This is the current code-level defect. It is not a WebView navigation failure, React boot failure, stale 404, platform opacity issue, or full-window native terminal HWND occlusion.

## Exact code path

1. Tauri executes `bun scripts/dev-frontend.mjs` from the repository root.
2. `scripts/dev-frontend.mjs` calls `createServer()` with the `ui` root and config file but does not change `process.cwd()` to `ui`.
3. Vite loads `ui/postcss.config.js`:

   ```js
   export default {
     plugins: {
       tailwindcss: {},
       autoprefixer: {},
     },
   };
   ```

4. `tailwindcss: {}` relies on automatic Tailwind config discovery.
5. Discovery occurs from the repository-root process context instead of `ui/`, so `ui/tailwind.config.js` is not loaded.
6. Tailwind emits the runtime warning:

   ```text
   The `content` option in your Tailwind CSS configuration is missing or empty.
   ```

7. `@tailwind base`, `@tailwind components`, and `@tailwind utilities` are processed without the Ferryx content globs and theme extension.
8. React class names remain in the DOM, but their CSS rules do not exist.

## Direct interactive WebView evidence

Evidence was collected from the user's real Session 1 WebView and written to:

```text
.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/current-interactive-webview.jsonl
```

Observed stages:

```text
main            isTauriApp=true
app-imported
render-called
dom-1s
dom-5s
```

This proves that `ui/index.html`, `ui/src/main.tsx`, the dynamic `App` import, and `ReactDOM.render` all complete.

At `dom-5s`:

```text
viewport       1280 x 850
#root          1280 x 850, one React child
App root       1280 x 406
main           y=213, height=193
native pane    y=359, height=47
body bg        rgb(35, 38, 45)
App root bg    transparent
main bg        transparent
```

The root text contained real Ferryx content (`Strawberry`, project/path data, branch text, DnD accessibility text). Therefore React is present and populated.

The App root has the source classes `h-screen w-screen bg-background`, but it measures only 406 pixels high inside an 850-pixel viewport and has a transparent computed background. This directly proves that the utility classes are absent from the active stylesheet.

The native terminal pane occupies only `1280 x 47` at the bottom of the rendered App content. It does not cover the full client and is not the cause of the missing layout.

## Served CSS evidence

From the current Windows programmatic runner on port 5173:

```text
stylesheet length  16,190 bytes
.h-screen          absent
.w-screen          absent
.bg-background     absent
```

The same source tree served by standard Vite launched with `ui/` as the working directory on port 5174:

```text
stylesheet length  71,473 bytes
.h-screen          present
.w-screen          present
.bg-background     present
```

This A/B comparison changes only the Vite process working-directory/config-discovery context. It confirms the runner/config interaction as the cause.

## Rejected hypotheses

| Hypothesis | Rejection evidence |
| --- | --- |
| Wrong or unreachable Vite URL | `/`, `/src/main.tsx`, `/src/App.tsx`, and `/@vite/client` are served successfully; the Ferryx WebView2 renderer has an established TCP connection to 5173 |
| React bootstrap failure | Interactive trace records `main`, `app-imported`, `render-called`, populated `#root`, and Ferryx text |
| Missing Tauri injection | `isTauriApp=true` in the actual WebView |
| Global root transparency remains | `body` computed background is opaque `rgb(35, 38, 45)` and the macOS-only selector fix is active |
| Native terminal HWND covers the client | DOM native pane bounds are only `1280 x 47`, not the `1280 x 850` viewport |
| Source mismatch | Hashes for runner, Vite config, main, CSS, Tauri configs, and Rust app code match the verified branch; only `index.html` EOF newline differs |

## Minimal fix seam

The fix must make Tailwind config discovery deterministic instead of depending on the caller's process working directory. The smallest valid options are:

1. Change `process.cwd()` to the absolute `ui` directory before `createServer()`; or
2. Configure the Tailwind PostCSS plugin with the absolute `ui/tailwind.config.js` path.

The preferred minimal fix is to establish the UI process context in `scripts/dev-frontend.mjs`, because the standard `bun run --cwd ui dev` A/B control already proves that this context produces the correct CSS and it also aligns every cwd-sensitive frontend tool with normal UI execution.

Before implementation, the regression test must start the actual runner and assert that the served `/src/index.css` contains `.h-screen`, `.w-screen`, and `.bg-background`. A string-only runner test cannot catch this defect.

## Instrumentation cleanup

Temporary interactive tracing was removed from `ui/src/main.tsx` and `ui/vite.config.ts` after evidence collection. The JSONL evidence is retained. No product fix was applied as part of this diagnosis.
