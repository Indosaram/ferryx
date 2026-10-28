// surfaces (plan task 14): the real remote web client, signed in to a real self-hosted account,
// drives the unified supervisor of one isolated daemon at 1440x900 (wide layout, sidebar
// Supervisor tab) and 390x844 (phone home), in light and dark. Submit goes through the
// composer, pause through the phone, resume through the wide layout; each surface must show the
// goal and revision the hub database and the daemon snapshot (the desktop app's IPC request)
// report. Stopping the daemon must leave the last goal shown as disconnected, never blank.
//
// Every product effect goes through product surfaces: `ferryx-cli supervisor setup|request`,
// `ferryx-cli account login` (device authorization), the daemon socket the GUI uses (workspace,
// remote relay mode, pane write, snapshot), the relay with its embedded account service (the
// file mailer's magic links are opened like a person would), and the browser UI itself. The hub
// database is only read, read-only.
//
// Not covered here (listed in assertions.json): the native desktop window (MahoRD capture), a
// goal reaching `completed` (needs a runnable verifier and T10's mhc judge, see notCovered), and
// typing/native-pane masks (ui unit tests).
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HarnessError, isInside, POSIX } from './harness.mjs';
import {
  DAEMON_PROTOCOL_VERSION, GATEWAY_PORT, WORKER, daemonExchange, freeLoopbackPort, onPath, openHubReader, portIsFree, readWorkerLogs, shellQuote, waitForChange,
} from './scenarios.mjs';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const WORKSPACE_ID = 'qa-surfaces';
const PROJECT = 'qa-supervisor';
const REPO_IDENTITY = 'qa:qa-surfaces';
const FIXTURE_TEXT = 'Create result.txt containing READY. Run the verifier. Correct failures until it passes.';
const EVENT_KEY = 'qa-surfaces-release';
const UI_DEADLINE_MS = 60_000;
const LAYOUTS = Object.freeze({
  desktop: { width: 1440, height: 900, mobile: false },
  phone: { width: 390, height: 844, mobile: true },
});
const APPEARANCE_KEY = 'ferryx.settings.appearance';

function loadPlaywright() {
  const require = createRequire(join(REPO_ROOT, 'ui', 'package.json'));
  const failures = [];
  for (const candidate of [process.env.PLAYWRIGHT_CORE_PATH, 'playwright-core', 'playwright'].filter(Boolean)) {
    try { return require(candidate); } catch (error) { failures.push(`${candidate}: ${error?.message ?? error}`); }
  }
  throw new HarnessError('PREREQUISITE_MISSING', `playwright-core is not resolvable from ui/ (set PLAYWRIGHT_CORE_PATH): ${failures.join('; ')}`);
}

// Runs in the page: is the supervisor section present, non-blank, inside the viewport width,
// and are its visible controls and labels free of overlaps?
function probeLayout(selector) {
  const root = document.querySelector(selector);
  const viewportWidth = document.documentElement.clientWidth;
  if (!root) return { found: false, viewportWidth };
  const box = root.getBoundingClientRect();
  const text = (root.innerText || '').trim();
  const leaves = [...root.querySelectorAll('button, textarea, input, select, [data-testid="supervisor-run-state"], [data-testid="supervisor-goal-id"], [data-testid="supervisor-goal-rev"], [data-testid="supervisor-goal-wait"], [data-testid="supervisor-connection"]')]
    .map((element) => ({ element, id: element.getAttribute('data-testid') || element.tagName.toLowerCase(), rect: element.getBoundingClientRect() }))
    .filter((item) => item.rect.width > 0 && item.rect.height > 0);
  const overlaps = [];
  for (let i = 0; i < leaves.length; i += 1) {
    for (let j = i + 1; j < leaves.length; j += 1) {
      const a = leaves[i]; const b = leaves[j];
      if (a.element.contains(b.element) || b.element.contains(a.element)) continue;
      const w = Math.min(a.rect.right, b.rect.right) - Math.max(a.rect.left, b.rect.left);
      const h = Math.min(a.rect.bottom, b.rect.bottom) - Math.max(a.rect.top, b.rect.top);
      if (w > 1 && h > 1) overlaps.push({ a: a.id, b: b.id, w: Math.round(w), h: Math.round(h) });
    }
  }
  const outside = leaves.filter((item) => item.rect.left < -1 || item.rect.right > viewportWidth + 1).map((item) => ({ id: item.id, left: Math.round(item.rect.left), right: Math.round(item.rect.right) }));
  return {
    found: true, viewportWidth, width: Math.round(box.width), height: Math.round(box.height), textLength: text.length,
    documentOverflowX: document.documentElement.scrollWidth > viewportWidth + 1, overlaps, outside, leaves: leaves.length,
    theme: document.documentElement.dataset.theme ?? null,
  };
}

