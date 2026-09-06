# Project chooser and App integration verification

- Task: `st_01a0779c`
- Date: 2026-09-06 (Vitest workstation-local start times below)
- Scope: C1-C3 in section 9 of `docs/PROJECT_LOCATION_PLAN_2026-09-06.md`, limited to UI behavior, transport wiring, and App/state routing.
- **Verdict: NOT an unconditional UI acceptance pass.** Both producer-focused commands pass (222 tests), and the actual chooser/shared-hook/adapter integration works under a transport fixture. A newly introduced Local picker retry defect was reproduced. The broader App suite still fails its reported updater test. Native desktop and backend acceptance are separate gates, not inferred from these results.
- This verifier wrote only this report. No production/test fixes, commits, full build, Rust tests, native picker, live SSH session, or browser screenshot were performed.

## 1. Current blockers and evidence limits

### B1 - Local picker retry becomes permanently pending after a picker error

**Reproduced against the actual `AddProjectDialog`, not a reimplementation.**

Sequence: choose Local -> native picker promise rejects -> manual fallback appears -> click Back -> choose Local again.

Observed output from the supplemental execution in section 5:

```text
Error: picker denied
PICKER_ERROR_MANUAL=true OPEN_CALLS=1
PICKER_RETRY_OPEN_CALLS=1 STUCK_PENDING=true
```

`ProjectDialogs.tsx:157-159` sets `local-pending`, then returns if `pickerOpenedRef.current` is already true. The first attempt sets the ref true; the rejection path switches to `local-manual` without clearing it. The newly added manual Back navigation permits reentering Local, but the second entry never invokes `open()` and cannot leave the pending state except by dismissal. This contradicts the broad claim that Local error/navigation behavior is fully preserved. Existing tests exercise initial rejection and manual submission, but not rejection -> Back -> retry.

Required producer correction: make a new explicit Local attempt possible after the previous picker has rejected, with deterministic behavioral coverage. No fix was made here. The lead has assigned RED-first reproduction/fix to the chooser owner. B1 describes the hashed snapshot verified below; any subsequent producer correction is not verified by this report. Verification is bounded and complete at these two seams: picker retry and the actual chooser/shared-hook/adapter chain. The supplemental harness ran entirely through stdin; no QA-only harness file exists to remove, and its servers/DOM were closed.

### B2 - Broader App regression command is not green

Independently reproduced:

```text
FAIL src/App.test.tsx > App project workspace flow > checks for a signed update when the native app starts
AssertionError: expected "spy" to be called once, but got 0 times
src/App.test.tsx:662:56
await waitFor(() => expect(updater.checkForUpdate).toHaveBeenCalledOnce());

Test Files  1 failed | 2 passed (3)
     Tests  1 failed | 104 passed (105)
error: script "test" exited with code 1
```

The runtime producer reports this as pre-existing and provides `runtime-updater-baseline.log`. That artifact was read: its HEAD-baseline-named test has the same failure. The relevant tracked App/test changes do not alter the updater seam. **Historical baseline classification is supported by the supplied capture, not independently re-created by this verifier.** The current failure itself is independently verified and must remain visible; it was not skipped, deleted, suppressed, or retried to get green.

### Evidence limits, not additional demonstrated product failures

- The chooser RED evidence is embedded in `chooser.md`; there is no standalone chooser RED log/exit artifact in the inspected evidence directory. It shows the right behavioral seam (an actual eager picker call), but pre-production chronology cannot be authenticated from the prose capture alone. Runtime RED has a separate executable-looking capture, `runtime-red.log`, with three actual local-registration failures and exit 1. Neither historical RED was replayed by altering production files.
- `runtime.md` says zero diagnostics on all changed files. Fresh diagnostics instead show two TypeScript **hints** in App (section 4), with no errors/warnings. Do not restate the stronger zero-diagnostics claim.
- Backend DTO/command wiring was source-inspected only. `qa-environment.md` now contains a lead-owned real-sshd/backend receipt; it does not transfer backend acceptance to this UI verifier and does not prove native desktop behavior.
- Native desktop acceptance remains open. The inspected environment receipt records unavailable Accessibility/Screen Recording and unsuccessful desktop/browser automation attempts. This verifier did not recheck permissions or perform desktop automation. DOM dialog roles do not prove native WGPU occlusion or real OS picker behavior.

## 2. C1-C3 claim matrix

