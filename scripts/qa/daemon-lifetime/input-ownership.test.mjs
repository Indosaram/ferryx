// The desktop-input ownership contract: no key, chord, click or modifier key-up leaves the
// harness unless gui.focus proved this recorded QA window holds the foreground.
//
// Measured on the private runtime candidate af645748 (run surv-mac-1117n / surv-mac-1117m):
// gui.mjs focus(mac) reported focused:true even when its osascript activation failed, so every
// later key went to whichever application really was frontmost; Windows and Linux answered
// focused:false and the walk still sent Escape, Enter and text. These tests pin the guard that
// closes both halves - zero input on an unconfirmed focus, and the product's normal interaction
// unchanged on a confirmed one - using fake input only. No daemon, GUI, host or real desktop is
// touched, and no clock is consulted: every fake resolves immediately.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createGuiDriver, ownsForeground, clickReach } from './lib/gui.mjs';
import { isolationLayout } from './lib/guards.mjs';
import {
  confirmInputOwnership, openSettings, summarizeFocus, OWNERSHIP_UNCONFIRMED,
  waitRestoredAttach, dismissSetupWizard, guiTempDir, SETTINGS_OPENED_RECEIPT,
} from './scenarios/q10-q13.mjs';

const RUN = '20261019t064512z';
const PANE = `q11-pane-${RUN}`;

/** A gui.focus answer the harness must treat as ownership. */
const OWNS = { focused: true, pid: 10, rect: { left: 100, top: 50, right: 1100, bottom: 850 } };

test('only an explicit focused:true is ownership; refusals, skips and malformed answers are not', () => {
  assert.equal(ownsForeground(OWNS), true);
  // The measured mac defect: activation asked, never proved.
  assert.equal(ownsForeground({ focused: false, reason: 'mac activation did not make this pid the frontmost process' }), false);
  assert.equal(ownsForeground({ focused: false }), false);
  // A platform that needs no focus step did not therefore take the foreground.
  assert.equal(ownsForeground({ skipped: 'linux' }), false);
  // The Linux chord refusal the driver now returns.
  assert.equal(ownsForeground({ refused: 'focus-not-verified', focused: false }), false);
  // Absent, null and malformed answers never authorise input.
  assert.equal(ownsForeground(undefined), false);
  assert.equal(ownsForeground(null), false);
  assert.equal(ownsForeground({}), false);
  assert.equal(ownsForeground({ focused: 'true' }), false, 'the string "true" is not proof');
  assert.equal(ownsForeground({ focused: 1 }), false);
  assert.equal(ownsForeground('focused'), false);
  // A driver that reports success but also declares the step refused is refused.
  assert.equal(ownsForeground({ focused: true, refused: 'focus-not-verified' }), false);
});

test('confirmInputOwnership never throws and records a failed or malformed focus as unowned', async () => {
  const mk = (impl) => ({ gui: { focus: impl } });
  assert.equal((await confirmInputOwnership(mk(async () => OWNS), { pid: 10 }, 'x')).owned, true);
  const failed = await confirmInputOwnership(mk(async () => ({ focused: false, reason: 'no' })), { pid: 10 }, 'x');
  assert.deepEqual([failed.owned, failed.error, failed.at], [false, null, 'x']);
  const threw = await confirmInputOwnership(mk(async () => { throw new Error('osascript exited 1'); }), { pid: 10 }, 'x');
  assert.deepEqual([threw.owned, threw.focus, threw.error], [false, null, 'osascript exited 1']);
  const absent = await confirmInputOwnership(mk(async () => undefined), { pid: 10 }, 'x');
  assert.equal(absent.owned, false);
});

test('summarizeFocus keeps the bounded diagnostic fields and drops raw child buffers', () => {
  const s = summarizeFocus({ focused: false, reason: 'r', activation: { status: 1 }, readBack: { pid: 4 }, rect: { left: 1 }, junk: 'x'.repeat(10) });
  assert.deepEqual(Object.keys(s).sort(), ['activation', 'focused', 'readBack', 'reason', 'rect']);
  assert.deepEqual(summarizeFocus(null), { value: null });
  assert.deepEqual(summarizeFocus('skipped'), { value: 'skipped' });
});

