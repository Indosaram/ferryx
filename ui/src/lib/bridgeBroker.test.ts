import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

// Executes the real isolated-world broker (src-tauri/src/browser/bridge/broker.js) under
// jsdom. The only rewrite is the inert `const hooks = null;` declaration, pointed at a
// per-test stub controlling the channel and event.isTrusted. Each test runs the broker
// against its own document, window event target and global object, so no state leaks.
const BROKER_PATH = resolve(__dirname, '../../../src-tauri/src/browser/bridge/broker.js');
const HOOK_DECL = 'const hooks = null;';

type Hooks = {
  channel: () => ((body: string) => void) | null;
  isTrusted: (event: Event) => boolean;
};

let posts: string[];
let channelPresent: boolean;
let trusted: boolean;
let doc: Document;
let fakeWindow: EventTarget & { top?: unknown };
let fakeGlobal: { __ferryxWorld?: unknown };

function loadBroker(): void {
  const source = readFileSync(BROKER_PATH, 'utf8');
  expect(source.split(HOOK_DECL)).toHaveLength(2);
  const instrumented = source.replace(HOOK_DECL, 'const hooks = __hooks;');
  const hooks: Hooks = {
    channel: () => (channelPresent ? (body: string) => posts.push(body) : null),
    isTrusted: () => trusted,
  };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const run = new Function('globalThis', 'window', 'document', '__hooks', instrumented);
  run(fakeGlobal, fakeWindow, doc, hooks);
}

function clickLink(attrs: Record<string, string>): MouseEvent {
  const anchor = doc.createElement('a');
  for (const [name, value] of Object.entries(attrs)) anchor.setAttribute(name, value);
  anchor.textContent = 'link';
  doc.body.appendChild(anchor);
  const event = new MouseEvent('click', { bubbles: true, cancelable: true });
  anchor.dispatchEvent(event);
  return event;
}

describe('isolated-world broker', () => {
  beforeEach(() => {
    posts = [];
    channelPresent = true;
    trusted = true;
    doc = document.implementation.createHTMLDocument('broker');
    // A top-level document: window.top is the window itself.
    fakeWindow = new EventTarget();
    fakeWindow.top = fakeWindow;
    fakeGlobal = {};
  });

  it('leaves a link click to native navigation when the channel is missing', () => {
    channelPresent = false;
    loadBroker();
    const event = clickLink({ href: 'https://example.com/next' });
    expect(event.defaultPrevented).toBe(false);
    expect(posts).toEqual([]);
  });

  it('leaves a download click to the native download when the channel is missing', () => {
    channelPresent = false;
    loadBroker();
    const event = clickLink({ href: 'https://example.com/file.zip', download: '' });
    expect(event.defaultPrevented).toBe(false);
    expect(posts).toEqual([]);
  });

  it('intercepts a trusted link click and posts exactly one envelope when the channel exists', () => {
    loadBroker();
    const event = clickLink({ href: 'https://example.com/next', target: '_blank' });
    expect(event.defaultPrevented).toBe(true);
    expect(posts).toHaveLength(1);
    expect(JSON.parse(posts[0])).toEqual({
      v: 1,
      kind: 'link',
      url: 'https://example.com/next',
      modifier: true,
    });
  });

  it('never intercepts an untrusted click', () => {
    trusted = false;
    loadBroker();
    const event = clickLink({ href: 'https://example.com/next' });
    expect(event.defaultPrevented).toBe(false);
    expect(posts).toEqual([]);
  });

  it('admits one popup per user activation', () => {
    loadBroker();
    fakeWindow.dispatchEvent(new MouseEvent('click'));
    const open = (handle: string) =>
      doc.dispatchEvent(
        new CustomEvent('ferryx-intent', {
          detail: JSON.stringify({ op: 'open', url: 'https://example.com/' + handle, handle }),
        }),
      );
    open('a');
    open('b');
    expect(posts.map((body) => JSON.parse(body).handle)).toEqual(['a']);
  });

  // WebView2 injects the broker (a CDP new-document script) into every frame, and the
  // binding is visible in every frame's ferryx-bridge world, but the host accepts posts only
  // from the main frame. A subframe broker must stay inert so in-frame clicks stay native.
  describe('in a subframe (window.top !== window)', () => {
    beforeEach(() => {
      fakeWindow.top = new EventTarget();
    });

    it('leaves a trusted link click to native navigation and posts nothing', () => {
      loadBroker();
      const event = clickLink({ href: 'https://example.com/next' });
      expect(event.defaultPrevented).toBe(false);
      expect(posts).toEqual([]);
    });

    it('leaves a trusted download click to the native download and posts nothing', () => {
      loadBroker();
      const event = clickLink({ href: 'https://example.com/file.zip', download: '' });
      expect(event.defaultPrevented).toBe(false);
      expect(posts).toEqual([]);
    });

    it('ignores ferryx-intent and installs no world api for the picker', () => {
      loadBroker();
      fakeWindow.dispatchEvent(new MouseEvent('click'));
      doc.dispatchEvent(
        new CustomEvent('ferryx-intent', {
          detail: JSON.stringify({ op: 'open', url: 'https://example.com/a', handle: 'a' }),
        }),
      );
      doc.dispatchEvent(new CustomEvent('ferryx-intent', { detail: JSON.stringify({ op: 'selfClose' }) }));
      expect(posts).toEqual([]);
      expect(fakeGlobal.__ferryxWorld).toBeUndefined();
    });

    it('stays inert when reading window.top throws', () => {
      Object.defineProperty(fakeWindow, 'top', {
        get() {
          throw new Error('SecurityError');
        },
      });
      loadBroker();
      const event = clickLink({ href: 'https://example.com/next' });
      expect(event.defaultPrevented).toBe(false);
      expect(posts).toEqual([]);
    });
  });

  it('still intercepts a trusted link click in the top frame and installs the world api', () => {
    loadBroker();
    const event = clickLink({ href: 'https://example.com/next' });
    expect(event.defaultPrevented).toBe(true);
    expect(posts.map((body) => JSON.parse(body))).toEqual([
      { v: 1, kind: 'link', url: 'https://example.com/next', modifier: false },
    ]);
    expect(fakeGlobal.__ferryxWorld).toBeDefined();
  });
});

