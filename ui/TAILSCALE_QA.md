# Tailnet connection QA

The fixture entry at `tailscale-qa.html` renders the real `RemoteSection` with mocked Tauri IPC. It does not contact a daemon, SSH server, or account server.

Run `bun run --cwd ui dev --host 127.0.0.1 --port 5189 --strictPort`, then open `/tailscale-qa.html?scenario=ready&theme=dark` in an isolated browser.

Supported scenarios: `ready`, `connecting`, `auth`, `offline`, `provision`, and `discovery-error`. Themes: `light` and `dark`.

## Executed frontend checks

- Focused Vitest run: 4 files, 56 tests passed; exit code 0. Receipt: `/tmp/ferryx-tailnet-ui-tests-final.log`.
- Frontend production build (`tsc && vite build`): exit code 0. Receipt: `/tmp/ferryx-tailnet-ui-build-final.log`.
- Ready machines enable the project action without opening a project automatically. An explicit click in the actual fixture component delivered the project callback. A reachable machine whose helper failed remains unavailable.
- Actual component rendering in isolated Chrome: readiness, connecting, authentication failure, offline, helper provisioning failure, and discovery failure displayed the expected state. No horizontal overflow at 1440px.
- Authentication failure rendered at 390px and 1440px in both light and dark themes. The machine name, concise status, error text, retry control, and details control remained visible without overlap. Measured document width equaled viewport width in all four cases.

The initial render exposed duplicate long error text colliding with controls. The final layout places the concise status beside the name and the exact diagnostic below the header.

## Backend and review evidence

- Focused backend run (`cargo test --manifest-path src-tauri/Cargo.toml --lib tailscale -- --test-threads=4`): 11 passed, 0 failed. Receipt: `/tmp/ferryx-tailnet-backend-tests-final.log`. Cases cover parser behavior, stopped local backend, saved SSH identity/credential preservation, stable node address changes, deletion tombstones, production orchestration with disabled/offline/mobile peers, and authentication/provisioning result classification.
- Static independent review: APPROVE, explicitly scoped to code and contract inspection, with no execution by the reviewer. Artifact: `.omo/evidence/st_01a12927-gate-review.md`.
- Backend library check: `CARGO_TARGET_DIR=/tmp/ferryx-tailnet-check-target cargo check --manifest-path src-tauri/Cargo.toml --lib` passed with exit code 0. Receipt: `/tmp/ferryx-tailnet-backend-check-isolated.log`. The original shared-target check timed out waiting for another build's directory lock; the isolated check finished in 12m 19s. Existing unrelated warnings were left unchanged.

These fixture and injected-seam checks do not prove real Tailnet hosts were provisioned or that the installed desktop app contains this change. No installed app, live desktop, or production daemon was changed. The local PATH has no `tailscale` executable; live multi-machine Tailnet operation and target-specific helper artifacts remain unverified.