/** The mac driver: osascript activation + frontmost read-back + geometry, all through agent.exec. */
function macDriver({ activationStatus = 0, frontmostPid = 10, geometry = '100, 50, 1000, 800' } = {}) {
  const scripts = [];
  const agent = { call: async (method, params = {}) => {
    assert.equal(method, 'exec');
    const script = params.args?.at(-1) ?? '';
    scripts.push(script);
    if (script.includes('set frontmost')) return { status: activationStatus, stdout: '', stderr: activationStatus === 0 ? '' : 'System Events got an error: Application isn\'t running. (-600)' };
    if (script.includes('frontmost is true')) return { status: 0, stdout: `${frontmostPid}\n`, stderr: '' };
    return { status: 0, stdout: geometry, stderr: '' };
  } };
  const gui = createGuiDriver({ host: { platform: 'mac' }, agent, runId: RUN, layout: isolationLayout('/m', 'mac'), allowedRoots: ['/m'], forbidden: [] });
  return { gui, scripts };
}

test('mac focus proves the foreground by reading the frontmost pid back, not by having asked', async () => {
  const ok = macDriver();
  const r = await ok.gui.focus({ pid: 10 });
  assert.equal(r.focused, true);
  assert.deepEqual([r.pid, r.frontPid], [10, 10]);
  assert.deepEqual(r.rect, { left: 100, top: 50, right: 1100, bottom: 850 });
  assert.ok(ok.scripts.some((s) => s.includes('frontmost is true')), 'the frontmost pid is read back');

  // The measured defect: the activation fails and the window still reports focus.
  const failed = macDriver({ activationStatus: 1, frontmostPid: 10 });
  const f = await failed.gui.focus({ pid: 10 });
  assert.deepEqual([f.focused, f.reason], [false, 'mac activation did not make this pid the frontmost process']);
  assert.equal(f.activation.status, 1);
  assert.match(f.activation.stderr, /-600/);

  // The activation answers ok but another application is frontmost: still not ours.
  const other = macDriver({ frontmostPid: 4242 });
  const o = await other.gui.focus({ pid: 10 });
  assert.deepEqual([o.focused, o.readBack.pid], [false, 4242]);

  // An unparseable read-back is not a success either.
  const junk = macDriver({ frontmostPid: 'x' });
  const j = await junk.gui.focus({ pid: 10 });
  assert.deepEqual([j.focused, j.readBack.pid], [false, null]);
});

test('mac focus keeps the window rect for the pane click even when ownership is refused', async () => {
  const { gui } = macDriver({ activationStatus: 1 });
  const r = await gui.focus({ pid: 10 });
  // The rect is evidence, not authorisation: it is still reported so a refusal can record where.
  assert.deepEqual(r.rect, { left: 100, top: 50, right: 1100, bottom: 850 });
  assert.equal(ownsForeground(r), false);
});

// ---- a global coordinate click needs a provable hit, not just a rect ----

const LINUX_WS = 1;
const client = (pid, { at = [0, 0], size = [800, 600], mapped = true, ws = LINUX_WS, cls = 'ferryx' } = {}) => ({
  pid, mapped, class: cls, workspace: { id: ws }, at, size,
});

test('clickReach allows a click only where exactly one visible client contains the point and it is ours', () => {
  const point = { x: 400, y: 12 };
  // Alone on its workspace: the hit is unambiguous.
  assert.equal(clickReach([client(10)], 10, point).ok, true);
  // An occluder that covers the point: refused, and the covering window is named.
  const occluded = clickReach([client(10), client(77, { cls: 'ferryx-other' })], 10, point);
  assert.deepEqual([occluded.ok, occluded.reason], [false, 'another window covers the click point']);
  // `covering` is every visible client containing the point, so the overlap names our own window and
  // the foreign one; the hit is ambiguous, which is why the click is refused. The same list is what
  // the Linux focus test asserts as [10, 77], so both describe one contract.
  assert.deepEqual(occluded.covering, [
    { pid: 10, class: 'ferryx', title: null },
    { pid: 77, class: 'ferryx-other', title: null },
  ]);
  // An occluder on another workspace does not receive the click, so it does not block it.
  assert.equal(clickReach([client(10), client(77, { ws: 2 })], 10, point).ok, true);
  // A point outside the recorded window is refused even when alone.
  assert.equal(clickReach([client(10)], 10, { x: 5000, y: 12 }).ok, false);
  // No mapped client, or no usable geometry, is a refusal.
  assert.equal(clickReach([client(10, { mapped: false })], 10, point).ok, false);
  assert.equal(clickReach([client(10, { at: null })], 10, point).ok, false);
  assert.equal(clickReach([], 10, point).ok, false);
});