| Criterion / claim | Result and actual evidence |
| --- | --- |
| C1: initial chooser, zero eager native picker | **PASS at component/transport surface.** `step` starts at `choose-location`; `open()` is only in `handleChooseLocal`. Producer tests pass under Tauri fixture, including StrictMode. Supplemental actual-component execution prints `INITIAL_PICKER_CALLS=0`. |
| C1: explicit Local native-directory options | **PASS at mocked native boundary.** Passing test asserts `{ directory: true, multiple: false, title: "Add Project" }` after Local click, not on mount. Actual OS invocation remains pending. |
| C1: cancellation, local registration, manual fallback | **PASS for covered paths; B1 blocks blanket preservation claim.** Tests cover null picker result closing, selected-path confirmation, derived/deduplicated ID, local registration error, non-Tauri manual registration, and picker-error manual registration. Pending dialog cancellation is covered. Null-result test does not separately assert `onRegistered` absence, but source never calls it on this branch. |
| C2: real remote request, not a fabricated sidebar project | **PASS for frontend wiring.** Real `remoteProject.ts` calls typed `invoke<RegisteredRemoteProject>("cmd_project_register_remote", { request })`; non-Tauri rejects. Adapter tests assert wire payload, canonical response, structured rejection, and target conversion. Supplemental chooser + real adapter execution reaches the core transport with the actual request. No local call or picker occurs. |
| C2: request/response agrees with Rust | **PASS by source comparison only.** Read `src-tauri/src/ipc/project_remote.rs`: camelCase DTO request (`workspaceId`, `hostId`, `repoPath`) and response (`workspaceId`, `repoRoot`, nullable `gitRoot`, `hostId`, `hostLabel`) match the adapter. `src-tauri/src/lib.rs:845` registers the command. Server returns canonical host/path identity, not the advisory slug. SSH probing/persistence/PTY correctness is outside this lane. |
| C2: same SSH inventory as Settings | **PASS.** Chooser and `SshSection` import the same `lib/sshHosts` module and real `useSshHosts`. Settings calls its update/import/delete wrappers, which publish backend-returned inventory through the shared cache/listeners. Passing hook tests exercise simultaneous consumers and stale read protection; Settings tests exercise the actual section/library. Supplemental execution mounts the actual chooser with the actual hook and observes update/disable/delete without remounting. No second host store was added. |
| C2: empty/all-disabled host states and Settings CTA | **PASS at UI surface.** Chooser tests cover empty and all-disabled guidance, disabled submit, loading, and CTA callback. App's actual chooser test proves callback routes to `ssh` and dismisses Add Project (Settings body is mocked there). Separate real Settings shell tests prove `initialSection="ssh"` mounts SSH Machines and nav selection works. Source includes `ssh` in App validation, Settings validation, and `SectionId`, not the inbound `remote` section. |
| C2: stale/deleted/disabled hosts and no silent retarget | **PASS for tested cases and inspected guards.** At submit, real chooser awaits `refreshHosts`, rejects refresh failure, checks exact selected ID and `disabled`, then invokes remote registration. Tests cover deleted authoritative inventory, failed refresh, and selection removal while another host remains. Supplemental real-hook mutations verify disable/delete empty state and disabled submit; reenabling does not silently reselect (`REENABLE_SELECTION=""`). Producer tests named “removed or disabled” only use deletion in those individual cases; the supplemental disable execution closes that particular coverage gap. Changes after the frontend check still require backend validation, not a UI guarantee. |
| C2: cancellation / late async completion | **PASS for UI callback lifetime.** Passing controlled-promise tests cover remote dismissal and external unmount during registration, suppressing `onRegistered` and late close callbacks. Source also checks lifetime after host refresh. Cancellation is not IPC abortion or rollback of a remote record already persisted by the backend; no such claim is accepted here. |
| C2/C3: preserve remote host and canonical identity | **PASS at App/state surface.** `loadProjects`, native session bootstrap, registration merge, serialization and restore retain `target`. `toRegisteredProject` uses the server's workspace ID/root/Git root/host ID. Real App tests cover stored and chooser targets, canonical ID/path adoption and persisted catalog target. `projectIdentity` rejects malformed targets and reserved `ssh:` IDs without valid SSH targets instead of converting to Local. |
| C3: active/inactive remote paths never locally register/list | **PASS for exercised App/state routes.** Active effect chooses remote registration and gates readiness/restore on success. Failed host stays unavailable with no terminal/local call; focus retries remote. Inactive hook returns an explicit-owner root without local registration/listing, including worktree-change events. Remote root-only runtime bypasses local Git listing. Tests assert no local registration, list, or DAG-watch call; stale registration after switching is ignored. |
| C2/C3: new tabs, splits, restored/queued roots | **PASS for frontend workspace routing, not actual SSH shell.** Real App/store/runtime/restore tests preserve backend remote workspace ID through new terminal, split request and native-startup restored/save flows. Deferred restore/switch test proves queued SSH-root selection waits for restoration. Explicit root owner distinguishes identical local/remote paths; identity tests also distinguish two SSH hosts. Backend must independently enforce remote spawning for these IDs. |
| C3: unsupported local actions fail closed | **PASS for scoped UI affordances; backend errors pending separately.** Sidebar remote root is not the local worktree row; Git/create and local reveal menu entries are disabled, and stale disabled menu callbacks are guarded. App rejects remote worktree creation and filters remote DAG watch roots/sessions. Tests execute stale menu callbacks and assert no reveal/create. Structured `UNSUPPORTED` results for backend mutation/file helpers are not proven by these UI tests. |
| C3: regression/quality and scope | **NOT unconditional PASS.** Producer-focused suites are green; B1 and B2 remain. No new polling/`any`/suppression was found in scoped additions (section 6). Foreign dirty changes already existed on entry; historical authorship cannot be inferred from a shared dirty tree. This verifier made no source/test edits. |

The runtime report's old Remove Project label caveat is no longer current in the inspected source: `RemoveProjectDialog` derives folder plus shared-inventory host label/ID instead of printing the opaque workspace hash. This is source-inspected; no new remote-removal execution is claimed.

## 3. Independent focused Vitest executions

Working directory: `/Users/indo/code/project/orca-lite`. `ui/package.json` maps `test` to `vitest run --maxWorkers=1`. These are Vitest executions via Bun's script runner, not the separate `bun test` runner. Each command below was executed once; no test retry was used.

