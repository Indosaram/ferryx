// No-GUI checks of the browser-agent harness: config/engine validation, isolated env, CLI
// runner, error-code parsing, window rect mapping, consent-card geometry and the evidence status
// rules. Run with `bun test scripts/qa/browser-agent/run.test.mjs` on a QA host (never on a build
// machine).
import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { deflateSync } from 'node:zlib';
import {
  MAX_FRAME_BYTES, SCENARIOS, browserEndpoint, cliErrorCode, createEvidence, evidenceLabel,
  isolatedEnv, loadConfig, parseArgs, receiptProvenance, resolveCandidate, runCli,
  validateSchema, windowRectFromClients, UsageError,
} from './run.mjs';
import {
  CARD_FILL, CARD_FILLS, CARD_LAYOUT, decodePng, locateConsentCard, measureActionButtons,
  pickCardButton, pickSiblingButton, tsvWords, confirmActionRow, looksLikeActionRow,
} from './gui.mjs';
import { createPipeSession } from './protocol.mjs';
import { callerControllerListed, dialogResolvedOnce, noUploadsRecorded } from './steps.mjs';
import { startFixtures, subscribeEvents } from './fixtures.mjs';

const root = mkdtempSync(path.join(tmpdir(), 'ferryx-browser-agent-qa-'));
after(() => rmSync(root, { recursive: true, force: true }));
let configId = 0;