const layoutOk = (probe) => probe?.found === true && probe.width > 0 && probe.height > 0 && probe.textLength > 0
  && probe.documentOverflowX === false && probe.overlaps.length === 0 && probe.outside.length === 0;

async function runSurfaces(ctx) {
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'surfaces runs the POSIX fixture worker pane; run it on Linux or macOS');
  const { config } = ctx;
  if (!config.ferryxRelay) throw new HarnessError('CONFIG_INVALID', 'surfaces needs ferryxRelay: the relay hosts the account service and serves the web client');
  if (!config.ferryxUiDist || !existsSync(join(config.ferryxUiDist, 'index.html')) || !statSync(config.ferryxUiDist).isDirectory()) {
    throw new HarnessError('CONFIG_INVALID', 'surfaces needs ferryxUiDist: the absolute ui/dist directory (with index.html) built from the same revision');
  }
  const git = onPath('git');
  if (!git) throw new HarnessError('PREREQUISITE_MISSING', 'git is not on PATH; the fixture project is a git repository');
  if (!(await portIsFree(GATEWAY_PORT))) throw new HarnessError('PREREQUISITE_MISSING', `127.0.0.1:${GATEWAY_PORT} is in use; the isolated daemon's gateway must bind it`);
  const { chromium } = loadPlaywright();

  const data = realpathSync(ctx.paths.data);
  const dbPath = join(data, 'hub', 'hub.sqlite');
  const socket = join(data, 'runtime', 'daemon.sock');
  for (const path of [dbPath, socket]) if (!isInside(path, data)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${path} is outside the run root`);
  const transcript = (line) => ctx.evidence.appendText('surfaces.log', `${new Date().toISOString()} ${line}\n`);
  const exchange = async (label, request, deadlineMs = 30_000) => {
    const frames = [{ type: 'handshake', version: DAEMON_PROTOCOL_VERSION }, request];
    const replies = await daemonExchange(socket, frames, deadlineMs);
    ctx.evidence.appendJsonl('socket.jsonl', { label, request, replies, at: new Date().toISOString() });
    return replies.at(-1) ?? null;
  };

  // 1. Fixture project, worker shell and workspace (as in admission).
  const workspaceRoot = join(data, 'ws');
  const repo = join(workspaceRoot, PROJECT);
  mkdirSync(repo, { recursive: true });
  const gitArgs = ['-c', 'init.defaultBranch=main', '-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid'];
  for (const [label, args] of [['git-init', [...gitArgs, 'init', '-q', repo]], ['git-commit', [...gitArgs, '-C', repo, 'commit', '-q', '--allow-empty', '-m', 'qa fixture']]]) {
    const result = await ctx.run(label, git, args, { deadlineMs: 30_000 });
    if (result.code !== 0) throw new HarnessError('FIXTURE_FAILED', `${label} exited ${result.code}: ${result.stderr}`);
  }
  const workerLogs = join(data, 'workers');
  mkdirSync(workerLogs, { recursive: true });
  const shell = join(data, 'bin', 'qa-surfaces-shell');
  mkdirSync(dirname(shell), { recursive: true });
  writeFileSync(shell, `#!/bin/sh\ncase "$1" in\n  -*c*) shift; exec /bin/sh -c "$1" ;;\nesac\nexec ${shellQuote(process.execPath)} ${shellQuote(WORKER)} ${shellQuote(workerLogs)}\n`, { mode: 0o755 });

  // 2. Relay with its embedded account service; it also serves the web client from ferryxUiDist.
  const relayPort = await freeLoopbackPort();
  const origin = `http://127.0.0.1:${relayPort}`;
  const accountData = join(data, 'account');
  const mailDir = join(accountData, 'mail');
  mkdirSync(mailDir, { recursive: true, mode: 0o700 });
  const machineToken = `qa-${ctx.runId}-${relayPort}`;
  const relayEnv = ctx.productEnv({
    FERRYX_DEPLOYMENT_MODE: 'selfhost', FERRYX_RELAY_MACHINE_TOKENS: machineToken, FERRYX_ACCOUNT_DATA_DIR: accountData,
    FERRYX_ACCOUNT_ORIGIN: origin, FERRYX_ACCOUNT_RELAY_ORIGIN: origin, FERRYX_UI_DIST_DIR: realpathSync(config.ferryxUiDist), RUST_LOG: 'info',
  });
  const relayCursor = ctx.journal.cursor();
  const relay = await ctx.spawn('relay', config.ferryxRelay, ['--port', String(relayPort)], { env: relayEnv, cwd: data, graceMs: 5_000 });
  await ctx.waitFor('relay listening', (e) => e.source === 'relay' && (e.kind === 'stdout' || e.kind === 'stderr') && String(e.line ?? '').includes('ferryx-relay listening'), { after: relayCursor, deadlineMs: 30_000 });
  transcript(`relay pid=${relay.pid} origin=${origin}`);

  const mailFiles = () => (existsSync(mailDir) ? readdirSync(mailDir).filter((name) => name.startsWith('magic-link-')).sort() : []);
  // Subscribes to the mail directory before `trigger` runs, then returns the one new link.
  const nextMail = async (label, trigger) => {
    const before = new Set(mailFiles());
    const arrived = waitForChange(ctx, `${label} mail`, mailDir, () => {
      const fresh = mailFiles().filter((name) => !before.has(name));
      return { done: fresh.length > 0, fresh };
    }, 60_000);
    arrived.catch(() => {});
    const triggered = await trigger();
    const { fresh } = await arrived;
    const url = readFileSync(join(mailDir, fresh[0]), 'utf8').trim();
    ctx.evidence.appendJsonl('mail.jsonl', { label, file: fresh[0], kind: url.includes('/device/approve') ? 'device-approve' : 'login', at: new Date().toISOString() });
    return { url, triggered };
  };

  // 3. Owner set-up, then the machine joins the account through device authorization.
  const email = `${config.ownerPrincipal.replace(/[^A-Za-z0-9._-]/g, '-')}@example.invalid`;
  const daemonPort = await freeLoopbackPort();
  const env = ctx.productEnv({
    SHELL: shell, FERRYX_HUB_TRACKER_ADDR: `127.0.0.1:${daemonPort}`, FERRYX_HUB_WORKSPACE: WORKSPACE_ID, OMON_LANE_AUTO_WORKTREE: '1',
    FERRYX_ALLOW_INSECURE_RELAY: '1', FERRYX_MACHINE_TOKEN: machineToken, FERRYX_RELAY_URL: origin, FERRYX_ACCOUNT_ORIGIN: origin,
  });
  const setup = await ctx.cli(['supervisor', 'setup', '--owner', config.ownerPrincipal, '--project', PROJECT, '--repo-identity', REPO_IDENTITY,
    '--host', 'local', '--git-common-dir', realpathSync(join(repo, '.git')), '--json'], { label: 'supervisor-setup', env });
  if (setup.code !== 0 || !setup.json?.projectId) throw new HarnessError('SETUP_FAILED', `supervisor setup failed: ${setup.stdout} ${setup.stderr}`);
  const projectId = setup.json.projectId;
  const loginCursor = ctx.journal.cursor();
  const { url: approveUrl } = await nextMail('device-login', async () => ctx.spawn('account-login', config.ferryxCli, ['account', 'login', '--email', email, '--origin', origin], { env, cwd: data, graceMs: 3_000 }));
  const approved = await fetch(approveUrl, { signal: ctx.signal });
  const loginExit = await ctx.waitFor('account login exit', (e) => e.source === 'account-login' && e.kind === 'exit', { after: loginCursor, deadlineMs: 120_000 });
  ctx.assert('machine-enrolled-in-account', approved.status === 200 && loginExit.code === 0, { approveStatus: approved.status, exit: loginExit.code });

  // 4. The isolated daemon in relay mode with agent control on; the hub runs the dispatcher.
  const daemon = await ctx.startDaemon({ env, deadlineMs: 60_000 });
  const registered = await exchange('register-workspace', { type: 'registerWorkspace', workspaceId: WORKSPACE_ID, repoRoot: realpathSync(workspaceRoot) });
  const control = await exchange('agent-control-on', { type: 'agentControlSettingsSet', enabled: true, excludedDeviceIds: [] });
  const daemonLog = join(env.FERRYX_DATA_DIR, 'logs', 'daemon.log');
  const connectedLines = () => (existsSync(daemonLog) ? readFileSync(daemonLog, 'utf8').split('\n').filter((line) => line.includes('relay control channel connected')).length : 0);
  const configured = await exchange('relay-mode', { type: 'remoteConfigure', config: { mode: 'relay', port: GATEWAY_PORT, allowControl: true, relayUrl: origin } });
  mkdirSync(dirname(daemonLog), { recursive: true });
  await waitForChange(ctx, 'relay control channel', dirname(daemonLog), () => ({ done: connectedLines() >= 1 }), 60_000);
  ctx.assert('daemon-reachable-through-relay', registered?.type === 'registerWorkspaceOk' && control?.type === 'agentControlSettingsOk' && configured?.type === 'remoteConfigureOk',
    { registered, control, configured });

  const hub = await openHubReader(dbPath);
  const goalRow = (goalId) => hub.one('SELECT run_state, revision FROM supervisor_goals WHERE goal_id = ?', goalId);
  const daemonGoal = async (label, goalId) => {
    const reply = await exchange(label, { type: 'supervisorSnapshot' });
    return reply?.snapshot?.supervisor?.goals?.find?.((goal) => goal.goalId === goalId) ?? null;
  };

  // 5. Browser: one wide and one phone context, each signed in through the account UI.
  const browser = await chromium.launch({ headless: true, channel: process.env.QA_BROWSER_CHANNEL || 'chrome' });
  const pageErrors = [];
  const shots = [];
  const surfaces = [];
  try {
    const signIn = async (name) => {
      const layout = LAYOUTS[name];
      const context = await browser.newContext({ viewport: { width: layout.width, height: layout.height }, isMobile: layout.mobile, hasTouch: layout.mobile, deviceScaleFactor: layout.mobile ? 3 : 1, colorScheme: 'dark' });
      // Appearance "system": the client follows prefers-color-scheme live (settingsRuntimeBridge),
      // so emulateMedia switches light/dark in place without a reload.
      await context.addInitScript(([key]) => {
        if (!localStorage.getItem(key)) localStorage.setItem(key, JSON.stringify({ theme: 'system', accentColor: 'default', density: 'compact' }));
      }, [APPEARANCE_KEY]);
      const page = await context.newPage();
      surfaces.push({ name, layout, page });
      page.setDefaultTimeout(UI_DEADLINE_MS);
      page.on('pageerror', (error) => pageErrors.push({ surface: name, message: String(error?.message ?? error) }));
      await page.goto(origin, { waitUntil: 'domcontentloaded' });
      await page.fill('[data-testid="account-email-input"]', email);
      const { url } = await nextMail(`${name}-login`, () => page.click('[data-testid="request-magic-link-btn"]'));
      // Opening the emailed link in this browser consumes the code and signs it in.
      await page.goto(url, { waitUntil: 'domcontentloaded' });
      return { name, layout, context, page };
    };
    // Opens the Supervisor view of this surface and waits until the goal (if any) is rendered.
    const openSupervisor = async (surface, goalId = null) => {
      const { page, layout } = surface;
      if (layout.mobile) {
        const tab = page.locator('[data-testid="mobile-home-tab-supervisor"]');
        await tab.waitFor({ state: 'visible' });
        await tab.click();
      } else {
        const tab = page.locator('[data-testid="remote-desktop-sidebar"]').getByRole('tab', { name: /^Supervisor/ });
        await tab.waitFor({ state: 'visible' });
        await tab.click();
      }
      await page.waitForSelector('[data-testid="supervisor-unified"]', { state: 'visible' });
      if (goalId) await page.waitForSelector(`[data-testid="supervisor-goal"][data-goal-id="${goalId}"]`, { state: 'visible' });
    };
    const goalOn = (surface, goalId) => surface.page.locator(`[data-testid="supervisor-goal"][data-goal-id="${goalId}"]`);
    const shownGoal = async (surface, goalId) => {
      const goal = goalOn(surface, goalId);
      return { runState: await goal.getAttribute('data-run-state'), revision: Number(await goal.getAttribute('data-revision')), id: (await goal.locator('[data-testid="supervisor-goal-id"]').innerText()).trim() };
    };
    const waitState = (surface, goalId, states) => surface.page.waitForFunction(({ id, wanted }) => {
      const element = document.querySelector(`[data-testid="supervisor-goal"][data-goal-id="${id}"]`);
      return element && wanted.includes(element.getAttribute('data-run-state'));
    }, { id: goalId, wanted: states }, { timeout: UI_DEADLINE_MS });
    const setTheme = async (surface, theme) => {
      await surface.page.emulateMedia({ colorScheme: theme });
      await surface.page.waitForFunction((wanted) => document.documentElement.dataset.theme === wanted, theme, { timeout: UI_DEADLINE_MS });
    };
    const capture = async (surface, label) => {
      const probe = await surface.page.evaluate(probeLayout, '[data-testid="supervisor-unified"]');
      const file = `screenshots/${surface.name}-${surface.layout.width}x${surface.layout.height}-${label}.png`;
      // Binary evidence: written directly, the text writers would redact bytes.
      writeFileSync(ctx.evidence.path(file), await surface.page.screenshot({ fullPage: false }));
      shots.push({ surface: surface.name, label, file, probe });
      return probe;
    };
    mkdirSync(ctx.evidence.path('screenshots'), { recursive: true });

    const desktop = await signIn('desktop');
    const phone = await signIn('phone');
    await openSupervisor(desktop);
    await openSupervisor(phone);
    const connections = await Promise.all([desktop, phone].map((surface) => surface.page.getAttribute('[data-testid="supervisor-connection"]', 'data-state')));
    ctx.assert('both-surfaces-reach-the-live-supervisor', connections.every((state) => state === 'live'), { connections });

    // 6. Submit through the wide layout's composer.
    await desktop.page.fill('[data-testid="supervisor-composer"]', FIXTURE_TEXT);
    await desktop.page.click('[data-testid="supervisor-submit"]');
    const receipt = desktop.page.locator('[data-testid="supervisor-receipt"]');
    await receipt.waitFor({ state: 'visible' });
    const shownReceipt = { outcome: await receipt.getAttribute('data-outcome'), goalId: await receipt.getAttribute('data-goal-id') };
    const goalId = shownReceipt.goalId;
    const request = goalId ? hub.one('SELECT source_adapter, goal_id, body FROM supervisor_requests WHERE goal_id = ?', goalId) : null;
    // Every gateway caller is admitted as the phone adapter (hub_runtime SupervisorCaller::Remote).
    ctx.assert('submit-receipt-names-the-backend-goal', shownReceipt.outcome === 'admitted' && typeof goalId === 'string'
      && request?.goal_id === goalId && request?.body === FIXTURE_TEXT && request?.source_adapter === 'phone', { shownReceipt, request });
    if (!goalId) throw new HarnessError('ADMISSION_FAILED', 'the composer submit showed no goal');
    transcript(`goal=${goalId}`);

    // 7. The dispatcher starts the fixture worker; both surfaces show running at the hub revision.
    await Promise.all([waitState(desktop, goalId, ['running']), waitState(phone, goalId, ['running'])]);
    const running = { hub: goalRow(goalId), daemon: await daemonGoal('snapshot-running', goalId), desktop: await shownGoal(desktop, goalId), phone: await shownGoal(phone, goalId) };
    const same = (state) => (view) => view.hub?.run_state === state && view.daemon?.runState === state && view.desktop.runState === state && view.phone.runState === state
      && view.desktop.revision === view.hub.revision && view.phone.revision === view.hub.revision && view.daemon.revision === view.hub.revision
      && view.desktop.id === goalId && view.phone.id === goalId;
    ctx.assert('running-same-goal-and-revision-everywhere', same('running')(running), running);

    // 8. Waiting: the owner adds an external-event dependency, the worker's turn ends (its pane
    //    reports idle), and the scheduler moves the goal to waiting_event by itself. Only the
    //    change notification can bring that state to the views.
    const dependFile = join(data, 'depend.json');
    writeFileSync(dependFile, JSON.stringify({ requestId: `qa-depend-${ctx.runId}`, projectId, goalId, expectedRevision: running.hub.revision, action: 'depend', payload: { eventKey: EVENT_KEY } }));
    const depended = await ctx.cli(['supervisor', 'request', '--json', dependFile], { label: 'owner-depend', env });
    const session = hub.one('SELECT provider_session_id FROM supervisor_execution_bindings WHERE goal_id = ?', goalId)?.provider_session_id ?? null;
    if (depended.code !== 0 || !session) throw new HarnessError('FIXTURE_FAILED', `depend or binding failed: ${JSON.stringify({ code: depended.code, json: depended.json, session })}`);
    await exchange('pane-settle', { type: 'write', sessionId: session, data: Buffer.from('QA-SETTLE\n').toString('base64') });
    await Promise.all([waitState(desktop, goalId, ['waiting_event']), waitState(phone, goalId, ['waiting_event'])]);
    const waiting = { hub: goalRow(goalId), daemon: await daemonGoal('snapshot-waiting', goalId), desktop: await shownGoal(desktop, goalId), phone: await shownGoal(phone, goalId) };
    const waitLines = await Promise.all([desktop, phone].map((surface) => goalOn(surface, goalId).locator('[data-testid="supervisor-goal-wait"]').innerText()));
    ctx.assert('waiting-reached-the-views-without-an-owner-action', same('waiting_event')(waiting) && waitLines.every((line) => line.includes(EVENT_KEY)), { waiting, waitLines });

    // 9. Readable at both widths, light and dark, while waiting.
    const waitingProbes = [];
    for (const theme of ['light', 'dark']) {
      for (const surface of [desktop, phone]) {
        await setTheme(surface, theme);
        waitingProbes.push({ surface: surface.name, theme, probe: await capture(surface, `waiting-${theme}`) });
      }
    }
    ctx.assert('waiting-view-readable-no-blank-no-overlap', waitingProbes.every(({ theme, probe }) => layoutOk(probe) && probe.theme === theme), waitingProbes);

    // 10. Pause on the phone; the wide layout and the daemon snapshot show the same paused revision.
    await goalOn(phone, goalId).locator('[data-testid="supervisor-pause"]').click();
    await Promise.all([waitState(phone, goalId, ['paused']), waitState(desktop, goalId, ['paused'])]);
    const paused = { hub: goalRow(goalId), daemon: await daemonGoal('snapshot-paused', goalId), desktop: await shownGoal(desktop, goalId), phone: await shownGoal(phone, goalId) };
    ctx.assert('phone-pause-seen-on-desktop-at-the-same-revision', same('paused')(paused) && paused.hub.revision === waiting.hub.revision + 1, paused);
    await capture(phone, 'paused-dark');
    await capture(desktop, 'paused-dark');

    // 11. Resume on the wide layout: the goal returns to the state the pause interrupted. Only an
    //     owner hold bumps the revision (transition_goal_in), so the resumed revision is the
    //     paused one.
    await goalOn(desktop, goalId).locator('[data-testid="supervisor-goal-resume"]').click();
    await Promise.all([waitState(desktop, goalId, ['waiting_event']), waitState(phone, goalId, ['waiting_event'])]);
    const resumed = { hub: goalRow(goalId), daemon: await daemonGoal('snapshot-resumed', goalId), desktop: await shownGoal(desktop, goalId), phone: await shownGoal(phone, goalId) };
    ctx.assert('desktop-resume-returns-live-on-both', same('waiting_event')(resumed) && resumed.hub.revision === paused.hub.revision, resumed);

    // 12. Disconnect: the daemon (recorded PID) stops. Both views keep the goal, say disconnected
    //     or unavailable, and stay readable in light and dark.
    await ctx.terminate(daemon, 'disconnect');
    const notLive = (surface) => surface.page.waitForFunction(() => {
      const banner = document.querySelector('[data-testid="supervisor-connection"]');
      return banner && banner.getAttribute('data-state') !== 'live';
    }, null, { timeout: 120_000 });
    await Promise.all([notLive(desktop), notLive(phone)]);
    const disconnectedProbes = [];
    for (const surface of [desktop, phone]) {
      for (const theme of ['light', 'dark']) {
        // No reload: the last view lives only in this page.
        await setTheme(surface, theme);
        const state = await surface.page.getAttribute('[data-testid="supervisor-connection"]', 'data-state');
        const kept = await goalOn(surface, goalId).count();
        const submitDisabled = await surface.page.isDisabled('[data-testid="supervisor-submit"]');
        disconnectedProbes.push({ surface: surface.name, theme, state, kept, submitDisabled, probe: await capture(surface, `disconnected-${theme}`) });
      }
    }
    ctx.assert('disconnect-keeps-the-goal-and-says-so', disconnectedProbes.every((entry) => entry.state !== 'live' && entry.kept === 1 && entry.submitDisabled && layoutOk(entry.probe)), disconnectedProbes);
    ctx.assert('no-page-errors', pageErrors.length === 0, pageErrors);

    ctx.evidence.writeJson('surfaces.json', {
      goalId, projectId, origin, email, receipt: shownReceipt, request, states: { running, waiting, paused, resumed }, screenshots: shots, pageErrors,
      worker: { kind: 'fixture', note: 'The pane runs fixture-worker.mjs: it reports working on its first line and idle on QA-SETTLE. It is not a real provider worker.' },
      desktopSurface: 'The 1440x900 surface is the remote web client wide layout; the native desktop window was not driven (MahoRD capture is not part of this runner). The desktop app reads the same daemon supervisorSnapshot recorded under states.*.daemon.',
    });
    for (const entry of readWorkerLogs(workerLogs)) ctx.evidence.writeText(`workers/${entry.name}`, entry.text);
  } catch (error) {
    // A failed UI wait leaves each open page as it was: record what a person would have seen.
    await captureFailure(ctx, surfaces, error, pageErrors);
    throw error;
  } finally {
    await browser.close().catch(() => {});
  }
}