test('Linux focus refuses the click, with zero global input, when a foreign window covers the target', async () => {
  const clicks = [];
  const agent = { call: async (method, params = {}) => {
    if (method === 'waitExit') return { pid: params.pid, exitedAt: 1 };
    const cmd = params.args?.at(-1) ?? '';
    if (cmd.includes('hyprctl instances')) return { status: 0, stdout: 'instance hypr-1\n', stderr: '' };
    // Both windows sit on the same workspace and cover the same title-bar point; neither is focused.
    return { status: 0, stdout: JSON.stringify([client(10, { size: [800, 600] }), client(77, { cls: 'foreign' })]), stderr: '' };
  } };
  const hotkeys = [];
  const mahord = { click: async (x, y) => { clicks.push([x, y]); }, hotkey: async (k) => { hotkeys.push(k); } };
  const gui = createGuiDriver({ host: { platform: 'linux' }, agent, runId: RUN, layout: isolationLayout('/l', 'linux'), allowedRoots: ['/l'], forbidden: [], mahord });
  const r = await gui.focus({ pid: 10 });
  assert.equal(r.focused, false);
  assert.match(r.reason, /focus click withheld: another window covers the click point/);
  assert.deepEqual(r.covering.map((c) => c.pid), [10, 77], 'both the target and the occluder are named');
  assert.deepEqual(clicks, [], 'no global click was posted through a possibly overlapping window');
  // Quit inherits the refusal: SUPER+Q is never sent.
  const before = clicks.length;
  const q = await gui.quit({ tag: 'g1', pid: 10 });
  assert.deepEqual([q.refused, hotkeys], ['focus-not-verified', []]);
  assert.equal(clicks.length, before, 'still no click');
});

test('Linux focus still clicks and confirms when the target is the only client at the point', async () => {
  const clicks = [];
  let focused = false;
  const agent = { call: async (method, params = {}) => {
    assert.equal(method, 'exec');
    const cmd = params.args?.at(-1) ?? '';
    if (cmd.includes('hyprctl instances')) return { status: 0, stdout: 'instance hypr-1\n', stderr: '' };
    const win = client(10);
    win.focusHistoryID = focused ? 0 : 1;
    return { status: 0, stdout: JSON.stringify([win]), stderr: '' };
  } };
  const mahord = { click: async (x, y) => { clicks.push([x, y]); focused = true; }, hotkey: async () => {} };
  const gui = createGuiDriver({ host: { platform: 'linux' }, agent, runId: RUN, layout: isolationLayout('/l', 'linux'), allowedRoots: ['/l'], forbidden: [], mahord });
  const r = await gui.focus({ pid: 10 });
  assert.deepEqual([r.focused, r.clicked], [true, { x: 400, y: 12 }]);
  assert.deepEqual(clicks, [[400, 12]], 'the owned path still delivers its title-bar click');
});

/**
 * A Linux world. `geometry: false` is a mapped window the compositor reports without a rect, so the
 * driver declines at once instead of polling; that is the unowned case. With geometry, the window
 * reaches focusHistoryID 0 only after focus's own title-bar click, which is the owned case.
 */
function linuxDriver({ geometry = true, focusAfterClick = true } = {}) {
  const clicks = [];
  const listed = 'instance hypr-1\n';
  const agent = { call: async (method, params = {}) => {
    if (method === 'waitExit') return { pid: params.pid, exitedAt: 1 };
    assert.equal(method, 'exec');
    const cmd = params.args?.at(-1) ?? '';
    if (cmd.includes('hyprctl instances')) return { status: 0, stdout: listed, stderr: '' };
    const focused = clicks.length > 0 && focusAfterClick;
    const win = { pid: 10, mapped: true, focusHistoryID: focused ? 0 : 1 };
    if (geometry) { win.at = [0, 0]; win.size = [800, 600]; }
    return { status: 0, stdout: JSON.stringify([win]), stderr: '' };
  } };
  const hotkeys = [];
  const mahord = { click: async (x, y) => { clicks.push([x, y]); }, hotkey: async (keys) => { hotkeys.push(keys); } };
  const gui = createGuiDriver({ host: { platform: 'linux' }, agent, runId: RUN, layout: isolationLayout('/l', 'linux'), allowedRoots: ['/l'], forbidden: [], mahord });
  return { gui, clicks, hotkeys };
}

test('Linux quit sends no close chord when the focus was never confirmed', async () => {
  const { gui, clicks, hotkeys } = linuxDriver({ geometry: false });
  const r = await gui.quit({ tag: 'g1', pid: 10 });
  assert.equal(r.refused, 'focus-not-verified');
  assert.deepEqual([r.focused, r.timedOut], [false, undefined], 'a refusal returns at once, with no exit waiter');
  assert.deepEqual(hotkeys, [], 'SUPER+Q is never sent to a desktop this harness does not own');
  assert.deepEqual(clicks, [], 'not even the focus click was sent: there was no rect to aim it at');
  // The refusal is explicit and carries the driver's own answer.
  assert.equal(ownsForeground(r), false);
  assert.equal(r.focus.reason, 'mapped window has no geometry');
});