### Chooser producer command - exit 0

```sh
bun run --cwd ui test src/components/ProjectDialogs.test.tsx src/lib/remoteProject.test.ts src/lib/sshHosts.test.tsx
```

```text
RUN v3.2.7 /Users/indo/code/project/orca-lite/ui

src/lib/sshHosts.test.tsx (16 tests) 30ms
src/components/ProjectDialogs.test.tsx (31 tests) 222ms
src/lib/remoteProject.test.ts (5 tests) 3ms

Test Files  3 passed (3)
     Tests  52 passed (52)
  Start at  01:45:41
  Duration  2.47s
```

### Runtime producer command - exit 0

```sh
bun run --cwd ui test \
  src/App.remote.test.tsx \
  src/state/inactiveProjectWorktrees.remote.test.tsx \
  src/components/Sidebar.remote.test.tsx \
  src/lib/projectIdentity.test.ts \
  src/state/inactiveProjectWorktrees.test.tsx \
  src/state/workspaceRuntime.test.tsx \
  src/state/workspaceRestore.test.tsx \
  src/state/workspaceStore.test.tsx \
  src/lib/sessionPersistence.test.ts \
  src/lib/worktreeOwnership.test.ts \
  src/components/Sidebar.test.tsx \
  src/components/Sidebar.dnd.test.tsx \
  src/components/Sidebar.activity.test.tsx
```

```text
RUN v3.2.7 /Users/indo/code/project/orca-lite/ui

src/components/Sidebar.test.tsx (30 tests) 399ms
src/App.remote.test.tsx (12 tests) 348ms
src/state/workspaceRuntime.test.tsx (11 tests) 573ms
src/components/Sidebar.dnd.test.tsx (7 tests) 181ms
src/components/Sidebar.remote.test.tsx (3 tests) 84ms
src/state/workspaceRestore.test.tsx (17 tests) 474ms
src/components/Sidebar.activity.test.tsx (4 tests) 110ms
src/state/workspaceStore.test.tsx (43 tests) 83ms
src/lib/sessionPersistence.test.ts (25 tests) 12ms
src/state/inactiveProjectWorktrees.test.tsx (7 tests) 15ms
src/state/inactiveProjectWorktrees.remote.test.tsx (1 test) 9ms
src/lib/projectIdentity.test.ts (3 tests) 5ms
src/lib/worktreeOwnership.test.ts (7 tests) 2ms

Test Files  13 passed (13)
     Tests  170 passed (170)
  Start at  01:45:41
  Duration  10.08s
```

### Broader App and actual Settings shell/section - exit 1

```sh
bun run --cwd ui test src/App.test.tsx src/components/SettingsDialog.ssh.test.tsx src/components/settings/SshSection.test.tsx
```

```text
src/App.test.tsx (89 tests | 1 failed) 3202ms
src/components/settings/SshSection.test.tsx (14 tests) 311ms
src/components/SettingsDialog.ssh.test.tsx (2 tests) 117ms

Test Files  1 failed | 2 passed (3)
     Tests  1 failed | 104 passed (105)
  Start at  01:46:31
  Duration  7.13s
error: script "test" exited with code 1
```

Failure assertion and source location are retained in B2; verbose DOM serialization is omitted here. The local Add Project -> explicit Local -> actual branch-dropdown integration case passed in this run.

## 4. Fresh LSP results

Called `lsp_diagnostics` with `severity: "all"`, in parallel, on the following 26 files. Tools return diagnostic text, not a shell exit code.

**App only:**

```text
ui/src/App.tsx
hint[typescript] (80006) at 943:37: This may be converted to an async function.
hint[typescript] (80006) at 950:4: This may be converted to an async function.
```

**Each of the following returned `No diagnostics found`:**

```text
ui/src/App.test.tsx
ui/src/App.remote.test.tsx
ui/src/components/ProjectDialogs.tsx
ui/src/components/ProjectDialogs.test.tsx
ui/src/components/Sidebar.tsx
ui/src/components/Sidebar.remote.test.tsx
ui/src/components/SettingsDialog.tsx
ui/src/components/settings/SshSection.tsx
ui/src/components/settings/types.ts
ui/src/lib/remoteProject.ts
ui/src/lib/remoteProject.test.ts
ui/src/lib/sshHosts.ts
ui/src/lib/projectIdentity.ts
ui/src/lib/projectIdentity.test.ts
ui/src/lib/sessionPersistence.ts
ui/src/lib/tauri.ts
ui/src/lib/types.ts
ui/src/lib/worktreeOwnership.ts
ui/src/state/inactiveProjectWorktrees.ts
ui/src/state/inactiveProjectWorktrees.test.tsx
ui/src/state/inactiveProjectWorktrees.remote.test.tsx
ui/src/state/workspaceRestore.ts
ui/src/state/workspaceRuntime.ts
ui/src/state/workspaceStore.ts
ui/src/state/workspaceStore.test.tsx
```

No full build was run, as requested until lead integration.

## 5. Supplemental actual chooser + shared hook + adapter execution

The producer's chooser suite mocks both `useSshHosts` and the remote module; App's remote suite mocks the hook and registration transport. Their individual GREEN results do not by themselves exercise the entire chooser/hook/adapter chain. A read-only stdin harness therefore loaded the real three modules through Vite, rendered the component in JSDOM, and substituted only core Tauri transport, native picker, and unrelated local Tauri exports.