/** Visible elements carrying a data-testid, with the attributes the scenario reads. */
function visibleTestIds() {
  return [...document.querySelectorAll('[data-testid]')].flatMap((element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    if (box.width === 0 || box.height === 0 || style.visibility === 'hidden' || style.display === 'none') return [];
    const data = Object.fromEntries(Object.entries(element.dataset).filter(([key]) => key !== 'testid'));
    const role = element.getAttribute('role');
    const label = element.getAttribute('aria-label');
    return [{ testid: element.dataset.testid, ...(role ? { role } : {}), ...(label ? { label } : {}), ...(Object.keys(data).length ? { data } : {}) }];
  });
}

/**
 * Writes `failure.json` and one full-page `screenshots/failure-<surface>.png` per open page: the
 * URL, title, visible testids, tabs and status text at the moment a UI step failed. Each page is
 * captured on its own; one page that cannot be read never hides the others or the original error.
 */
async function captureFailure(ctx, surfaces, error, pageErrors) {
  const pages = [];
  for (const { name, layout, page } of surfaces) {
    const entry = { surface: name, viewport: `${layout.width}x${layout.height}` };
    try {
      entry.url = page.url();
      entry.title = await page.title();
      entry.testids = await page.evaluate(visibleTestIds);
      entry.tabs = await page.evaluate(() => [...document.querySelectorAll('[role="tab"]')].map((tab) => ({
        name: tab.getAttribute('aria-label') ?? tab.textContent?.trim() ?? '', selected: tab.getAttribute('aria-selected'),
      })));
      entry.status = await page.evaluate(() => [...document.querySelectorAll('[role="status"],[role="alert"]')].map((node) => node.textContent?.trim() ?? ''));
      const file = `screenshots/failure-${name}-${layout.width}x${layout.height}.png`;
      mkdirSync(ctx.evidence.path('screenshots'), { recursive: true });
      // Binary evidence: written directly, the text writers would redact bytes.
      writeFileSync(ctx.evidence.path(file), await page.screenshot({ fullPage: true }));
      entry.screenshot = file;
    } catch (captureError) {
      entry.captureError = String(captureError?.message ?? captureError);
    }
    pages.push(entry);
  }
  ctx.evidence.writeJson('failure.json', { at: new Date().toISOString(), error: String(error?.stack ?? error), pages, pageErrors });
  ctx.log(`surfaces UI failure captured: failure.json, ${pages.filter((page) => page.screenshot).length} screenshot(s)`);
}

