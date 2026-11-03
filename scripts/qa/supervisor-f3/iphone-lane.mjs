// iPhone lane of the F3 acceptance run: Safari on the owner's real iPhone, driven through the
// provisioned WebDriverAgent by the existing producer's seam (herdr-reference-device.mjs
// bindIosDevice: driver status, session and device provenance are read, never invented). Every
// action is a real touch from the accessibility tree or the page rect; page state is only READ.
//
// Push (F3-P4) needs Web Push permission for the served origin, which iOS grants only to a Home
// Screen web app the owner allowed. The lane checks the gateway's own subscription list for a
// decision-enabled subscription of THIS device; absent, P4 is blocked PUSH_PERMISSION_ABSENT
// with the exact one-time owner action, never passed. Other steps still run.
import { join } from 'node:path';
import { bindIosDevice } from '../herdr-reference-device.mjs';
import { LaneBlocked, StepFailed, markNotRun, runStep, until } from './steps.mjs';
import { scenariosForLane } from './contract.mjs';

// Read-only page probe: the decision cards, their buttons and rects, the Stop buttons and the
// visual viewport. The gesture area is that viewport minus a fixed 44 pt top (status bar) and
// 34 pt bottom (home indicator); no CSS safe-area variable is read.
const PANEL_EXPR = `(() => {
  const rect = (el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height, bottom: r.bottom, right: r.right }; };
  const vv = window.visualViewport;
  const cards = [...document.querySelectorAll('[data-testid="decision-card"]')].map((card) => ({
    label: card.getAttribute('aria-label'), rect: rect(card), text: (card.innerText || '').slice(0, 2000),
    buttons: [...card.querySelectorAll('button[data-primary]')].map((b) => ({ text: b.innerText.trim(), primary: b.dataset.primary === 'true', disabled: b.disabled, rect: rect(b) })),
  }));
  const stops = [...document.querySelectorAll('[data-testid="handling-row"] button[aria-label^="Stop "]')].map((b) => ({ label: b.getAttribute('aria-label'), rect: rect(b) }));
  return { url: location.href, home: !!document.querySelector('[data-testid="phone-supervisor-home"]'), cards, stops,
    viewport: { w: innerWidth, h: innerHeight, vvTop: vv ? vv.offsetTop : 0, vvH: vv ? vv.height : innerHeight }, at: Date.now() };
})()`;
const TOP_PT = 44;
const BOTTOM_PT = 34;
const MIN_TARGET_PT = 44;

/** Inside the gesture area: fully within the visual viewport, clear of the top bar and home indicator. */
const inGestureArea = (r, page) => r.y >= page.viewport.vvTop + TOP_PT && r.bottom <= page.viewport.vvTop + page.viewport.vvH - BOTTOM_PT && r.x >= 0 && r.right <= page.viewport.w;
// The fixture pane classifies each received chunk; only these are answers (D4 report writes,
// F3-DRAW and typed text are `report:*`, `draw` and `input`).
const isAnswer = (entry) => entry.kind === 'received' && (entry.classified === 'approve' || entry.classified === 'reject' || /^option\d$/.test(entry.classified ?? ''));

