import { afterEach, beforeAll, describe, expect, test } from 'bun:test';
import { CONSENT_TEMPLATE_ID, consentMarkup, initAnalytics } from './analyticsRuntime';
import { PLATFORMS } from './downloads';

// The download menu is a hydrated React island that unmounts its panel on the same click that
// follows the link. This exercises the genuine component against the genuine capture listener.
type JSDOMConstructor = typeof import('../../../ui/node_modules/jsdom/lib/api.js').JSDOM;

const MEASUREMENT_ID = 'G-TESTID1234';

let JSDOM: JSDOMConstructor;
let react: typeof import('react');
let reactDomClient: typeof import('react-dom/client');
let menuModule: typeof import('../components/DownloadMenu');
let cleanup: (() => void) | null = null;

beforeAll(async () => {
  const [jsdomModule, reactModule, clientModule, componentModule] = await Promise.all([
    import('../../../ui/node_modules/jsdom/lib/api.js'),
    import('react'),
    import('react-dom/client'),
    import('../components/DownloadMenu'),
  ]);
  JSDOM = jsdomModule.JSDOM;
  react = reactModule;
  reactDomClient = clientModule;
  menuModule = componentModule;
}, 180_000);

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function mountMenu(variant: 'hero' | 'navbar' | 'compact') {
  const dom = new JSDOM(
    `<!doctype html><html><head><template id="${CONSENT_TEMPLATE_ID}" data-measurement-id="${MEASUREMENT_ID}">` +
      consentMarkup({ privacyHref: '/privacy/' }) +
      `</template></head><body><div id="island"></div></body></html>`,
    { url: 'https://ferryx.dev/', pretendToBeVisual: true },
  );
  const win = dom.window;
  const doc = win.document;
  win.localStorage.setItem('ferryx.site.analytics-consent', 'granted');

  const globals = globalThis as unknown as Record<string, unknown>;
  const saved = new Map<string, unknown>();
  for (const key of ['window', 'document', 'navigator', 'HTMLElement', 'Node', 'MouseEvent', 'Event', 'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame']) {
    saved.set(key, globals[key]);
    globals[key] = (win as unknown as Record<string, unknown>)[key];
  }
  globals.IS_REACT_ACT_ENVIRONMENT = true;

  const { createElement, act } = react;
  const { createRoot } = reactDomClient;
  const { DownloadMenu } = menuModule;

  const root = createRoot(doc.getElementById('island')!);
  await act(async () => {
    root.render(createElement(DownloadMenu, { variant }));
  });
  initAnalytics(win);

  cleanup = () => {
    for (const [key, value] of saved) globals[key] = value;
    delete globals.IS_REACT_ACT_ENVIRONMENT;
    win.close();
  };

  const click = async (element: Element) => {
    await act(async () => {
      element.dispatchEvent(new win.MouseEvent('click', { bubbles: true, cancelable: true }));
    });
  };

  const downloadEvents = () =>
    ((win as unknown as { dataLayer?: IArguments[] }).dataLayer ?? [])
      .map((args) => [...args])
      .filter((args) => args[0] === 'event' && args[1] === 'download_click')
      .map((args) => args[2] as Record<string, unknown>);

  doc.addEventListener('click', (event) => event.preventDefault());
  return { win, doc, click, downloadEvents };
}

describe('download menu instrumentation', () => {
  test('the hero primary link reports its own surface', async () => {
    const { doc, click, downloadEvents } = await mountMenu('hero');
    const primary = doc.querySelector(`a[href="${PLATFORMS.macos.defaultAsset.url}"]`)!;
    expect(primary.closest('[data-ferryx-location]')?.getAttribute('data-ferryx-location')).toBe('hero_primary');
    await click(primary);
    expect(downloadEvents()).toEqual([
      {
        platform: 'macos',
        asset_id: 'macos-universal',
        destination: 'github_release_asset',
        link_location: 'hero_primary',
      },
    ]);
  });

  test('the navbar primary link reports its own surface', async () => {
    const { doc, click, downloadEvents } = await mountMenu('navbar');
    const primary = doc.querySelector(`a[href="${PLATFORMS.macos.defaultAsset.url}"]`)!;
    await click(primary);
    expect(downloadEvents()[0]).toMatchObject({ link_location: 'navbar_primary' });
  });

  test('a panel asset click survives the panel closing on the same click', async () => {
    const { doc, click, downloadEvents } = await mountMenu('compact');
    await click(doc.querySelector('button[aria-expanded]')!);
    const deb = doc.querySelector(`a[href="${PLATFORMS.linux.assets[1]!.url}"]`)!;
    expect(deb.closest('[data-ferryx-location]')?.getAttribute('data-ferryx-location')).toBe('download_menu');
    await click(deb);
    // The React onClick closed the menu, so the anchor is gone by the time the event settles.
    expect(doc.querySelector(`a[href="${PLATFORMS.linux.assets[1]!.url}"]`)).toBeNull();
    expect(downloadEvents()).toEqual([
      {
        platform: 'linux',
        asset_id: 'linux-deb',
        destination: 'github_release_asset',
        link_location: 'download_menu',
      },
    ]);
  });

  test('opening the releases listing from the panel is not a download', async () => {
    const { doc, click, downloadEvents } = await mountMenu('compact');
    await click(doc.querySelector('button[aria-expanded]')!);
    await click(doc.querySelector('a[href$="/releases/latest"]')!);
    expect(downloadEvents()).toEqual([]);
  });
});