export const SURFACES_SCENARIO = Object.freeze({
  implemented: true,
  task: '14',
  summary: 'Real web client signed in to a self-hosted account at 1440x900 and 390x844, light and dark: composer submit names the backend goal, running and waiting_event reach both views at the hub revision, phone pause is seen on desktop, desktop resume returns live, a stopped daemon leaves the goal shown as disconnected',
  deadlineMs: 12 * 60_000,
  requiredAssertions: [
    'machine-enrolled-in-account', 'daemon-reachable-through-relay', 'both-surfaces-reach-the-live-supervisor', 'submit-receipt-names-the-backend-goal',
    'running-same-goal-and-revision-everywhere', 'waiting-reached-the-views-without-an-owner-action', 'waiting-view-readable-no-blank-no-overlap',
    'phone-pause-seen-on-desktop-at-the-same-revision', 'desktop-resume-returns-live-on-both', 'disconnect-keeps-the-goal-and-says-so', 'no-page-errors',
  ],
  notCovered: [
    { clause: 'native desktop window', reason: 'the native Ferryx window is captured with MahoRD, outside this runner', coveredBy: 'unrun: MahoRD native capture' },
    { clause: 'run state completed', reason: 'T10 completes a goal only through a Ferryx-runnable verifier graded by the retained mhc judge; the composer submits with the "owner review" verifier, which T10 holds as VERIFIER_NOT_RUNNABLE, and driving the judge needs the T6 provider stub this scenario does not start. The views render completed from the same snapshot field (data-run-state), and the completion paths announce the goal (supervisor/completion.rs)', coveredBy: 'autonomy scenario (task 10): goal-completed-without-owner; ui/src/lib/unifiedSupervisor.test.ts (completed goal offers no controls)' },
    { clause: 'typing and native-pane masks', reason: 'rendered from fixture snapshots, not from a live hub', coveredBy: 'ui/src/components/supervisor/Supervisor.test.tsx, ui/src/remote/home/SupervisorHome.test.tsx' },
  ],
  run: runSurfaces,
});