// The production channel() detection, with no hooks: the WebView2 binding is the free
// global `__ferryxTrusted`, and the host disarms a downgraded document by deleting it and
// setting `__ferryxBridgeDisabled` in the world (Runtime.removeBinding alone leaves the
// function in place, so detection must not trust its mere presence).
describe('isolated-world broker channel detection (no hooks)', () => {
  type World = { __ferryxWorld?: { channel: () => unknown; post: (e: unknown) => boolean }; __ferryxBridgeDisabled?: boolean };
  const realGlobal = globalThis as unknown as { __ferryxTrusted?: unknown };
  let calls: string[];

  function loadRealBroker(world: World): NonNullable<World['__ferryxWorld']> {
    const source = readFileSync(BROKER_PATH, 'utf8');
    // eslint-disable-next-line @typescript-eslint/no-implied-eval
    const run = new Function('globalThis', 'window', 'document', source);
    const topWindow = new EventTarget() as EventTarget & { top?: unknown };
    topWindow.top = topWindow;
    run(world, topWindow, document.implementation.createHTMLDocument('broker'));
    const api = world.__ferryxWorld;
    if (!api) throw new Error('broker did not install');
    return api;
  }

  beforeEach(() => {
    calls = [];
    realGlobal.__ferryxTrusted = (body: string) => calls.push(body);
  });

  afterEach(() => {
    delete realGlobal.__ferryxTrusted;
  });

  it('finds the binding while it is a function', () => {
    const api = loadRealBroker({});
    expect(api.channel()).toBeTypeOf('function');
    expect(api.post({ v: 1, kind: 'popupSelfClose' })).toBe(true);
    expect(calls).toHaveLength(1);
  });

  it('reports no channel once the binding global is deleted', () => {
    const api = loadRealBroker({});
    delete realGlobal.__ferryxTrusted;
    expect(api.channel()).toBeNull();
    expect(api.post({ v: 1, kind: 'popupSelfClose' })).toBe(false);
  });

  it('reports no channel in a disarmed world even though the binding function survives', () => {
    const world: World = {};
    const api = loadRealBroker(world);
    world.__ferryxBridgeDisabled = true;
    expect(typeof realGlobal.__ferryxTrusted).toBe('function');
    expect(api.channel()).toBeNull();
    expect(api.post({ v: 1, kind: 'popupSelfClose' })).toBe(false);
    expect(calls).toEqual([]);
  });
});