test('Linux quit still sends the close chord, and returns the exit, on a confirmed focus', async () => {
  const { gui, hotkeys } = linuxDriver({ focusAfterClick: true });
  const r = await gui.quit({ tag: 'g1', pid: 10 });
  assert.deepEqual(hotkeys, [['Meta', 'q']]);
  assert.equal(r.refused, undefined);
  assert.equal(r.focus.focused, true);
  assert.equal(r.pid, 10, 'the normal exit receipt is preserved');
});

// ---- Q10: the settings walk sends nothing without ownership ----

/** A Q10 world whose GUI never proves the foreground; every input call is recorded. */
function q10World({ focus = async () => ({ focused: false, reason: 'not frontmost' }), markerWritten = true } = {}) {
  const input = [];
  const log = [];
  const ctx = {
    platform: 'mac',
    mahord: {
      key: async (k) => { input.push(['key', k]); },
      hotkey: async (keys) => { input.push(['hotkey', keys.join('+')]); if (markerWritten) log.push(`x ${SETTINGS_OPENED_RECEIPT} supported=true`); },
      screenshot: async () => Buffer.from('png'),
    },
    agent: { call: async (method, params = {}) => {
      if (method === 'readFile') return log.join('\n');
      if (method === 'waitLog') {
        const r = new RegExp(params.pattern).exec(log.join('\n'));
        return r ? { matched: r[0] } : { matched: null, timedOut: true };
      }
      if (method === 'waitPath') return true;
      throw new Error(`unexpected agent call ${method}`);
    } },
    gui: { focus },
  };
  return { ctx, input };
}

test('Q10 openSettings presses no chord when the gate refuses, and says why', async () => {
  const { ctx, input } = q10World();
  const gate = async () => confirmInputOwnership(ctx, { pid: 10 }, 'settings-open');
  const r = await openSettings(ctx, '/l/gui.log', gate);
  assert.deepEqual([r.opened, r.refused], [false, OWNERSHIP_UNCONFIRMED]);
  assert.equal(r.guard.owned, false);
  assert.deepEqual(r.attempts, [{ before: null, opened: false, refused: OWNERSHIP_UNCONFIRMED, guard: r.guard }]);
  assert.deepEqual(input, [], 'no settings chord reached the desktop');
});

test('Q10 openSettings keeps its receipt-driven retry on a confirmed gate', async () => {
  const { ctx, input } = q10World({ focus: async () => ({ focused: true, pid: 10 }) });
  const gate = async () => confirmInputOwnership(ctx, { pid: 10 }, 'settings-open');
  const r = await openSettings(ctx, '/l/gui.log', gate);
  assert.deepEqual([r.opened, r.attempts.length], [true, 1]);
  assert.deepEqual(input, [['hotkey', 'Meta+,']], 'one press, after the gate');
});

// ---- Q11: Escape, the pane click and the typing ride on the same proof ----

/** A Q11-shaped world for the two attach/wizard paths, with fake events and fake input. */
function attachWorld({ focus, attachAfterEscape = true, wizardEvent = false } = {}) {
  const input = [];
  const events = [];
  const ctx = {
    platform: wizardEvent ? 'windows' : 'linux',
    runId: RUN,
    layout: isolationLayout('/r', wizardEvent ? 'windows' : 'linux'),
    gui: { focus },
    mahord: { key: async (k) => { input.push(['key', k]); if (k === 'Escape' && attachAfterEscape) events.push(JSON.stringify({ kind: 'attached', attachTuple: { backendSessionId: 'SSH1' } })); } },
    agent: { call: async (method, params = {}) => {
      assert.equal(method, 'waitLog');
      const text = params.file.endsWith('ferryx-switch-debug.jsonl') ? '' : events.join('\n');
      const r = new RegExp(params.pattern, params.flags ?? '').exec(text);
      return r ? { matched: r[0] } : { matched: null, timedOut: true };
    } },
  };
  return { ctx, input, events };
}