/** A minimal 8-bit RGBA PNG, so card detection is exercised on real pixels, not a stub. */
function rgbaPng(width, height, colorAt) {
  const raw = Buffer.alloc(height * (width * 4 + 1));
  let p = 0;
  for (let y = 0; y < height; y += 1) {
    raw[p] = 0;
    p += 1;
    for (let x = 0; x < width; x += 1) {
      const color = colorAt(x, y);
      raw[p] = color.r; raw[p + 1] = color.g; raw[p + 2] = color.b; raw[p + 3] = 255;
      p += 4;
    }
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
    let crc = ~0;
    for (const byte of body) {
      crc ^= byte;
      for (let k = 0; k < 8; k += 1) crc = crc & 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1;
    }
    const out = Buffer.alloc(12 + data.length);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE((~crc) >>> 0, 8 + data.length);
    return out;
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * A screenshot with one consent card at `box`: a themed `bg-popover` panel on the themed canvas with
 * two action buttons in its lower band, laid out exactly as `BrowserAgentApprovalCard.tsx` does
 * (`justify-end gap-2`, 64x28 CSS px buttons scaled by `scale`).
 */
function cardFrame({ canvas, card, scale = 1, left = 60, top = 30, box = { width: 440, height: 160 } }) {
  const width = 1280;
  const height = 800;
  const buttonWidth = Math.round(CARD_LAYOUT.buttonWidth * scale);
  const buttonHeight = Math.round(CARD_LAYOUT.buttonHeight * scale);
  const gap = Math.round(8 * scale);
  const pad = Math.round(CARD_LAYOUT.padding * scale);
  const w = Math.round(box.width * scale);
  const h = Math.round(box.height * scale);
  const buttons = {
    allow: { left: left + w - pad - buttonWidth, right: left + w - pad },
    deny: { left: left + w - pad - 2 * buttonWidth - gap, right: left + w - pad - buttonWidth - gap },
  };
  const buttonTop = top + h - pad - buttonHeight;
  const BUTTON = { r: 0x22, g: 0x22, b: 0x22 };
  return rgbaPng(width, height, (x, y) => {
    const insideCard = x >= left && x < left + w && y >= top && y < top + h;
    if (!insideCard) return canvas;
    for (const button of Object.values(buttons)) {
      if (x >= button.left && x < button.right && y >= buttonTop && y < buttonTop + buttonHeight) return BUTTON;
    }
    return card;
  });
}

function config(extra = {}) {
  return {
    os: 'macos',
    binaries: { gui: ['/opt/ferryx/Ferryx'], cli: ['/opt/ferryx/ferryx'] },
    isolation: { runtimeRoot: path.join(root, 'run'), dataRoot: path.join(root, 'data'), profileRoot: path.join(root, 'profile') },
    mahord: { host: 'maho-mac', pairingId: '01234567-89ab-cdef-0123-456789abcdef' },
    remoteGateway: { url: 'http://127.0.0.1:1', bootstrapFile: path.join(root, 'bootstrap.json') },
    ...extra,
  };
}

function load(value) {
  const file = path.join(root, `config-${configId++}.json`);
  writeFileSync(file, JSON.stringify(value));
  return loadConfig(file);
}

test('the engine selects evidence dir and the Chromium engine dir', () => {
  const webview = load(config());
  assert.equal(evidenceLabel(webview), 'macos-webview');
  assert.equal(isolatedEnv(webview).FERRYX_CEF_ENGINE_DIR, undefined);
  assert.equal(isolatedEnv(webview).FERRYX_DATA_DIR, path.join(root, 'data'));

  const chromium = load(config({ engine: 'chromium', engineDir: '/opt/ferryx-engine' }));
  assert.equal(evidenceLabel(chromium), 'macos-chromium');
  assert.equal(isolatedEnv(chromium).FERRYX_CEF_ENGINE_DIR, '/opt/ferryx-engine');
});

test('an engine config without its dir, or with a stray dir or unknown engine, is refused', () => {
  assert.throws(() => load(config({ engine: 'chromium' })), UsageError);
  assert.throws(() => load(config({ engineDir: '/opt/ferryx-engine' })), UsageError);
  assert.throws(() => load(config({ engine: 'gecko' })), UsageError);
});

test('host env additions are isolated: they never override the QA roots', () => {
  const loaded = load(config({ env: { WEBKIT_DISABLE_DMABUF_RENDERER: '1' } }));
  const env = isolatedEnv(loaded);
  assert.equal(env.WEBKIT_DISABLE_DMABUF_RENDERER, '1');
  assert.equal(env.FERRYX_DATA_DIR, path.join(root, 'data'));
});

test('windowRect and env are the only allowed host extras; a typo is refused', () => {
  assert.throws(() => load(config({ windowRectangle: { left: 0, top: 0, right: 10, bottom: 10 } })), UsageError);
  const ok = load(config({ windowRect: { left: 0, top: 0, right: 10, bottom: 10 } }));
  assert.deepEqual(ok.windowRect, { left: 0, top: 0, right: 10, bottom: 10 });
});

test('runCli reports exit code, output and parsed JSON; a hung child is killed at the bound', async () => {
  const spawnStub = (code, stdout, stderr, never = false) => () => ({
    exited: never ? new Promise(() => {}) : Promise.resolve(code),
    stdout: new Response(stdout).body,
    stderr: new Response(stderr).body,
    kill() {},
  });
  const ok = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(0, '{"type":"acted"}\n', '') });
  assert.deepEqual([ok.code, ok.json], [0, { type: 'acted' }]);
  const stderr = JSON.stringify({ type: 'error', code: 'BROWSER_CLI_INVALID_OR_UNAVAILABLE', message: 'BROWSER_FORBIDDEN: no grant for this tab' });
  const refused = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(2, '', `waiting...\n${stderr}\n`) });
  assert.equal(refused.code, 2);
  assert.equal(cliErrorCode(refused), 'BROWSER_FORBIDDEN');
  const local = JSON.stringify({ type: 'error', code: 'BROWSER_CLI_INVALID_OR_UNAVAILABLE', message: 'missing required option --ref' });
  assert.equal(cliErrorCode({ stderr: local }), 'BROWSER_CLI_INVALID_OR_UNAVAILABLE');
  assert.equal(cliErrorCode({ stderr: 'not json' }), null);
  const hung = await runCli('/bin/ferryx', ['list'], { spawn: spawnStub(0, '', '', true), timeoutMs: 20 });
  assert.equal(hung.code, null);
});