This is **DOM/transport-fixture execution, not native desktop, actual disk storage, SSH connectivity, or a full build**. It uses direct React `act` and awaited mutation promises, without sleeps/polling. No source/test/harness file was created.

Initial harness attempt exited 1: Vite externalized the Tauri core import, so its fixture was not used and the real non-Tauri hook produced an empty list (`Unable to find ... remote-host-select`). Closing that server while dependency scanning was active emitted `The server is being restarted or closed. Request is outdated`. This was a harness configuration failure, not counted as product failure or a pass. The corrected command used Vite aliases for transport modules and disabled dependency discovery. It exited 0 with the following complete observation output apart from the expected picker-error stack:

```text
INITIAL_PICKER_CALLS=0
REAL_HOOK_SELECTED_HOST=build
REAL_REMOTE_INVOKE=["cmd_project_register_remote",{"request":{"workspaceId":"input","hostId":"build","repoPath":"/input"}}]
REAL_REMOTE_REGISTERED=[{"workspaceId":"ssh:canonical","repoRoot":"/canonical","gitRoot":null,"target":{"kind":"ssh","hostId":"build"}}]
REMOTE_PICKER_CALLS=0 LOCAL_REGISTER_CALLS=0
REAL_HOOK_DISABLE_EMPTY=true SUBMIT_DISABLED=true
REENABLE_SELECTION=""
REAL_HOOK_DELETE_EMPTY=true SUBMIT_DISABLED=true
Error: picker denied
PICKER_ERROR_MANUAL=true OPEN_CALLS=1
PICKER_RETRY_OPEN_CALLS=1 STUCK_PENDING=true
HARNESS_CLEANUP=complete
```

The exit 0 means observations completed; it does **not** declare `STUCK_PENDING=true` acceptable. That is B1.

Exact corrected command:

```sh
cd ui && node --input-type=module <<'JS'
import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost' });
for (const name of ['window','document','HTMLElement','HTMLSelectElement','Node','MutationObserver','Event','MouseEvent']) globalThis[name]=dom.window[name];
Object.defineProperty(globalThis,'navigator',{value:dom.window.navigator,configurable:true}); globalThis.IS_REACT_ACT_ENVIRONMENT=true;
const React=await import('react'); const {render,screen,fireEvent,act,cleanup}=await import('@testing-library/react'); const {createServer}=await import('vite');
const host={id:'build',label:'Build',hostname:'build.example',source:'manual',authMethod:'agent',disabled:false};
const f=globalThis.__uiVerification={hosts:[host],opens:0,invokes:[],local:[],closed:0,registered:[],open:async()=>{throw Error('picker denied');}};
const mocks={
 '\0verify-core':`export const isTauri=()=>true;export async function invoke(command,args){const f=globalThis.__uiVerification;f.invokes.push([command,args]);if(command==='cmd_ssh_list_hosts')return f.hosts;if(command==='cmd_ssh_delete_host'){f.hosts=f.hosts.filter(h=>h.id!==args.id);return f.hosts;}if(command==='cmd_ssh_update_host'){f.hosts=f.hosts.map(h=>h.id===args.host.id?args.host:h);return f.hosts;}if(command==='cmd_project_register_remote')return {workspaceId:'ssh:canonical',repoRoot:'/canonical',gitRoot:null,hostId:args.request.hostId,hostLabel:'Build'};throw Error(command);}`,
 '\0verify-dialog':`export function open(){const f=globalThis.__uiVerification;f.opens++;return f.open();}`,
 '\0verify-tauri':`export const isTauriRuntime=()=>true;export async function registerProject(r){globalThis.__uiVerification.local.push(r);return {workspaceId:r.workspaceId,repoRoot:r.repoPath};}export async function listProjectBranches(){return [];}export async function createWorktree(){throw Error('unexpected local worktree');}`
};
const aliases={'@tauri-apps/api/core':'\0verify-core','@tauri-apps/plugin-dialog':'\0verify-dialog','../lib/tauri':'\0verify-tauri','./tauri':'\0verify-tauri'};
const server=await createServer({configFile:false,server:{middlewareMode:true},optimizeDeps:{noDiscovery:true,include:[]},resolve:{alias:aliases},esbuild:{jsx:'automatic'},plugins:[{name:'verification-transport',resolveId:id=>Object.hasOwn(mocks,id)?id:null,load:id=>mocks[id]}]});
try{
 const {AddProjectDialog}=await server.ssrLoadModule('/src/components/ProjectDialogs.tsx');const ssh=await server.ssrLoadModule('/src/lib/sshHosts.ts');
 const props={onClose:()=>f.closed++,onRegistered:p=>f.registered.push(p)};
 await act(async()=>{render(React.createElement(AddProjectDialog,props));});console.log('INITIAL_PICKER_CALLS='+f.opens);
 fireEvent.click(screen.getByTestId('project-type-remote'));console.log('REAL_HOOK_SELECTED_HOST='+screen.getByTestId('remote-host-select').value);
 fireEvent.change(screen.getByTestId('remote-repo-path-input'),{target:{value:'/input'}});await act(async()=>{fireEvent.click(screen.getByTestId('add-project-confirm-remote'));});
 console.log('REAL_REMOTE_INVOKE='+JSON.stringify(f.invokes.find(([c])=>c==='cmd_project_register_remote')));console.log('REAL_REMOTE_REGISTERED='+JSON.stringify(f.registered));console.log('REMOTE_PICKER_CALLS='+f.opens+' LOCAL_REGISTER_CALLS='+f.local.length);cleanup();
 await act(async()=>{render(React.createElement(AddProjectDialog,props));});fireEvent.click(screen.getByTestId('project-type-remote'));fireEvent.change(screen.getByTestId('remote-repo-path-input'),{target:{value:'/input'}});
 await act(async()=>{await ssh.updateSshHost({...host,disabled:true});});console.log('REAL_HOOK_DISABLE_EMPTY='+Boolean(screen.queryByTestId('configure-ssh-settings'))+' SUBMIT_DISABLED='+screen.getByTestId('add-project-confirm-remote').disabled);
 await act(async()=>{await ssh.updateSshHost(host);});console.log('REENABLE_SELECTION='+JSON.stringify(screen.getByTestId('remote-host-select').value));fireEvent.change(screen.getByTestId('remote-host-select'),{target:{value:'build'}});
 await act(async()=>{await ssh.deleteSshHost('build');});console.log('REAL_HOOK_DELETE_EMPTY='+Boolean(screen.queryByTestId('configure-ssh-settings'))+' SUBMIT_DISABLED='+screen.getByTestId('add-project-confirm-remote').disabled);cleanup();
 await act(async()=>{render(React.createElement(AddProjectDialog,props));});await act(async()=>{fireEvent.click(screen.getByTestId('project-type-local'));});console.log('PICKER_ERROR_MANUAL='+Boolean(screen.queryByLabelText('Repository path'))+' OPEN_CALLS='+f.opens);
 fireEvent.click(screen.getByTestId('add-project-back'));await act(async()=>{fireEvent.click(screen.getByTestId('project-type-local'));});console.log('PICKER_RETRY_OPEN_CALLS='+f.opens+' STUCK_PENDING='+screen.getByRole('dialog',{name:'Add Project'}).getAttribute('aria-busy'));
}finally{cleanup();await server.close();dom.window.close();console.log('HARNESS_CLEANUP=complete');}
JS
```

