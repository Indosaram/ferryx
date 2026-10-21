# Tailnet connection QA

The fixture entry at `tailscale-qa.html` renders the real `RemoteSection` with mocked Tauri IPC. It does not contact a daemon, SSH server, or account server.

Run `bun run --cwd ui dev --host 127.0.0.1 --port 5189 --strictPort`, then open `/tailscale-qa.html?scenario=ready&theme=dark` in an isolated browser.

Themes: `light` and `dark`. An unknown or missing `scenario` falls back to `ready`; the rendered scenario is exposed as `data-fixture-scenario` on `<html>`.

Tailnet scenarios (one Tailscale machine in a single state): `ready`, `connecting`, `auth`, `offline`, `provision`, and `discovery-error`.

Remote settings redesign scenarios:

| Scenario | Fixture |
| --- | --- |
| `machines` | Five paired machines: online, offline and checking (all machine-scoped, from `paired_host_reachability_list` with `supported: true`), one `needsMachineGrant` mirror credential and one revoked credential. Also a manual SSH machine, a provisioned Tailscale machine, and an unprovisioned Tailscale peer that belongs under Discovered. This machine has the relay gateway on with two authorized devices. |
| `empty` | No paired, SSH or Tailscale machines, gateway off, no devices. |
| `account` | `machines` plus a signed-in account. The account client's `fetch` is answered locally for `https://account.invalid` only: two account machines (one is the paired Studio Mac, one is not yet granted), enrollment-code issue, and logout. Other routes answer a typed 404. |
| `drawer` | `machines`, mounted with `initialContext={{ page: "machines", filter: "all", machine: { kind: "pairedDaemon", hostId, generation } }}` for the online Studio Mac, so the page opens with that machine selected (its details drawer once the drawer ships). |

The harness mounts `RemoteSection` only through its public props (`store`, `inventory`, `initialContext`, `accountOrigin`, `accountSessionToken`, `onOpenProject`, `onOpenSshProject`), and paired hosts reach the page through the real `createPairedHostInventory` and `bootstrapPairedHostReachability` code over mocked IPC. A project action sets `data-fixture-project-opened` and `data-fixture-project-target` on `<html>` instead of opening anything. Nothing polls; reachability is one list call plus the mocked change event.

## Remote settings layout

Settings > Remote renders these sections in this order. Check every scenario against it.

1. **Account**: one bar. Signed out, it shows the email sign-in form. Signed in, it shows the account server, an issue-enrollment-code action and Sign Out.
2. **My machines**: paired machines and SSH machines in one list ("Remote machines"), with search, one Add Machine button and Refresh. Each row shows the name, kind, status label, path chip, one primary action (Add Project, or Connect for an account machine that has no grant yet), Details, and a "More actions" menu. A disabled primary action shows its reason as text under the row. Details, Edit, Delete and agent integration open in a right-side drawer. A disabled SSH machine stays here so its menu can enable it again.
3. **Discovered**: Tailscale peers whose helper is not provisioned yet, with the discovery banner. Each row has Set up and Details. A peer moves to My machines once setup finishes.
4. **This machine**: access to this machine (gateway, relay, authorized devices).
5. **Advanced**: collapsed by default. It holds the account server URL and **Connection diagnostics**, which shows the native inventory status, the credential migration status and the remote access details.

## Status vocabulary

Paired and SSH machines use one vocabulary from `ui/src/lib/machineStatus.ts`. The row shows the label as text; the status element carries the code in `data-code` (`machine-status` on rows, `machine-drawer-status` in the drawer).

| Label | State | Codes |
| --- | --- | --- |
| Connected | `connected` | `READY`, `REACHABLE` |
| Offline | `offline` | `OFFLINE` |
| Checking… | `checking` | `CHECKING`, `UNCHECKED` (paired check in flight), `TESTING`, `CONNECTING` |
| Needs access | `needsAccess` | `MACHINE_GRANT_REQUIRED` (missing grant or revoked credential) |
| Setting up | `settingUp` | Tailscale peer that needs setup; a paired host not yet checked shows "Unchecked" (`UNCHECKED`); an untested SSH host shows "Not checked" (`UNCHECKED`) |
| Error | `error` | `NATIVE_CONTEXT_REQUIRED`, `ERROR`, a failed capability check code (for example `UNSUPPORTED_CAPABILITY`, `STALE_HOST_GENERATION`, `PAIRED_HOST_UNAVAILABLE`), or a Tailscale diagnostic code |
| Disabled | `disabled` | `DISABLED` |

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
