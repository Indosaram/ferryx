# Ferryx scope: isolated QA environment

Prepared 2026-09-05 on macOS arm64. This is executable preparation, not runtime
proof: no app, browser, SSH server, build, or test was launched for this task.
Only these docs were written. LSP is globally unreachable per task context;
direct source reads and CLI help are the fallback, not claimed LSP validation.
UI scope and exclusions are fixed in `docs/DESIGN.md`.

## Isolation contract and launch recipe

Run from `/Users/indo/code/project/orca-lite` in a dedicated shell when execution
is authorized. Use a fresh root each run; retain its path with evidence. The
following is a prepared command, not a command already executed:

```bash
set -eu
umask 077
QA_ROOT="$(mktemp -d /tmp/ferryx-qa.XXXXXX)"
export QA_ROOT
export FERRYX_RUNTIME_DIR="$QA_ROOT/runtime"
export FERRYX_SESSION_DIR="$QA_ROOT/session"
export FERRYX_DATA_DIR="$QA_ROOT/data"
export FERRYX_AGENT_STATE_SOCKET="$FERRYX_RUNTIME_DIR/agent-state.sock"
export CARGO_TARGET_DIR="$QA_ROOT/target"
export CARGO_BUILD_JOBS=8
mkdir -p "$FERRYX_RUNTIME_DIR" "$FERRYX_SESSION_DIR" "$FERRYX_DATA_DIR"
printf 'QA_ROOT=%s\n' "$QA_ROOT"
# Both must be vacant; if occupied, stop, do not kill the listener.
if lsof -nP -iTCP:5173 -sTCP:LISTEN; then exit 1; fi
if lsof -nP -iTCP:43821 -sTCP:LISTEN; then exit 1; fi
bun tauri dev --config '{"identifier":"com.ferryx.qa.st01a070e2"}'
```

- `daemon/server.rs`: runtime owns `daemon.sock`, `daemon.lock`, and
  `agent-state.sock`; session override owns `session_state.json`; data override
  isolates persistent daemon lock and remote data. Runtime directories must be
  owned by this user and private. Short `/tmp` paths avoid Unix socket length limits.
- `remote/state.rs`: remote auth/config storage honors `FERRYX_DATA_DIR`.
  `ipc/ssh.rs` instead stores `dev/ssh_hosts.json` under Tauri app data;
  `ipc/browser.rs` also consults app data. Hence the distinct Tauri identifier:
  expected macOS app data is `~/Library/Application Support/com.ferryx.qa.st01a070e2`.
  Verify actual paths at runtime; the environment overrides alone are NOT complete
  app/WebKit storage isolation. Reuse of this fixed QA identifier reuses that QA
  app data; select another QA-only identifier for a clean repeat, never erase normal data.
- Keep `HOME`, global Ghostty config, login SSH config, installed Ferryx, and launchd
  untouched. This does not isolate external agent credentials/config automatically.
  Use fixture-only projects and explicit test credentials; do not run ordinary workspaces.
- Root `package.json` maps `tauri` to `cargo tauri`. CLI help confirms inline
  `--config` merging and automatic macOS config. `tauri.macos.conf.json` selects
  `scripts/macos-dev-runner.sh`, which builds then assembles and executes
  `$CARGO_TARGET_DIR/debug/Ferryx.app/Contents/MacOS/ferryx`. Its bundle basename
  remains Ferryx, so computer-use must distinguish PID, not just app name.
- Cargo defaults include `native-terminal`; do not remove it for desktop QA.
  Eight Cargo jobs is a bounded pool on this 16-core machine. Cold native/FFI build
  prerequisites and successful ad-hoc bundle signing are not verified here.
- `beforeDevCommand` runs `bun scripts/dev-frontend.mjs`: first `ui build`
  (`tsc && vite build`), then Vite, then `FERRYX_FRONTEND_READY`. This writes
  `ui/dist` in the shared checkout; serialize with other build owners. Daemon
  readiness is `FERRYX_DAEMON_READY`, not an arbitrary sleep.