test('the consent card button position is optional and must be whole screen pixels', () => {
  assert.deepEqual(load(config({ consentCard: { approve: { x: 720, y: 96 } } })).consentCard.approve, { x: 720, y: 96 });
  assert.throws(() => load(config({ consentCard: { approve: { x: -1, y: 96 } } })), UsageError);
  assert.throws(() => load(config({ consentCard: {} })), UsageError);
});

test('a host may name its tesseract, and an absent one is simply not configured', () => {
  assert.equal(load(config({ tesseract: '/opt/homebrew/bin/tesseract' })).tesseract, '/opt/homebrew/bin/tesseract');
  // Relative paths are refused: the harness would run it under a different cwd.
  assert.throws(() => load(config({ tesseract: 'tesseract' })), UsageError);
  assert.equal(load(config()).tesseract, undefined);
});

test('scenario selection accepts single scenarios, lists and all', () => {
  const file = path.join(root, 'args.json');
  writeFileSync(file, JSON.stringify(config()));
  assert.deepEqual(parseArgs(['--config', file, '--scenario', 'q1,q3', '--evidence', root]).scenarios, ['Q1', 'Q3']);
  assert.deepEqual(parseArgs(['--config', file, '--scenario', 'all', '--evidence', root]).scenarios, [...SCENARIOS]);
  assert.throws(() => parseArgs(['--config', file, '--scenario', 'Q10', '--evidence', root]), UsageError);
  assert.throws(() => parseArgs(['--config', file, '--scenario', 'all']), UsageError);
});

test('a window rect is only derived from a mapped client with a usable geometry', () => {
  assert.deepEqual(
    windowRectFromClients([{ pid: 42, mapped: true, at: [10, 20], size: [300, 200] }], 42),
    { left: 10, top: 20, right: 310, bottom: 220, width: 300, height: 200 },
  );
  assert.equal(windowRectFromClients([{ pid: 42, mapped: false, at: [0, 0], size: [1, 1] }], 42), null);
  assert.equal(windowRectFromClients([], 42), null);
  assert.equal(windowRectFromClients([{ pid: 42, mapped: true, at: [0, 0], size: [0, 100] }], 42), null);
});

test('the first existing candidate wins and a missing one is null', () => {
  const present = path.join(root, 'present.bin');
  writeFileSync(present, 'x');
  assert.equal(resolveCandidate([path.join(root, 'absent.bin'), present]), present);
  assert.equal(resolveCandidate([path.join(root, 'absent.bin')]), null);
  assert.equal(resolveCandidate([null, present]), present);
  assert.equal(MAX_FRAME_BYTES, 1024 * 1024);
});

test('preflight uses the Windows port file and the Unix domain socket', () => {
  const windows = config({ os: 'windows' });
  const linux = config({ os: 'linux' });
  const runtime = linux.isolation.runtimeRoot;
  assert.equal(browserEndpoint(windows, 'linux'), path.join(runtime, 'browser.port'));
  assert.equal(browserEndpoint(linux, 'win32'), path.join(runtime, 'browser.port'));
  assert.equal(browserEndpoint(linux, 'linux'), path.join(runtime, 'browser.sock'));
  assert.equal(browserEndpoint(config(), 'darwin'), path.join(runtime, 'browser.sock'));
});

test('Q4 requires exactly the calling controller rather than an empty or duplicate list', () => {
  assert.equal(callerControllerListed([{ id: 'qa-b' }], 'qa-b'), true);
  assert.equal(callerControllerListed(['qa-b'], 'qa-b'), true);
  for (const controllers of [undefined, null, {}, [], [{ id: 'qa-a' }], [{ id: 'qa-b' }, { id: 'qa-b' }], ['qa-a', 'qa-b']]) {
    assert.equal(callerControllerListed(controllers, 'qa-b'), false);
  }
});

