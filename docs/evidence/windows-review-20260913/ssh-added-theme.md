# C002: SSH Added icon theme correction

## Delivered change

`ui/src/components/settings/SshSection.tsx:623` changes only the Added Check icon utility from `text-emerald-500` to the existing `text-status-success`. Copy, layout classes, inventory matching, and behavior are unchanged. The file was clean on initial and immediately pre-edit status/diff checks. Foreign working-tree changes were left untouched. No commit, branch, worktree, dependency installation, or other product/test edit was made.

The settings shell renders `SshSection` for the SSH section. Its real `useSshHosts` hook loads configured inventory through `cmd_ssh_list_hosts`; system discovery loads through `cmd_ssh_read_system_config`. Expanding Show Hosts renders Added when a discovered host matches configured label or hostname/port/username. The defect was the fixed palette color at that actual branch, not the inventory logic. `ui/DESIGN.md` requires semantic utilities; `tailwind.config.js` maps success to `--status-success-rgb`. `index.css` supplies dark success `#86efac`; `settings-runtime.css` overrides light success to `#15803d`.

## Verification

- LSP on changed `SshSection.tsx`: **No diagnostics found**.
- `git diff --check -- ui/src/components/settings/SshSection.tsx`: clean.
- Exact requested command, executed once, exit 0:

  ```sh
  CI=1 bun run --cwd ui test src/appearanceThemeContract.test.ts src/components/settings/SshSection.test.tsx --reporter=verbose
  ```

  **2 test files passed; 33 tests passed; 0 failed** (3.19 seconds). Unchanged assertions. Full output: [ssh-added-tests.log](ssh-added-tests.log).
- One headless rendering harness execution: **4/4 viewport/theme cases passed**, using Bun 1.4.0 `Bun.WebView` Chrome backend and existing Vite/React/Tailwind dependencies. Actual production `SshSection`, its hooks and UI primitives, `index.css`, and `settings-runtime.css` were rendered. Only the Tauri runtime/IPC boundary was mocked; a known `review-server` at `dev@review.example.test:22` was returned in both inventory and discovery, with distinct IDs. Each case recorded exactly the two read commands, no mutation or connection commands.
- The harness expanded the real Show Hosts button and observed the resulting DOM via MutationObserver subscribed before the click, with a bounded failure timeout (no sleeps or polling). It awaited font readiness, measured the real SVG computed styles and bounds, then captured browser screenshots.

| Viewport | Theme | Actual SVG color and stroke | Existing token | Icon bounds x/y/w/h |
|---|---|---|---|---|
| 1280 x 844 | light | `rgb(21, 128, 61)` | `#15803d` | 574.359375 / 249 / 12 / 12 |
| 1280 x 844 | dark | `rgb(134, 239, 172)` | `#86efac` | 574.359375 / 249 / 12 / 12 |
| 390 x 844 | light | `rgb(21, 128, 61)` | `#15803d` | 295.359375 / 285 / 12 / 12 |
| 390 x 844 | dark | `rgb(134, 239, 172)` | `#86efac` | 295.359375 / 285 / 12 / 12 |

All four actual SVGs had `lucide lucide-check size-3 text-status-success`, parent label Added, and viewport-contained 12px bounds. Runtime details: [results.json](ssh-added-render/results.json), [render log](ssh-added-render.log).

## Real screenshots and SHA-256

PNG signatures and exact dimensions were independently checked with `file`.

- [Desktop light](ssh-added-render/1280-light.png): `ced5baad535acc5c305391358a049d211a59d51c0a6c82186dcda5b43a852117`
- [Desktop dark](ssh-added-render/1280-dark.png): `4536357342d849887011eb5fafbdc41227b7d78a9c067b1208b6126d9986f375`
- [Mobile light](ssh-added-render/390-light.png): `fb6dcc9314daacaf30d7c77b74f149b33b36f376c9cbd9a67f9704c0e412f21f`
- [Mobile dark](ssh-added-render/390-dark.png): `d8f53f67455dea504ad7bbb218c46ee2dfff68e957acb08ac7d1d07b28d4a20a`

**Visual inspection limitation:** all four screenshots were submitted to the image read tool, which returned `Current model does not support images. The image will be omitted from this request.` Consequently, actual rendering, computed color/stroke, state, bounds, and PNG generation are verified, but human-like visual review of screenshot pixels is externally blocked in this child session. The parent/image-capable reviewer can inspect the retained images. Native Windows/WebView2 acceptance, native mobile behavior, SSH backend connectivity, and the complete settings dialog are not claimed.

## Source snapshots (Git blob hashes)

| File | Before | After |
|---|---|---|
| `SshSection.tsx` | `d8d3407589b4eddc0a163252a7795375acba53ac` | `98110db8fe15520d8e081cd8009f078416dc8b15` |
| `appearanceThemeContract.test.ts` | `ae140e85e6ce5202f062aece5c74a9fa089325ab` | unchanged |
| `SshSection.test.tsx` | `56252db6eb97669dedcf273d1485e26f32ca6bd9` | unchanged |

The parent's reported pre-fix full-suite failure is task context, not a full-suite run performed by this child. No broader build or suite was run for this one-token change.

## Resource cleanup

Four isolated headless WebViews were closed in `finally`; each close returned. The owned loopback Vite server used port 5173, and `Vite.close` resolved in `finally`; subsequent `lsof` found no TCP listener on that port. The exclusively owned `ssh-added-harness` directory and its three temporary files were removed after execution. Pre-removal harness SHA-256 hashes are retained in [ssh-added-harness-hashes.txt](ssh-added-harness-hashes.txt); removal and port check receipt is [ssh-added-cleanup.log](ssh-added-cleanup.log). No desktop applications, OS dialogs, user daemon, or user browser profile were operated. Changes remain uncommitted in the shared working tree and are subject to concurrent edits.