Both attempts used middleware-mode servers, not a listening desktop UI. Cleanup closes server and DOM in `finally`. Closing `lsof -nP -iTCP:5173 -sTCP:LISTEN` returned no output (exit 1, no listener). Vite/Vitest can update ignored tool caches; no product source/configuration file was written.

## 6. RED-first, test discipline, and dirty-tree audit

Read the plan (including execution corrections and section 9), chooser/runtime producer reports, separate runtime RED capture, foundation verification, QA environment receipt, actual source, scoped tracked diffs, and relevant untracked source/tests. Ordinary `git diff` omits untracked files; those files were explicitly read.

Historical runtime RED capture contains:

```text
src/App.remote.test.tsx (2 tests | 2 failed)
  actual local registerProject({ repoPath: "/srv/repo", workspaceId: "ssh:remote" })
src/state/inactiveProjectWorktrees.remote.test.tsx (1 test | 1 failed)
  actual local registerProject({ repoPath: "/srv/repo", workspaceId: "ssh:build" })
Test Files  2 failed (2)
     Tests  3 failed (3)
error: script "test" exited with code 1
```

Chooser's embedded RED includes eager picker called once, refresh failure still registering, silent host-2 retarget, and late unmount callback. These are behavioral failures, not missing-import RED. The provenance limitation is retained in section 1.

Searched scoped tracked **added** lines and new remote test/adapter/identity files for `any`, `waitFor`, `findBy`, timers/sleeps, `@ts-`, `eslint-disable`, `.skip(` and `.only(`: no matching additions. Scope included App, chooser, Sidebar, model/persistence/ownership and state diffs. Existing `any` in restore/store tests and existing polling in older App/runtime tests are not newly introduced and were not misreported as new. The updater failure itself demonstrates that the older App suite still contains polling. New remote/chooser tests use controlled promises and React `act`; Vitest bounds their execution. Expected-error console mocks assert the error calls; production registration errors remain surfaced rather than swallowed. The store change removes its previous silent spawn-failure catch and retains the retry cleanup in `finally`.

`git diff --check` for scoped tracked UI changes returned exit 0 (`DIFF_CHECK_EXIT=0`). `tauri.ts` tracked diff is just the RunTarget import and optional RegisteredProject target; `types.ts` additions are explicit remote root ownership and optional session target/Git metadata. No unrelated additions were found in the inspected chooser/runtime diffs. Native terminal/link routing, Rust, vendor, permission, and unrelated documentation changes were already present on entry and were not edited or attributed to this lane merely because they were dirty.

No historical no-foreign-write guarantee is possible without the producers' pre-lane snapshots. The verified claim is this verifier's report-only write scope and the current scoped diff content, not blanket ownership of the shared dirty tree.

### Closing SHA-256 snapshot