- Vite is hard-coded to `127.0.0.1:5173`, strict port, HMR client 5173; Tauri
  devUrl and CSP match. There is no supported frontend port environment override.
  Tauri `--port` only controls its built-in static server, not this Vite server.
- Gateway port is fixed **43821** outside tests (tests use 0); IPC ignores a
  requested alternate port. No supported gateway-port environment override exists.
  Fresh data defaults to Remote Access off. Enabling it binds `0.0.0.0`, not
  loopback-only, so use only an approved QA network. Do not enable a second
  listener beside another Ferryx remote gateway.

Read-only preflight found no listeners on 5173, 43821, or proposed SSH fixture
port 22222 at preparation time; this is not a reservation. Inherited agent socket
was `/tmp/rorca-501/agent-state.sock`, which must not receive QA fixture reports.
Do not stop or delete existing daemons. At teardown stop only recorded QA-owned
PIDs/listeners and browser session; keep evidence and do not run global cleanup.

## Real browser and desktop QA surfaces

Default browser QA is **Bun.WebView from the eval Bun kernel**, using native
macOS WebKit, not agent-browser when a kernel path is available. Read API reference:
`/Users/indo/.bun/install/global/node_modules/@code-yeongyu/senpi-codemode/src/skill/bun-1-4/references/runtime-apis.md`,
section "Bun.WebView" (links to `https://bun.com/docs/runtime/webview`). It documents
native macOS WebKit, `navigate`, string `evaluate`, Blob screenshots and
`await using` cleanup. No new dev dependency or standalone browser CLI is needed.

Literal eval code-cell example against the existing 5173 endpoint, after its
authorized isolated launch; set `qaRoot` to the recorded QA_ROOT path (the eval
kernel does not automatically inherit exports from another shell):

```ts
// qaRoot is the existing isolated QA_ROOT path supplied to this eval kernel.
{
  await using view = new Bun.WebView({ width: 390, height: 844 });
  await view.navigate("http://127.0.0.1:5173");
  const rendered = await view.evaluate(
    "JSON.stringify({url: location.href, title: document.title, text: document.body.innerText})"
  );
  console.log(rendered);
  await Bun.write(`${qaRoot}/mobile-web.png`, await view.screenshot());
} // Close: await using invokes async disposal here, including on failure.
```

The scoped `await using` is the reference-backed close example; do not leave a
WebView alive across unrelated QA work. Record actual rendered state and screenshot
path; navigation alone does not prove async feature readiness. Subscribe to the
relevant state/event before triggering behavior, without fixed sleeps. These
examples were documented, not executed; this child has no eval tool exposed.

Pair through the UI; do not save PINs/tokens in shared evidence. Port 43821 serves
built `ui/dist` and API together (navigate there for paired API integration);
5173 alone loads `RemoteApp` without a configured API proxy and is not desktop QA.
Browser automation cannot inspect native Tauri terminal surfaces or establish
physical-device push delivery. Use **Playwright from eval against real installed
Chrome**, with a QA-only profile, when Chrome behavior, traces, or actual Chrome
Web Push is required. Native WebKit WebView is not a substitute for real Chrome
push support. Desktop Chrome proof still does not replace the physical-phone
background receipt/tap-through gate below (real Chrome on Android; supported
installed Safari PWA on iOS). Chrome/Playwright availability and push permissions
remain unverified; do not install dependencies or reuse a personal profile to
paper over them. agent-browser is only a fallback when no kernel path exists.

Actual computer-use executable is `/usr/local/bin/orca` (no ORCA override set).
`orca skills get computer-use` was read; `orca status --json` returned reachable
runtime 1.4.184 and `orca computer capabilities --json` returned macOS screenshot,
accessibility, PID/window targeting and action support. Permission and Ferryx
capture success are not yet proven. Use the version-matched invocation:

```bash
orca computer list-apps --json
# Substitute the QA PID identified by its executable path, never production PID.
orca computer list-windows --app "pid:$QA_PID" --json
orca computer get-app-state --app "pid:$QA_PID" --json
# Once selected, keep --window-id from list-windows on observations/actions.
# Use fresh tree indexes for click/set-value, not inferred indexes.
```

The guide confirms `click --app ... --element-index ... --json`,
`set-value --app ... --element-index ... --value ... --json`, and
`press-key --app ... --key Escape --json`. Read `result.snapshot.treeText` and
the returned screenshot path. These are real command forms; QA PID/window/index
values are deliberately not invented. Do not use `orca open` or change permissions
during this preparation.

## SSH and mobile proof gates

SSH client `/usr/bin/ssh` and server `/usr/sbin/sshd` exist. No isolated server,
authorized QA user/key, or disposable remote repository has been provisioned or
connected. Therefore SSH/Run on end-to-end proof is **blocked**, not passed.
Reserve candidate `127.0.0.1:22222` only after a fresh port check. A future fixture
must own host key, authorized key, PID, logs, known_hosts and repo beneath its QA
root, bind loopback, disable password login, and avoid `~/.ssh/config` edits.
Use an explicit `ssh -F /dev/null -p 22222 -i "$QA_ROOT/ssh/client_key"
-o IdentitiesOnly=yes -o UserKnownHostsFile="$QA_ROOT/ssh/known_hosts" ...` client
against the provisioned fixture; username/host key are currently unknown.
Do not disable host-key verification or enable the global macOS SSH service.

Source risk: `ssh/exec.rs::probe_argv` currently uses `ConnectTimeout=2.5` and
does not append configured port/identity/jump options, whereas `interactive_argv`
does. A custom-port fixture cannot be declared supported by the probe on source
inspection alone. Validate actual launch/probe behavior during implementation;
do not fix it in this documentation task.

Mobile/public API source baseline: `remote/server.rs::create_remote_router`
exposes `/api/v1/health`, pairing, sessions, workspace state/selection/worktrees,
devices/revoke, terminal preferences, events and terminal WebSockets. Health
readiness can be observed with `curl -fsS http://127.0.0.1:43821/api/v1/health`;
it does not prove authenticated chat/approval or independent selection.
`ui/public/sw.js` has only install/activate/fetch cache handling, no push or
notification-click handler. Manifest/service-worker presence is not push support.

No real phone, trusted HTTPS origin, subscription/VAPID setup, or successful push
receipt was verified. Actual background push proof is **blocked** until these
exist. Required evidence: pair a real supported phone at trusted HTTPS, explicitly
grant notifications (install the PWA where required), background/lock it, trigger
an isolated daemon-owned waiting event with desktop UI closed, capture OS-level
notification receipt, and tap through to the exact conversation/approval context.
Also prove denial, revoked device, and stale/expired subscription behavior. A
desktop notification, browser viewport screenshot, page-open WebSocket badge,
fake push sender, or simulator-only result is not physical mobile push proof.
HTTPS provisioning must not introduce a relay/CloudVM feature into this scope.

## Implementation verification contract

Use `bun run --cwd ui test -- <related-file>`: the package script is Vitest with
`--maxWorkers=1`, not raw `bun test` for those React tests. Use
`cargo test --manifest-path src-tauri/Cargo.toml --lib <related-filter>` for Rust,
then `bun run --cwd ui build` and the affected desktop build when product changes
exist. Those commands were read, not run here. Async tests subscribe before the
trigger and await bounded events/state; never fixed sleeps. Capture actual UI
success and failure for each bounded feature, preserve terminal/browser drag and
settings behavior, and explicitly distinguish source checks, fixture integration,
desktop screenshots, mobile viewport tests, and physical-device evidence.