export async function runIphoneLane(lane, { device, run, shared }) {
  const ids = scenariosForLane('iphone');
  let ios;
  try {
    try {
      ios = await bindIosDevice({ serial: device.serial, driverEndpoint: device.driverEndpoint, model: device.model, osVersion: device.osVersion, evidenceDir: join(lane.dir, 'wda') });
    } catch (error) {
      // bindIosDevice deletes a WDA session the probe opened before rethrowing; keep that proof.
      lane.receipt.cleanup.push(...(error.cleanup ?? []).map((r) => ({ ...r, action: r.action === 'deleted' ? 'session-deleted' : r.action })));
      throw new LaneBlocked(error.reason === 'device-owner-acknowledgment-needed' ? 'DEVICE_CONSENT_NEEDED' : 'DEVICE_UNAVAILABLE', `${device.serial} via ${device.driverEndpoint}: ${error.message}`);
    }
    lane.receipt.machine = { id: device.serial, model: ios.device.model, osVersion: ios.device.osVersion, driverEndpoint: device.driverEndpoint, session: ios.session };
    // The phone runs no checkout: it loads the UI the session host serves from the candidate
    // build, so its candidate is the session host's, checked by the served build stamp.
    lane.receipt.candidate = await shared.servedCandidate();
    lane.write('identity', 'identity.json', { machine: lane.receipt.machine, candidate: lane.receipt.candidate });

    await ios.wda.navigate(shared.phoneUrl);
    const read = async () => ios.page.evaluate(PANEL_EXPR);
    // Sign in the way a person does: email typed through native key events, the emailed link
    // (read from the run-owned relay's file mailer) opened in the same Safari tab.
    const signedIn = await ios.page.evaluate(`!!document.querySelector('[data-testid="phone-supervisor-home"]')`);
    if (!signedIn) {
      const tree = await ios.hierarchy();
      const field = await ios.pageTapPoint(ios.page, tree.nodes, { hint: 'email', cdpRectExpr: ios.rectExpr('[data-testid="account-email-input"]') });
      await ios.tap('email', field.x, field.y);
      await ios.wda.keys([...shared.ownerEmail]);
      // One mail section (shared with desktop enrolment): snapshot, tap send, wait for the one new link.
      const link = await shared.signInPhone(async () => {
        const send = await ios.pageTapPoint(ios.page, tree.nodes, { hint: 'request-link', cdpRectExpr: ios.rectExpr('[data-testid="request-magic-link-btn"]') });
        await ios.tap('request-link', send.x, send.y);
      });
      await ios.wda.navigate(link);
    }
    const capture = async (step, name) => {
      const png = join(lane.dir, 'tmp', `${name}.png`);
      await ios.screenshot(png);
      step.artifact('screenshot', `${name}.png`, (await import('node:fs')).readFileSync(png));
      const tree = await ios.hierarchy();
      step.artifact('accessibility-source', `${name}.xml`, tree.xml);
      const page = await read();
      step.artifact('panel-read', `${name}.page.json`, page);
      return { tree, page };
    };
    const tapRect = async (label, r, nodes) => {
      const point = await ios.pageTapPoint(ios.page, nodes, { hint: label, cdpRectExpr: `({ x: ${r.x}, y: ${r.y}, w: ${r.w}, h: ${r.h} })` });
      await ios.tap(label, point.x, point.y);
      return point;
    };
    await until('phone supervisor home with a decision card', read, (p) => p?.home && p.cards.length > 0, { deadlineMs: 120_000 });
    // Answering changes the decision card (next nonce); hold until both desktops recorded the
    // same card, prompt and Reply, so same-card-both-desktops compares one card.
    await shared.desktopsSawCard;

    // One tap per action, the card inside the gesture area, the fixture pane receives exactly
    // the answer and the card for that prompt leaves (the next prompt is a new card).
    const answer = async (step, kind) => {
      const before = await capture(step, `${kind}-before`);
      // The card's command line carries the fixture title (prod.example.com/<title><nonce>).
      const card = before.page.cards.find((c) => c.text.includes(`prod.example.com/${shared.decisionTitle}`));
      if (!card) throw new StepFailed('CARD_NOT_SHOWN', `no decision card for ${shared.decisionTitle}`);
      step.assert('card-in-gesture-area', inGestureArea(card.rect, before.page), { card: card.rect, viewport: before.page.viewport });
      const button = kind === 'approve' ? card.buttons.find((b) => b.primary) : card.buttons.find((b) => /^no\b/i.test(b.text));
      if (!button) throw new StepFailed('OPTION_NOT_SHOWN', `${kind} option missing: ${JSON.stringify(card.buttons)}`);
      step.assert('button-in-gesture-area', inGestureArea(button.rect, before.page), { button: button.rect });
      step.assert('touch-target-44pt', button.rect.h >= MIN_TARGET_PT && button.rect.w >= MIN_TARGET_PT, { w: button.rect.w, h: button.rect.h });
      const mark = await shared.agentLogCursor('decision');
      const point = await tapRect(`${kind}:${button.text}`, button.rect, before.tree.nodes);
      const answered = await until(`fixture pane receives ${kind}`, () => shared.agentLog('decision', mark), (entries) => entries.some((e) => e.kind === 'answered'), { deadlineMs: 60_000 });
      step.artifact('fixture-log', `${kind}.agent.jsonl`, answered.map((e) => JSON.stringify(e)).join('\n'));
      const received = answered.filter(isAnswer);
      step.assert('one-tap-one-answer', received.length === 1 && received[0].classified === kind, { received, point });
      const after = await until('answered card leaves', read, (p) => !p.cards.some((c) => c.text === card.text), { deadlineMs: 60_000 });
      step.assert('answered-card-left', !after.cards.some((c) => c.text === card.text), { cards: after.cards.map((c) => c.text.slice(0, 200)) });
      await capture(step, `${kind}-after`);
    };
    await runStep(lane, 'F3-P1', (step) => answer(step, 'approve'));
    await runStep(lane, 'F3-P2', (step) => answer(step, 'reject'));

    await runStep(lane, 'F3-P3', async (step) => {
      const before = await until('Stop on the upstream session with a card visible', read, (p) => p.stops.length > 0 && p.cards.length > 0, { deadlineMs: 180_000 });
      const shot = await capture(step, 'stop-before');
      const stop = before.stops.find((s) => s.label.includes(shared.upstreamTitle));
      if (!stop) throw new StepFailed('STOP_NOT_SHOWN', `no Stop for ${shared.upstreamTitle}: ${JSON.stringify(before.stops)}`);
      step.assert('card-visible-while-stopping', shot.page.cards.some((c) => inGestureArea(c.rect, shot.page)), { cards: shot.page.cards.map((c) => c.rect) });
      step.assert('stop-touch-target-44pt', stop.rect.h >= MIN_TARGET_PT && stop.rect.w >= MIN_TARGET_PT, stop.rect);
      const mark = await shared.agentLogCursor('upstream');
      await tapRect(stop.label, stop.rect, shot.tree.nodes);
      const log = await until('upstream session stopped', () => shared.agentLog('upstream', mark), (entries) => entries.some((e) => e.kind === 'stopped'), { deadlineMs: 60_000 });
      step.artifact('fixture-log', 'stop.agent.jsonl', log.map((e) => JSON.stringify(e)).join('\n'));
      step.assert('one-tap-stopped-session', log.filter((e) => e.kind === 'received' && e.classified === 'stop').length === 1, { log });
      await capture(step, 'stop-after');
    });

    // P4 surface: the Home Screen web app (iOS delivers Web Push only to one), which the
    // notification tap opens. WDA cannot script that web app's page, so the card is read from
    // the accessibility tree. Permission is the owner's one-time action; the delivery itself is
    // proven on the device (the banner) and in the Windows daemon's own push store, which still
    // holds the subscription after delivery (push.rs drops an endpoint the push service gone'd).
    await runStep(lane, 'F3-P4', async (step) => {
      // The run's daemon profile is new, so its push store starts empty: the owner enables Phone
      // alerts in the Home Screen web app during the run, within `pushPermissionWaitMs`.
      lane.log(`F3-P4 waiting ${device.pushPermissionWaitMs ?? 0} ms for the owner to enable Phone alerts for ${shared.phoneOrigin}`);
      const subs = await until('a decision push subscription', shared.pushSubscriptions, (list) => list.some((s) => s.needsInput === true), { deadlineMs: device.pushPermissionWaitMs ?? 0, gapMs: 5_000 }).catch(() => shared.pushSubscriptions());
      step.artifact('push-subscription', 'subscriptions-before.json', subs);
      // push.rs PushKind::Decision.wanted_by: a subscription wants decision pushes iff needsInput.
      // The store belongs to this run's isolated daemon, so any entry was made in this run.
      const mine = subs.filter((s) => s.needsInput === true);
      if (mine.length === 0) {
        throw new LaneBlocked('PUSH_PERMISSION_ABSENT', `no decision push subscription in this run's push store; on the iPhone open the Home Screen "Ferryx" web app for ${shared.phoneOrigin}, sign in, Settings > Phone alerts > on, Allow, then re-run`);
      }
      const banners = (tree) => tree.nodes.filter((n) => /ferryx/i.test(`${n.desc} ${n.text}`) && /needs you|decision|approve/i.test(`${n.desc} ${n.text}`));
      // In the web app the card is read from the accessibility tree; its command text is a
      // static text node under the card, so the fixture title in the command identifies it.
      const label = (title) => (tree) => tree.nodes.some((n) => `${n.desc} ${n.text}`.includes(`prod.example.com/${title}`));
      const before = await ios.hierarchy();
      step.artifact('accessibility-source', 'push-other-card.xml', before.xml);
      const shotBefore = join(lane.dir, 'tmp', 'push-other-card.png');
      await ios.screenshot(shotBefore);
      step.artifact('screenshot', 'push-other-card.png', (await import('node:fs')).readFileSync(shotBefore));
      step.assert('user-on-another-card', label(shared.decisionTitle)(before) && !label(shared.pushTitle)(before), { nodes: before.nodes.filter((n) => /prod\.example\.com/.test(`${n.desc} ${n.text}`)).map((n) => `${n.desc} ${n.text}`) });
      step.assert('banner-absent-before-release', banners(before).length === 0, { banners: banners(before) });
      const sent = Date.now();
      await shared.releaseHeldDecision();
      // The notification banner is system UI: found in the accessibility tree, tapped there.
      const banner = await until('decision push banner', ios.hierarchy, (tree) => banners(tree).length > 0, { deadlineMs: 90_000 });
      step.artifact('accessibility-source', 'push-banner.xml', banner.xml);
      const node = banners(banner)[0];
      step.assert('push-banner-shown', Boolean(node), { node, afterReleaseMs: Date.now() - sent });
      const png = join(lane.dir, 'tmp', 'push-banner.png');
      await ios.screenshot(png);
      step.artifact('screenshot', 'push-banner.png', (await import('node:fs')).readFileSync(png));
      await ios.tap('push-banner', node.cx, node.cy);
      const opened = await until('push opens the new card', ios.hierarchy, label(shared.pushTitle), { deadlineMs: 60_000 });
      step.artifact('accessibility-source', 'push-opened.xml', opened.xml);
      const card = opened.nodes.find((n) => `${n.desc} ${n.text}`.includes(`prod.example.com/${shared.pushTitle}`));
      const windowRect = ios.device.windowRect;
      step.artifact('panel-read', 'push-opened.cards.json', opened.nodes.filter((n) => /prod\.example\.com/.test(`${n.desc} ${n.text}`)));
      step.artifact('push-subscription', 'subscriptions-after.json', await shared.pushSubscriptions());
      step.assert('push-opened-new-card', Boolean(card), { card });
      // No window size from WDA means the gesture area cannot be proven: the assertion fails.
      const height = Number(windowRect?.height);
      step.assert('opened-card-in-gesture-area', Number.isFinite(height) && height > 0 && card.bounds.y1 >= TOP_PT && card.bounds.y2 <= height - BOTTOM_PT, { bounds: card.bounds, windowRect: windowRect ?? null });
      const shotAfter = join(lane.dir, 'tmp', 'push-after.png');
      await ios.screenshot(shotAfter);
      step.artifact('screenshot', 'push-after.png', (await import('node:fs')).readFileSync(shotAfter));
    });
  } catch (error) {
    if (error instanceof LaneBlocked) lane.receipt.blocked = { code: error.code, detail: error.detail, at: new Date().toISOString() };
    else lane.receipt.failure = { code: error.code ?? 'LANE_ERROR', detail: String(error.stack ?? error).slice(0, 4000) };
  } finally {
    markNotRun(lane, ids, lane.receipt.blocked?.code ?? 'lane stopped before this step');
    if (ios) lane.receipt.cleanup.push(...(await ios.cleanup()).map((r) => ({ ...r, action: r.action === 'deleted' ? 'session-deleted' : r.action })));
  }
}