test('the attach Escape is withheld when the GUI does not own the foreground', async () => {
  const { ctx, input } = attachWorld({ focus: async () => ({ focused: false, reason: 'not frontmost' }), attachAfterEscape: true });
  const r = await waitRestoredAttach(ctx, 'SSH1', { pid: 10 });
  assert.equal(r.escapeSent, false, 'no Escape was sent');
  assert.equal(r.refused, OWNERSHIP_UNCONFIRMED);
  assert.equal(r.focus.focused, false);
  assert.deepEqual(input, [], 'the desktop received no key');
  // The attach wait still runs, so a tab that attaches on its own is observed rather than failed.
  assert.equal(r.matched, null);
});

test('the attach Escape is still sent, once, on a confirmed focus', async () => {
  const { ctx, input } = attachWorld({ focus: async () => ({ focused: true, pid: 10 }) });
  const r = await waitRestoredAttach(ctx, 'SSH1', { pid: 10 });
  assert.deepEqual([r.escapeSent, r.refused], [true, undefined]);
  assert.deepEqual(input, [['key', 'Escape']]);
  assert.ok(r.matched, 'the attach that followed the Escape is reported');
});

test('the attach Escape is withheld when gui.focus throws, and the error is kept', async () => {
  const { ctx, input } = attachWorld({ focus: async () => { throw new Error('osascript exited 1'); } });
  const r = await waitRestoredAttach(ctx, 'SSH1', { pid: 10 });
  assert.deepEqual([r.escapeSent, r.focusError], [false, 'osascript exited 1']);
  assert.deepEqual(input, []);
});

/** A Windows Q11 wizard world: the pane's switch-debug log names the Welcome dialog, then closes. */
function wizardWorld({ focus, closes = true } = {}) {
  const input = [];
  let debug = [`{"event":"terminal.surface.skipped","details":{"localSessionId":"${PANE}","reason":"surfaceVisible=false yielding=1 which=dialog:Welcome to Ferryx"}}`];
  const ctx = {
    platform: 'windows',
    runId: RUN,
    layout: isolationLayout('/r', 'windows'),
    gui: { focus },
    mahord: { key: async (k) => {
      input.push(['key', k]);
      if (k === 'Escape' && closes) debug.push(`{"event":"terminal.surface.input.gate.state","details":{"paneIdentity":"${PANE}","visible":true}}`);
    } },
    agent: { call: async (method, params = {}) => {
      assert.equal(method, 'waitLog');
      const r = new RegExp(params.pattern, params.flags ?? '').exec(debug.join('\n'));
      return r ? { matched: r[0] } : { matched: null, timedOut: true };
    } },
  };
  return { ctx, input, tempDir: guiTempDir(ctx.layout) };
}

test('the wizard Escape is withheld when the GUI does not own the foreground', async () => {
  const { ctx, input } = wizardWorld({ focus: async () => ({ focused: false, reason: 'not frontmost' }) });
  const r = await dismissSetupWizard(ctx, { pid: 10 }, PANE);
  assert.deepEqual([r.expected, r.opened, r.closed, r.escapes], [true, true, false, 0]);
  assert.equal(r.refused, OWNERSHIP_UNCONFIRMED);
  assert.deepEqual(input, [], 'the key that would have closed the wizard never left the harness');
  assert.deepEqual(r.focus.map((f) => f.owned), [false], 'the refused round is recorded');
});

test('the wizard is still closed with the same bounded Escape on a confirmed focus', async () => {
  const { ctx, input } = wizardWorld({ focus: async () => ({ focused: true, pid: 10 }) });
  const r = await dismissSetupWizard(ctx, { pid: 10 }, PANE);
  assert.deepEqual([r.opened, r.closed, r.escapes, r.refused], [true, true, 1, undefined]);
  assert.deepEqual(input, [['key', 'Escape']]);
  assert.deepEqual(r.focus.map((f) => f.owned), [true]);
});

test('the driver gates mac/Q11 input on the same predicate the scenarios import', () => {
  // One predicate, not two: the scenarios and the driver must agree on what proof looks like, or a
  // future driver change silently re-opens the hole the scenario guard was closing.
  const gui = readFileSync(new URL('./lib/gui.mjs', import.meta.url), 'utf8');
  const scen = readFileSync(new URL('./scenarios/q10-q13.mjs', import.meta.url), 'utf8');
  assert.match(gui, /export function ownsForeground\(focus\)/);
  assert.match(scen, /import \{ ownsForeground \} from '\.\.\/lib\/gui\.mjs'/);
  // The Linux chord is behind the predicate; it is not sent before the check.
  const quit = gui.slice(gui.indexOf('async function quit(gui)'), gui.indexOf('/** Q02: SIGKILL'));
  assert.ok(quit.indexOf('ownsForeground(focus)') < quit.indexOf("mahord.hotkey(['Meta', 'q'])"), 'the chord follows the proof');
});