test('Q6 requires exactly one accepted dialog resolution', () => {
  assert.equal(dialogResolvedOnce([{ result: true }]), true);
  for (const dialogs of [undefined, null, {}, [], [{}], [{ result: false }], [{ result: true }, { result: true }], [{ result: true }, { result: false }]]) {
    assert.equal(dialogResolvedOnce(dialogs), false);
  }
});

test('Q7 requires an existing empty upload ledger', () => {
  assert.equal(noUploadsRecorded([]), true);
  for (const uploads of [undefined, null, {}, { length: 0 }, [{ filename: 'unexpected.txt' }]]) {
    assert.equal(noUploadsRecorded(uploads), false);
  }
});

test('Q5 distinguishes stale references from unrelated CLI failures', () => {
  const refused = (code) => ({ code: 1, stderr: JSON.stringify({ type: 'error', code }) });
  assert.equal(cliErrorCode(refused('BROWSER_STALE_REF')), 'BROWSER_STALE_REF');
  assert.notEqual(cliErrorCode(refused('BROWSER_FORBIDDEN')), 'BROWSER_STALE_REF');
  assert.notEqual(cliErrorCode({ code: 1, stderr: 'connection failed' }), 'BROWSER_STALE_REF');
  assert.notEqual(cliErrorCode({ code: 0, json: { applied: false } }), 'BROWSER_STALE_REF');
});

test('fixture SSE reports replacement and dialog events without timing sleeps', async () => {
  const fixtures = await startFixtures();
  let events;
  try {
    events = await subscribeEvents(fixtures.origins.a);
    for (const [type, fields, kind] of [
      ['control-ready', {}, 'page:control-ready'],
      ['node-replaced', { reason: 'control', gen: 1 }, 'page:node-replaced'],
      ['dialog', { result: true }, 'dialog'],
    ]) {
      const [record, response] = await Promise.all([
        events.waitFor((entry) => entry.kind === kind, { timeoutMs: 2_000 }),
        fetch(`${fixtures.origins.a}/__event`, {
          method: 'POST', headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ type, ...fields }),
        }),
      ]);
      assert.equal(response.status, 200);
      assert.equal(record.kind, kind);
      for (const [key, value] of Object.entries(fields)) assert.equal(record[key], value);
    }
    assert.equal(dialogResolvedOnce(fixtures.state().dialogs), true);
  } finally {
    events?.close();
    await fixtures.stop();
  }
});

test('Q6 decoder errors survive valid unsolicited frames and EOF', () => {
  const session = createPipeSession({ write() {} });
  const refusal = `${JSON.stringify({ id: null, ok: false, error: { code: 'PIPE_FRAME_MALFORMED' } })}\n`;
  session.feed(refusal);
  assert.equal(session.errors.length, 0);
  session.feed(`not JSON\n${refusal}`);
  assert.equal(session.unsolicited.length, 2);
  assert.deepEqual(session.errors.map((error) => error.code), ['MALFORMED_FRAME']);
  session.feed('{"incomplete":');
  session.close();
  assert.deepEqual(session.errors.map((error) => error.code), ['MALFORMED_FRAME', 'UNTERMINATED_FRAME']);
});

test('receipts persist the explicit source SHA and hashes of both candidate executables', async () => {
  const cli = path.join(root, 'receipt-cli.bin');
  const gui = path.join(root, 'receipt-gui.bin');
  writeFileSync(cli, 'abc');
  writeFileSync(gui, 'hello');
  const provenance = await receiptProvenance({ cli, gui }, {
    env: { FERRYX_QA_SHA: ` ${'a'.repeat(40)}\n` }, repo: path.join(root, 'not-a-repo'),
  });
  assert.deepEqual(provenance, {
    sha: 'a'.repeat(40),
    cliSha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    guiSha256: '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824',
  });
  const evidence = createEvidence(root, 'receipt-test', 'Q1');
  evidence.assert('fixture', true);
  assert.equal(evidence.finish(provenance), 'PASS');
  const receipt = JSON.parse(readFileSync(path.join(evidence.dir, 'receipt.json'), 'utf8'));
  assert.deepEqual([receipt.sha, receipt.cliSha256, receipt.guiSha256], [provenance.sha, provenance.cliSha256, provenance.guiSha256]);
});