```text
fdf9c8aefc54a592d27afac74e63b5a5b156489efb9397a98c98dc6299072a0b  ui/src/components/ProjectDialogs.tsx
9654c5fcfa077c30e0779e72b8626a8502d1bd248e95e32c3e9bfc6b810bab49  ui/src/components/ProjectDialogs.test.tsx
1433a5948986d4713dfa8bc5cbe4176cfac654f8c1efbbc486c3dc4ed363f31d  ui/src/lib/remoteProject.ts
f08f474c4af587631e041f78dbf263c16e5734617cc10fc9d7aa5d5c5c6e2070  ui/src/lib/sshHosts.ts
cd798a716f71ee34cc2949a6350923e9d0580816c303a328cae32dae790bd546  ui/src/App.tsx
e214c711918f69594d7b29987f41d28543cda03355ec7c20d5961bcdd506c81f  ui/src/App.remote.test.tsx
bd7d27381ef2413f3628ade7ef159424068164ddc5e833b26ac9d3d6892b3422  ui/src/state/inactiveProjectWorktrees.ts
1b45d153a5753d98650b5f8a72023dc63c64cbb4d4663672d7f8ac703c7a26a3  ui/src/state/workspaceRuntime.ts
02e4169e912d50d9d00f893f3017b940030bceead2e63d1869e7c855fd5fb955  ui/src/state/workspaceRestore.ts
b0712855e6b2005a9da59c7c6232e0b46988c8574e24dde541006242dd7816d2  ui/src/state/workspaceStore.ts
```

**Conclusion:** Frontend direct remote invocation, shared Settings inventory, fail-closed host selection, target preservation and tested App/state no-local-fallback routes are verified at their stated surfaces. B1 prevents a blanket UI delivery pass; B2 prevents claiming the broader App regression command is green. Historical RED chronology, full integrated build, backend acceptance and native desktop proof remain explicitly separate.

---

## 7. Delta verdict - 2026-09-06 UTC - Local retry and final chooser

- Verification task: `st_01a077af`.
- Executed 2026-09-06 UTC; workstation-local date was **2026-09-07 KST**, with the focused Vitest run starting at 02:05:30. UTC clock receipt after verification: `2026-09-06 17:06:49 UTC`.
- **Current scoped UI code verdict: PASS. B1 is verified fixed.** Local -> picker rejection -> Back -> Local invokes the picker twice; resolving the second promise leaves pending, displays the selected path, and permits successful local registration. The duplicate in-flight and StrictMode protections remain effective.
- **Native desktop gate: STILL UNAVAILABLE / NOT VERIFIED.** This is not a native picker, WGPU occlusion, screenshot, live SSH, or backend acceptance receipt.
- Sections 1-6 above are retained unchanged as the **historical pre-fix report**, including their B1 failure and older 52-test result. This delta supersedes those B1/current-chooser conclusions only; it does not erase them or reinterpret their original observations as passing.
- **B2 remains explicit:** the previously captured broader App updater failure is classified as pre-existing on the baseline evidence described above, not independently re-created in this delta. Its assertion remains `expected "spy" to be called once, but got 0 times` at `src/App.test.tsx:662:56`, with `1 failed | 104 passed (105)`. No broader-App-green or unconditional all-gates acceptance claim is made.
- The previously approved 170-test runtime execution and actual-hook remote registration/inventory scenarios were not rerun. Their prior, surface-limited verdicts are retained. No broad exploration, build, implementation/test edits, native automation, or commits were performed; this verifier wrote only this report.

### 7.1 Independently inspected mechanism and final delta

Read `chooser.md` and the complete current `ProjectDialogs.tsx`/test, `remoteProject.ts`/test, and `sshHosts.ts`/test. The actual Local handler now resets `pickerOpenedRef.current = false` in the native promise chain's `finally`. The ref is set before calling `open()` and remains true until settlement; a duplicate in-flight handler invocation returns before another `open()`. The rejection handler still surfaces the error and switches to `local-manual`; Back returns to `choose-location`. These transitions now lead to a new explicit Local attempt rather than a ref that stays locked forever.

The added deterministic regression uses two distinct deferred picker promises, rejects the first within `act`, navigates Back and Local, asserts exactly two picker calls, resolves the second within `act`, and asserts the selected path. Existing StrictMode and parent-rerender tests pass. The supplemental execution below additionally dispatches two Local clicks inside one synchronous `act` before React commits the pending surface, exercises a StrictMode parent rerender while the first promise remains pending, and completes registration after the successful retry.

**Friendly remote removal: PASS at display/callback surface, backend-ID preservation source-confirmed.** The two added removal tests execute configured-label display (`my-service (Dev Server)`, no opaque hash), unknown-host fallback (`legacy-repo (unknown-host-id)`), and unchanged confirmation/close callbacks. Existing local removal display remains covered. The real-hook supplement renders `my-service (Build)` from shared inventory with a frozen project object and confirms its removal callback retains `ssh:canonical`. This is not an actual unregister IPC execution: `RemoveProjectDialog` calls the existing no-argument `onConfirm`, not a renamed-ID argument. A narrowly scoped caller inspection confirms `App.tsx:1182-1204` captures `pendingProjectRemove` and calls `unregisterProject({ workspaceId: target.workspaceId })`; its dialog wiring at 2118-2123 uses that handler. The friendly name is presentation-only and does not replace the backend identity.

The real remote adapter and shared-host source hashes match the historical snapshot. Their focused tests remain green; the chooser still awaits authoritative refresh, fails closed on its rejection/unavailable host, and maps server identity through the adapter. No new full-chain remote registration proof is claimed beyond the retained historical execution.

### 7.2 Fresh executable checks

Executed the requested command **once**, exit 0; these are Vitest tests through Bun's script runner:

