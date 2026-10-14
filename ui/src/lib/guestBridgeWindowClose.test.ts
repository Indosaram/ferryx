import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Executes the real page-world bridge template from src-tauri/src/browser/guest.rs. The
// host only substitutes placeholders, so this test performs the same substitution for a
// non-popup tab (the window.close body is read from guest.rs as well) and runs the result
// as sloppy-mode script, as the engines do, against a fake window, location and console.
const GUEST_PATH = resolve(__dirname, '../../../src-tauri/src/browser/guest.rs');

function between(source: string, start: string, end: string): string {
  const from = source.indexOf(start);
  expect(from).toBeGreaterThanOrEqual(0);
  const body = from + start.length;
  const to = source.indexOf(end, body);
  expect(to).toBeGreaterThan(body);
  return source.slice(body, to);
}

function nonPopupBridgeScript(): string {
  const source = readFileSync(GUEST_PATH, 'utf8');
  const template = between(source, 'const BRIDGE_SCRIPT_TEMPLATE: &str = r#"', '"#;');
  const closeBody = JSON.parse(between(source, 'const TAB_WINDOW_CLOSE_BODY: &str =\n    ', ';\n'));
  return template
    .replaceAll(PLACEHOLDERS[0], JSON.stringify('test-nonce'))
    .replaceAll(PLACEHOLDERS[1], '')
    .replaceAll(PLACEHOLDERS[2], closeBody)
    .replaceAll(PLACEHOLDERS[3], 'false')
    .replaceAll(PLACEHOLDERS[4], 'false');
}

const PLACEHOLDERS = [
  '__FERRYX_BROWSER_BRIDGE_NONCE__',
  '__FERRYX_IS_POPUP_SURFACE__',
  '__FERRYX_WINDOW_CLOSE_BODY__',
  '__FERRYX_BROWSER_BRIDGE_HARD_RELOAD__',
  '__FERRYX_BROWSER_BRIDGE_NATIVE_OPEN__',
];

type Ring = { level: string; text: string }[];
type FakeWindow = Record<string, unknown> & { __FERRYX_BROWSER_CONSOLE__?: Ring };

function runBridge(fakeWindow: FakeWindow): Ring {
  const script = nonPopupBridgeScript();
  for (const placeholder of PLACEHOLDERS) expect(script).not.toContain(placeholder);
  const fakeLocation = { href: 'https://example.com/', assign: () => {} };
  const fakeConsole = { log: () => {}, info: () => {}, warn: () => {}, error: () => {} };
  const doc = document.implementation.createHTMLDocument('guest');
  fakeWindow.open = () => null;
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const run = new Function('window', 'location', 'document', 'console', script);
  run(fakeWindow, fakeLocation, doc, fakeConsole);
  return fakeWindow.__FERRYX_BROWSER_CONSOLE__ ?? [];
}

const closeFailures = (ring: Ring) =>
  ring.filter((entry) => entry.text.startsWith('[ferryx] could not replace window.close'));

describe('guest bridge window.close replacement', () => {
  it('installs its own close and reports nothing when the page left close alone', () => {
    const fakeWindow: FakeWindow = { close: () => {} };
    const ring = runBridge(fakeWindow);
    expect(Object.getOwnPropertyDescriptor(fakeWindow, 'close')?.configurable).toBe(false);
    expect(String(fakeWindow.close)).toContain('function close()');
    expect(closeFailures(ring)).toEqual([]);
  });

  // Late install through webview.eval: page script already locked close down, so
  // defineProperty throws and the sloppy-mode fallback assignment silently does nothing.
  it('reports a failure when the fallback assignment silently leaves the page close in place', () => {
    const pageClose = () => 'page';
    const fakeWindow: FakeWindow = {};
    Object.defineProperty(fakeWindow, 'close', { value: pageClose, writable: false, configurable: false });
    const ring = runBridge(fakeWindow);
    expect(fakeWindow.close).toBe(pageClose);
    expect(closeFailures(ring)).toHaveLength(1);
  });

  it('reports a failure when the fallback assignment throws', () => {
    const fakeWindow: FakeWindow = {};
    Object.defineProperty(fakeWindow, 'close', {
      get: () => () => 'page',
      set: () => {
        throw new Error('refused');
      },
      configurable: false,
    });
    const ring = runBridge(fakeWindow);
    expect(closeFailures(ring)).toHaveLength(1);
  });
});