test('receipt provenance resolves git HEAD when no source SHA override is set', async () => {
  const expected = Bun.spawnSync(['git', '-C', import.meta.dir, 'rev-parse', 'HEAD'], { stdout: 'pipe', stderr: 'pipe' });
  assert.equal(expected.exitCode, 0);
  const provenance = await receiptProvenance({ cli: null, gui: null }, { env: { ...process.env, FERRYX_QA_SHA: '' } });
  assert.deepEqual(provenance, { sha: expected.stdout.toString().trim(), cliSha256: null, guiSha256: null });
});

test('receipt provenance refuses invalid or unresolved SHAs and unreadable candidates', async () => {
  const binaries = { cli: null, gui: null };
  await assert.rejects(receiptProvenance(binaries, { env: { FERRYX_QA_SHA: 'not-a-sha' } }), /full git commit hash/);
  await assert.rejects(receiptProvenance(binaries, { env: { ...process.env, FERRYX_QA_SHA: '' }, repo: root }), /cannot resolve receipt SHA/);
  await assert.rejects(receiptProvenance({ cli: path.join(root, 'missing-candidate'), gui: null }, { env: { FERRYX_QA_SHA: 'a'.repeat(40) } }), /ENOENT/);
});

test('schema validation reports the path of a bad value', () => {
  const errors = validateSchema({ os: 'linux' }, { type: 'object', required: ['os', 'engine'] });
  assert.deepEqual(errors, ['$: missing required engine']);
});

test('a refusal the daemon could not correlate still settles its request', async () => {
  // The pipe refuses an oversized frame on size alone, before it has read an id, so the response
  // carries `id: null`. The client only matched responses by id, so the caller waited out its whole
  // deadline and reported a transport timeout for an immediate typed refusal - Q6's real failure on
  // omarchy. A request sent with allowOversize owns that refusal; a raw malformed line owns nothing.
  const written = [];
  const session = createPipeSession({ write: (bytes) => written.push(bytes), timeoutMs: 2_000 });
  const oversize = session.send({ op: 'list', pad: 'x'.repeat(64) }, { allowOversize: true });
  // Decode what the client wrote so the test replies in the real wire shape.
  assert.equal(JSON.parse(new TextDecoder().decode(written[0]).trim()).op, 'list');
  session.feed(new TextEncoder().encode(`${JSON.stringify({ id: null, ok: false, error: { code: 'PIPE_FRAME_TOO_LARGE', message: 'frame exceeds the maximum' } })}\n`));
  const refusal = await oversize;
  assert.equal(refusal.error.code, 'PIPE_FRAME_TOO_LARGE');

  // A malformed line has no request behind it, so its refusal must stay unsolicited.
  session.sendRaw('{not json\n');
  session.feed(new TextEncoder().encode(`${JSON.stringify({ id: null, ok: false, error: { code: 'PIPE_FRAME_MALFORMED', message: 'frame is not valid JSON' } })}\n`));
  assert.equal(session.unsolicited.at(-1)?.frame?.error?.code, 'PIPE_FRAME_MALFORMED');
  await session.close();
});