```sh
bun run --cwd ui test src/components/ProjectDialogs.test.tsx src/lib/remoteProject.test.ts src/lib/sshHosts.test.tsx
```

```text
$ vitest run --maxWorkers=1 src/components/ProjectDialogs.test.tsx src/lib/remoteProject.test.ts src/lib/sshHosts.test.tsx
RUN v3.2.7 /Users/indo/code/project/orca-lite/ui

src/components/ProjectDialogs.test.tsx (34 tests) 244ms
src/lib/sshHosts.test.tsx (16 tests) 27ms
src/lib/remoteProject.test.ts (5 tests) 3ms

Test Files  3 passed (3)
     Tests  55 passed (55)
  Start at  02:05:30
  Duration  2.59s
```

Fresh parallel `lsp_diagnostics`, severity `all`, returned **No diagnostics found** for each of these six files:

```text
ui/src/components/ProjectDialogs.tsx
ui/src/components/ProjectDialogs.test.tsx
ui/src/lib/remoteProject.ts
ui/src/lib/remoteProject.test.ts
ui/src/lib/sshHosts.ts
ui/src/lib/sshHosts.test.tsx
```

`git diff --check -- ui/src/components/ProjectDialogs.tsx ui/src/components/ProjectDialogs.test.tsx` returned `DIFF_CHECK_EXIT=0`. Searching the three scoped test files for `waitFor`, `findBy`, `setTimeout`, `setInterval`, `.skip(` and `.only(` returned no matches. No tests were added or altered by this verifier. The supplemental harness uses controlled promises and direct React `act`, no sleeps or polling, with a 60-second tool execution bound.

### 7.3 Actual shared-hook retry supplement - exit 0, assertions passed

Adapted the existing section 5 stdin recipe to the exact B1 retry, duplicate-guard and friendly-removal seams. Vite loads the actual component, shared SSH hook, and remote adapter; only core transport, native picker, and unrelated local Tauri exports are substituted. The second picker promise is explicitly resolved, and assertions fail the command if the retry hangs in pending or identity changes. The expected picker rejection is logged by production code, not suppressed.

Observed output (expected error stack elided, error retained):

```text
STRICT_INITIAL_PICKER_CALLS=0 REAL_SHARED_HOST=build
DUPLICATE_INFLIGHT_AND_STRICT_RERENDER_OPEN_CALLS=1
Error: picker denied
PICKER_ERROR_MANUAL=true OPEN_CALLS=1
PICKER_RETRY_OPEN_CALLS=2 SECOND_PROMISE_RESOLVED=true STUCK_PENDING=false
RETRY_LOCAL_REGISTERED=true CLOSE_CALLS=1
REAL_HOOK_REMOTE_LABEL=my-service (Build) REMOVAL_CALLBACK_ID=ssh:canonical
DELTA_ASSERTIONS=PASS
HARNESS_CLEANUP=complete
```

Exact command, run once without a harness file or listening UI server:

```sh
cd ui && node --input-type=module <<'JS'
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'http://localhost' });
for (const name of ['window','document','HTMLElement','HTMLSelectElement','Node','MutationObserver','Event','MouseEvent']) globalThis[name] = dom.window[name];
Object.defineProperty(globalThis, 'navigator', { value: dom.window.navigator, configurable: true });
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const React = await import('react');
const { render, screen, fireEvent, act, cleanup } = await import('@testing-library/react');
const { createServer } = await import('vite');
function deferred() { let resolve, reject; const promise = new Promise((res, rej) => { resolve = res; reject = rej; }); return { promise, resolve, reject }; }
const first = deferred(), second = deferred(), registration = deferred();
const host = { id: 'build', label: 'Build', hostname: 'build.example', source: 'manual', authMethod: 'agent', disabled: false };
const f = globalThis.__uiVerification = { hosts: [host], opens: [], local: [], closed: 0, registered: [], pickers: [first.promise, second.promise], registration: registration.promise };
const mocks = {
 '\0verify-core': `export const isTauri=()=>true;export async function invoke(command){if(command==='cmd_ssh_list_hosts')return globalThis.__uiVerification.hosts;throw Error('Unexpected IPC: '+command);}`,
 '\0verify-dialog': `export function open(options){const f=globalThis.__uiVerification;f.opens.push(options);const promise=f.pickers.shift();if(!promise)throw Error('Unexpected duplicate picker');return promise;}`,
 '\0verify-tauri': `export const isTauriRuntime=()=>true;export function registerProject(r){const f=globalThis.__uiVerification;f.local.push(r);return f.registration;}export async function listProjectBranches(){return [];}export async function createWorktree(){throw Error('Unexpected local worktree');}`
};
const aliases = { '@tauri-apps/api/core': '\0verify-core', '@tauri-apps/plugin-dialog': '\0verify-dialog', '../lib/tauri': '\0verify-tauri', './tauri': '\0verify-tauri' };
const server = await createServer({ configFile: false, server: { middlewareMode: true }, optimizeDeps: { noDiscovery: true, include: [] }, resolve: { alias: aliases }, esbuild: { jsx: 'automatic' }, plugins: [{ name: 'verification-transport', resolveId: id => Object.hasOwn(mocks, id) ? id : null, load: id => mocks[id] }] });
try {
 const { AddProjectDialog, RemoveProjectDialog } = await server.ssrLoadModule('/src/components/ProjectDialogs.tsx');
 const ssh = await server.ssrLoadModule('/src/lib/sshHosts.ts');
 const props = { onClose: () => f.closed++, onRegistered: p => f.registered.push(p) };
 const view = await act(async () => render(React.createElement(React.StrictMode, null, React.createElement(AddProjectDialog, props))));
 assert.equal(f.opens.length, 0);
 assert.equal(ssh.getCachedSshHosts()[0].id, 'build');
 console.log('STRICT_INITIAL_PICKER_CALLS=0 REAL_SHARED_HOST=build');
 const local = screen.getByTestId('project-type-local');
 act(() => { fireEvent.click(local); fireEvent.click(local); });
 assert.equal(f.opens.length, 1);
 view.rerender(React.createElement(React.StrictMode, null, React.createElement(AddProjectDialog, { ...props, onClose: () => f.closed++ })));
 assert.equal(f.opens.length, 1);
 assert.equal(screen.getByRole('dialog', { name: 'Add Project' }).getAttribute('aria-busy'), 'true');
 console.log('DUPLICATE_INFLIGHT_AND_STRICT_RERENDER_OPEN_CALLS=1');
 await act(async () => { first.reject(new Error('picker denied')); });
 assert.ok(screen.getByLabelText('Repository path'));
 assert.equal(f.closed, 0);
 console.log('PICKER_ERROR_MANUAL=true OPEN_CALLS=1');
 fireEvent.click(screen.getByTestId('add-project-back'));
 fireEvent.click(screen.getByTestId('project-type-local'));
 assert.equal(f.opens.length, 2);
 assert.equal(screen.getByRole('dialog', { name: 'Add Project' }).getAttribute('aria-busy'), 'true');
 await act(async () => { second.resolve('/Users/dev/retried-project'); });
 assert.ok(screen.getByText('/Users/dev/retried-project'));
 assert.notEqual(screen.getByRole('dialog', { name: 'Add Project' }).getAttribute('aria-busy'), 'true');
 assert.equal(f.opens.length, 2);
 assert.deepEqual(f.opens, Array.from({ length: 2 }, () => ({ directory: true, multiple: false, title: 'Add Project' })));
 console.log('PICKER_RETRY_OPEN_CALLS=2 SECOND_PROMISE_RESOLVED=true STUCK_PENDING=false');
 fireEvent.click(screen.getByRole('button', { name: 'Add Project' }));
 assert.deepEqual(f.local, [{ workspaceId: 'retried-project', repoPath: '/Users/dev/retried-project' }]);
 const registered = { workspaceId: 'retried-project', repoRoot: '/Users/dev/retried-project', gitRoot: null };
 await act(async () => { registration.resolve(registered); });
 assert.deepEqual(f.registered, [registered]);
 assert.equal(f.closed, 1);
 console.log('RETRY_LOCAL_REGISTERED=true CLOSE_CALLS=1');
 cleanup();
 const project = Object.freeze({ workspaceId: 'ssh:canonical', repoRoot: '/srv/apps/my-service', gitRoot: null, target: Object.freeze({ kind: 'ssh', hostId: 'build' }) });
 const removed = [];
 render(React.createElement(RemoveProjectDialog, { project, onClose: () => {}, onConfirm: () => removed.push(project.workspaceId) }));
 assert.ok(screen.getByText('my-service (Build)'));
 assert.equal(screen.queryByText('ssh:canonical'), null);
 fireEvent.click(screen.getByRole('button', { name: 'Remove Project' }));
 assert.deepEqual(removed, ['ssh:canonical']);
 assert.equal(project.workspaceId, 'ssh:canonical');
 console.log('REAL_HOOK_REMOTE_LABEL=my-service (Build) REMOVAL_CALLBACK_ID=ssh:canonical');
 console.log('DELTA_ASSERTIONS=PASS');
} finally { cleanup(); await server.close(); dom.window.close(); console.log('HARNESS_CLEANUP=complete'); }
JS
```

### 7.4 Verified delta SHA-256 snapshot and acceptance boundary

These six hashes were captured before the executable checks completed and checked again afterward; they were unchanged between captures:

```text
b67b7b4361358de3694948fc69ece62bd890d9c5e0f0b4134e90f9e7dda15cc7  ui/src/components/ProjectDialogs.tsx
267bdfa25fdb4ba88c22150f0f604cc549cc7e9539aa85bb3399e337f55e280e  ui/src/components/ProjectDialogs.test.tsx
1433a5948986d4713dfa8bc5cbe4176cfac654f8c1efbbc486c3dc4ed363f31d  ui/src/lib/remoteProject.ts
b88d1eb66d54749e5f1c13c9c05f5c8a59b768c3e03ccef8e814098099f01ac1  ui/src/lib/remoteProject.test.ts
f08f474c4af587631e041f78dbf263c16e5734617cc10fc9d7aa5d5c5c6e2070  ui/src/lib/sshHosts.ts
dc150d7d5ad799b195d5a20708550005a247bf8572c839a15acf6de4a72fba54  ui/src/lib/sshHosts.test.tsx
```

**Final delta conclusion:** B1 is closed by independent deterministic component and actual-shared-hook execution. The final chooser/removal delta passes at its UI code and transport-fixture surfaces. The historical runtime acceptance is retained without rerunning it. The pre-existing App updater failure remains recorded, and native desktop acceptance remains unavailable and separate; neither is converted to a pass by these 55 green tests.