test('the consent card is found in every theme the app can render', () => {
  // The card is `bg-popover`, and `--popover` is themed: the shipping dark `#2e323b`, the settings
  // dark `#333333` and the settings light `#ffffff` are all real cards. A live Windows run had its
  // light-theme card on screen and no consent step could find it, because only the dark fill was
  // known. Every theme must be located, with both buttons measured inside it.
  for (const fill of CARD_FILLS) {
    const image = decodePng(cardFrame({ canvas: { r: 0x23, g: 0x26, b: 0x2d }, card: fill }));
    const card = locateConsentCard(image, { tolerance: 12 });
    assert.ok(card, `the ${fill.name} card (rgb ${fill.r},${fill.g},${fill.b}) must be located`);
    assert.equal(card.fill, fill.name);
    assert.ok(card.buttons.allow, `the ${fill.name} card must expose its Allow button`);
    assert.ok(card.buttons.deny, `the ${fill.name} card must expose its Deny button`);
    // The primary is the right-most control (`justify-end`), and it is never the card's own fill.
    assert.ok(card.buttons.allow.x > card.buttons.deny.x, 'Allow sits right of Deny');
    assert.notDeepEqual([card.buttons.allow.x, card.buttons.allow.y], [card.left, card.top]);
  }

  // The card is 440 CSS px and a screenshot is device pixels. On maho-win at a 236% scale the real
  // card measured 1040 px wide, which the old 1.8x cap (792 px) rejected while it was plainly on
  // screen, so a high-DPI window must still be found.
  const hiDpi = decodePng(cardFrame({ canvas: { r: 0x23, g: 0x26, b: 0x2d }, card: CARD_FILL, scale: 2.36, box: { width: 440, height: 126 } }));
  const scaled = locateConsentCard(hiDpi, { tolerance: 12 });
  assert.ok(scaled, 'a 236%-scale card (1040 px wide) must be located');
  assert.ok(scaled.width > 1000 && scaled.width < 1100, `expected ~1040 px, saw ${scaled?.width}`);

  // A same-fill box with no action row is a page or a blank panel, never the card: clicking its
  // middle would dismiss whatever is really on screen.
  const blank = decodePng(rgbaPng(1280, 800, (x, y) => (x >= 60 && x < 500 && y >= 30 && y < 190 ? { r: 0x2e, g: 0x32, b: 0x3b } : { r: 0x23, g: 0x26, b: 0x2d })));
  assert.equal(locateConsentCard(blank, { tolerance: 12 }), null);
});

test('a page element below the card does not stretch it', () => {
  // On omarchy the screenshot-permission card sits on a settings page whose tab strip shares the
  // popover grey 64 rows lower and overlaps its span. Merging by horizontal overlap alone absorbed
  // that strip, so the card measured 687 px wide and 211 tall, its "Approve" point landed on the
  // page beside the button, and the one-time grant expired unanswered. The panel is the card only:
  // its rows are contiguous and its columns are filled for most of those rows.
  const width = 900;
  const height = 420;
  const frame = decodePng(rgbaPng(width, height, (x, y) => {
    const inCard = x >= 60 && x < 570 && y >= 20 && y < 160;
    const inStrip = x >= 60 && x < 570 && y >= 224 && y < 300;
    const inStrayText = x >= 600 && x < 700 && y >= 60 && y < 100;
    if (inCard) {
      // Text and two buttons, so the panel's own rows are not uniformly solid.
      const text = y >= 40 && y < 52 && x >= 80 && x < 400;
      const deny = x >= 430 && x < 490 && y >= 120 && y < 150;
      const allow = x >= 500 && x < 560 && y >= 120 && y < 150;
      return text || deny || allow ? { r: 0xdc, g: 0xdc, b: 0xdc } : { r: 0x2e, g: 0x32, b: 0x3b };
    }
    if (inStrip || inStrayText) return { r: 0x2e, g: 0x32, b: 0x3b };
    return { r: 0x18, g: 0x1b, b: 0x21 };
  }));
  const card = locateConsentCard(frame, { tolerance: 12 });
  assert.notEqual(card, null);
  assert.deepEqual({ left: card.left, top: card.top, right: card.right, bottom: card.bottom }, { left: 60, top: 20, right: 569, bottom: 159 });
  assert.ok(card.buttons.allow.left >= 500 && card.buttons.allow.right <= 560, JSON.stringify(card.buttons));
  assert.ok(card.buttons.allow.y >= 120 && card.buttons.allow.y < 150, JSON.stringify(card.buttons));
});

test('a same-fill desktop band is not mistaken for the card', () => {
  // On omarchy the settings surface's popover grey also covers a wallpaper band, and the measured
  // "Allow" there was 10 px tall with its "Deny" 94 px - the click went to the desktop and the
  // pairing request expired unanswered. Two runs only make an action row when they are the same
  // height, tall enough to be a control, and one button-gap apart.
  const box = { width: 689, height: 156 };
  const real = {
    deny: { left: 1488, right: 1658, top: 104, bottom: 197, x: 1573, y: 151 },
    allow: { left: 1746, right: 1801, top: 148, bottom: 157, x: 1774, y: 153 },
  };
  assert.equal(looksLikeActionRow(real, box), false);
  // The genuine card on the same screen, one band lower.
  const genuine = { deny: { left: 2028, right: 2085, top: 198, bottom: 230 }, allow: { left: 2097, right: 2161, top: 198, bottom: 230 } };
  assert.equal(looksLikeActionRow(genuine, { width: 520, height: 146 }), true);
  // Ordering matters: `justify-end` puts the primary last.
  assert.equal(looksLikeActionRow({ deny: genuine.allow, allow: genuine.deny }, { width: 520, height: 146 }), false);
  assert.equal(looksLikeActionRow({ deny: genuine.deny, allow: null }, { width: 520, height: 146 }), false);
});

test('the measured button is the button, not the description above it', () => {
  // Both captured frames had their click too high: `measureActionButtons` unioned every inked row of
  // a button column, and the card's description inks the same columns. On omarchy the Allow button
  // is rows 198-230 but the union ran 157-231, so the click landed in the card's padding and the
  // request expired unapproved. The action row is the lowest band, because `justify-end` puts it last.
  const card = { left: 200, right: 900, top: 100, bottom: 300, width: 700, height: 200 };
  const image = decodePng(rgbaPng(1000, 400, (x, y) => {
    if (x < card.left || x > card.right || y < card.top || y > card.bottom) return { r: 0x23, g: 0x26, b: 0x2d };
    // One word of the card's description, sitting directly above the Allow button's own columns.
    if (y >= 182 && y <= 196 && x >= 800 && x <= 866 && (x % 4) < 3) return { r: 0xa0, g: 0xa0, b: 0xa0 };
    // Deny (outline) and Allow (filled): the measured runs, not the description, decide the rows.
    if (x >= 700 && x <= 782 && y >= 244 && y <= 282) return { r: 0x50, g: 0x55, b: 0x60 };
    if (x >= 792 && x <= 874 && y >= 244 && y <= 282) return { r: 0xe5, g: 0xe5, b: 0xe5 };
    return { r: 0x2e, g: 0x32, b: 0x3b };
  }));
  const buttons = measureActionButtons(image, card, { fill: CARD_FILL });
  assert.ok(buttons.allow && buttons.deny, 'both buttons must be measured');
  // The click has to be inside the button itself, not in the description line above it.
  assert.ok(buttons.allow.top >= 244, `Allow must start at its own top (244), saw ${buttons.allow.top}`);
  assert.ok(buttons.allow.y >= 250 && buttons.allow.y <= 280, `Allow centre must be in the button, saw ${buttons.allow.y}`);
  assert.ok(buttons.deny.top >= 244, `Deny must start at its own top (244), saw ${buttons.deny.top}`);
  assert.ok(buttons.allow.x > buttons.deny.x, 'the filled primary is the right-most control');
});

test('the card image is read as words and the click is chosen only on positive evidence', () => {
  // tesseract TSV, as `--psm 6`/`--psm 11 -c tessedit_create_tsv=1` prints it. The title opens with
  // the primary's own verb, which is exactly what made a topmost-word click land on the title.
  // tesseract TSV has twelve columns: level, page, block, par, line, word, left, top, width,
  // height, conf, text.
  const row = (block, par, line, word, left, top, width, height, text, conf = 90) =>
    [5, 1, block, par, line, word, left, top, width, height, conf, text].join('\t');
  const tsv = [
    'level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext',
    row(1, 1, 1, 1, 100, 60, 70, 20, 'Allow'),
    row(1, 1, 1, 2, 180, 60, 90, 20, '"qa-a"'),
    row(1, 1, 1, 3, 280, 60, 60, 20, 'to'),
    row(1, 1, 1, 4, 350, 60, 90, 20, 'browser?'),
    row(1, 1, 1, 5, 300, 200, 60, 24, 'Deny'),
    row(1, 1, 1, 6, 380, 200, 70, 24, 'Allow'),
    '',
  ].join('\n');
  const words = tsvWords(tsv);
  assert.equal(words.length, 6);
  assert.deepEqual(words[0], { text: 'Allow', left: 100, top: 60, width: 70, height: 20, conf: 90 });

  const box = { left: 0, top: 0, right: 600, bottom: 260, width: 600, height: 260 };
  // The real button: `Deny` sits immediately left of it on the same row (tier 2, the best evidence).
  const picked = pickCardButton(words, 'Allow', { box });
  assert.deepEqual(picked.at, { x: 415, y: 212 });
  assert.equal(picked.tier, 2);
  assert.match(picked.via, /right of Deny/);
  assert.deepEqual(pickSiblingButton(words, picked, { box }), { x: 330, y: 212, text: 'Deny' });

  // A lone label below the title with nothing beside it is only tier 1, never the title itself.
  const lonely = pickCardButton(words.filter((word) => word.text !== 'Deny'), 'Allow', { box });
  assert.equal(lonely.tier, 1);
  assert.deepEqual(lonely.at, { x: 415, y: 212 });

  // With the title line known, the title's own "Allow" is rejected outright - this is the click
  // that landed at the title on the Windows audit and let the request expire unapproved.
  const titleOnly = pickCardButton([{ text: 'Allow', left: 100, top: 60, width: 70, height: 20 }], 'Allow', { box });
  assert.equal(titleOnly.at, null);
  assert.equal(titleOnly.via, 'none');
  assert.match(titleOnly.rejected[0].why, /title line/);

  // "Approve" is the same button on a grant card; no label at all yields no click.
  assert.equal(pickCardButton([{ text: 'Approve', left: 380, top: 200, width: 80, height: 24 }], 'Approve', { box }).tier, 0);
  assert.equal(pickCardButton([], 'Allow', { box }).at, null);
});

test('OCR corroborates the measured action row and never vetoes a card that is there', () => {
  // On both real frames tesseract read the secondary reliably (`Deny`) and the primary badly
  // (`Allew`, `Aulgw`, `Ausw`), because the primary is a small filled button. So the geometry comes
  // from the measured pixels and OCR only has to place a known secondary verb on that same row; a
  // garbled primary must not turn a visible card into a timeout.
  const buttons = {
    deny: { left: 4049, right: 4172, top: 399, bottom: 458, x: 4111, y: 429 },
    allow: { left: 4193, right: 4324, top: 388, bottom: 462, x: 4259, y: 425 },
  };
  const box = { left: 3315, right: 4355, top: 195, bottom: 493, width: 1040, height: 298 };
  const deny = { text: 'Deny', left: 4079, top: 405, width: 60, height: 26 };
  const garbled = { text: 'Aulsw', left: 4224, top: 400, width: 66, height: 26 };
  const confirmed = confirmActionRow([deny, garbled], buttons, { box });
  assert.deepEqual(confirmed, { confirmed: true, sibling: 'Deny', top: 418 });

  // The description line above the row is not on it, so it corroborates nothing.
  const description = { text: 'Deny', left: 4079, top: 250, width: 60, height: 26 };
  assert.equal(confirmActionRow([description], buttons, { box }).confirmed, false);
  // Nothing measured means there is no row to corroborate, whatever the words say.
  assert.equal(confirmActionRow([deny], { deny: null, allow: null }, { box }).confirmed, false);
});
