import { afterEach, beforeAll, beforeEach, describe, expect, test } from 'bun:test';
import { ANALYTICS_CONSENT_STORAGE_KEY } from './analytics';
import {
  CONSENT_HOST_ID,
  CONSENT_TEMPLATE_ID,
  consentMarkup,
  initAnalytics,
  mountConsentUi,
  startAnalytics,
} from './analyticsRuntime';
import { MICROSOFT_STORE_URL, PLATFORMS } from './downloads';

// jsdom is already vendored for the desktop UI test suite; the site adds no dependency for it.
// Loading it once keeps the DOM contract tests honest about real capture-phase delegation.
type JSDOMConstructor = typeof import('../../../ui/node_modules/jsdom/lib/api.js').JSDOM;
let JSDOM: JSDOMConstructor;

const MEASUREMENT_ID = 'G-TESTID1234';
const PAGE_URL = 'https://ferryx.dev/docs/introduction/?utm_source=news&email=a%40b.com';

beforeAll(async () => {
  const mod = await import('../../../ui/node_modules/jsdom/lib/api.js');
  JSDOM = mod.JSDOM;
}, 180_000);

interface Harness {
  win: Window & typeof globalThis & { close: () => void };
  doc: Document;
  host: HTMLElement;
  panel: HTMLElement;
  accept: HTMLButtonElement;
  decline: HTMLButtonElement;
  reopen: HTMLButtonElement;
  gtagCalls: () => unknown[][];
  gtagScripts: () => HTMLScriptElement[];
  networkRequests: string[];
  storageWrites: Array<[string, string]>;
}

let harness: Harness | null = null;

function mount(options: { measurementId?: string; stored?: string; url?: string } = {}): Harness {
  const measurementId = options.measurementId ?? MEASUREMENT_ID;
  const dom = new JSDOM(
    `<!doctype html><html><head></head><body><main></main>` +
      `<div id="${CONSENT_HOST_ID}" data-measurement-id="${measurementId}" hidden>` +
      consentMarkup({ privacyHref: '/privacy/' }) +
      `</div></body></html>`,
    { url: options.url ?? PAGE_URL, pretendToBeVisual: true },
  );
  const win = dom.window;
  const doc = win.document;

  const networkRequests: string[] = [];
  const storageWrites: Array<[string, string]> = [];
  const nativeSetItem = win.Storage.prototype.setItem;
  win.Storage.prototype.setItem = function patched(key: string, value: string) {
    storageWrites.push([key, value]);
    return nativeSetItem.call(this, key, value);
  };
  if (options.stored !== undefined) {
    win.localStorage.setItem(ANALYTICS_CONSENT_STORAGE_KEY, options.stored);
    storageWrites.length = 0;
  }
  // Any element that would hit the network records itself instead of fetching.
  const nativeAppend = win.HTMLHeadElement.prototype.appendChild;
  win.HTMLHeadElement.prototype.appendChild = function patched<T extends Node>(node: T): T {
    const src = (node as unknown as { src?: string }).src;
    if (src) networkRequests.push(src);
    return nativeAppend.call(this, node) as T;
  };
  doc.addEventListener('click', (event) => event.preventDefault());

  const host = doc.getElementById(CONSENT_HOST_ID) as HTMLElement;
  const built: Harness = {
    win,
    doc,
    host,
    panel: host.querySelector('[role="dialog"]') as HTMLElement,
    accept: host.querySelector('[data-consent-action="granted"]') as HTMLButtonElement,
    decline: host.querySelector('[data-consent-action="denied"]') as HTMLButtonElement,
    reopen: host.querySelector('[data-consent-action="reopen"]') as HTMLButtonElement,
    gtagCalls: () => ((win as unknown as { dataLayer?: unknown[][] }).dataLayer ?? []).map((args) => [...args]),
    gtagScripts: () =>
      [...doc.querySelectorAll('script')].filter((script) =>
        (script.getAttribute('src') ?? '').includes('googletagmanager.com'),
      ) as HTMLScriptElement[],
    networkRequests,
    storageWrites,
  };
  startAnalytics(win);
  harness = built;
  return built;
}

function click(element: HTMLElement) {
  element.dispatchEvent(new harness!.win.MouseEvent('click', { bubbles: true, cancelable: true }));
}

function calls(h: Harness, command: string): unknown[][] {
  return h.gtagCalls().filter((args) => args[0] === command);
}

function storedChoice(h: Harness): string | null {
  return h.win.localStorage.getItem(ANALYTICS_CONSENT_STORAGE_KEY);
}

afterEach(() => {
  harness?.win.close();
  harness = null;
});

beforeEach(() => {
  harness = null;
});

describe('analytics runtime — unconfigured builds', () => {
  test('an empty measurement id ships no consent prompt and no Google contact', () => {
    const h = mount({ measurementId: '' });
    expect(h.host.hasAttribute('hidden')).toBe(true);
    expect(h.gtagScripts().length).toBe(0);
    expect(h.networkRequests).toEqual([]);
    expect((h.win as unknown as { dataLayer?: unknown[] }).dataLayer).toBeUndefined();
    expect(h.storageWrites).toEqual([]);
  });

  test('a malformed measurement id is treated as unconfigured', () => {
    const h = mount({ measurementId: 'UA-1234-5', stored: 'granted' });
    expect(h.host.hasAttribute('hidden')).toBe(true);
    expect(h.gtagScripts().length).toBe(0);
    expect(h.networkRequests).toEqual([]);
  });
});

describe('analytics runtime — consent gate', () => {
  test('nothing reaches Google before a choice is made', () => {
    const h = mount();
    expect(h.host.hasAttribute('hidden')).toBe(false);
    expect(h.panel.hasAttribute('hidden')).toBe(false);
    expect(h.reopen.hasAttribute('hidden')).toBe(true);
    expect(h.gtagScripts().length).toBe(0);
    expect(h.networkRequests).toEqual([]);
    expect((h.win as unknown as { dataLayer?: unknown[] }).dataLayer).toBeUndefined();
    expect(h.storageWrites).toEqual([]);
    expect(storedChoice(h)).toBeNull();
  });

  test('accept and decline are the same control type with the same emphasis', () => {
    const h = mount();
    expect(h.accept.tagName).toBe('BUTTON');
    expect(h.decline.tagName).toBe('BUTTON');
    expect(h.accept.getAttribute('type')).toBe('button');
    expect(h.decline.getAttribute('type')).toBe('button');
    expect(h.accept.className).toBe(h.decline.className);
    expect(h.accept.textContent?.trim().length).toBeGreaterThan(0);
    expect(h.decline.textContent?.trim().length).toBeGreaterThan(0);
    expect(h.panel.getAttribute('role')).toBe('dialog');
    const labelledBy = h.panel.getAttribute('aria-labelledby');
    const describedBy = h.panel.getAttribute('aria-describedby');
    expect(h.doc.getElementById(labelledBy ?? '')).not.toBeNull();
    expect(h.doc.getElementById(describedBy ?? '')).not.toBeNull();
    expect(h.reopen.tagName).toBe('BUTTON');
    expect(h.reopen.textContent?.trim().length).toBeGreaterThan(0);
  });

  test('declining stores the refusal and still loads no tag', () => {
    const h = mount();
    click(h.decline);
    expect(storedChoice(h)).toBe('denied');
    expect(h.gtagScripts().length).toBe(0);
    expect(h.networkRequests).toEqual([]);
    expect((h.win as unknown as { dataLayer?: unknown[] }).dataLayer).toBeUndefined();
    expect(h.panel.hasAttribute('hidden')).toBe(true);
    expect(h.reopen.hasAttribute('hidden')).toBe(false);
  });

  test('only the open choice overlays the page', () => {
    const h = mount();
    expect(h.host.getAttribute('data-consent-state')).toBe('open');
    click(h.decline);
    expect(h.host.getAttribute('data-consent-state')).toBe('closed');
    click(h.reopen);
    expect(h.host.getAttribute('data-consent-state')).toBe('open');
  });

  test('a stored refusal never prompts or loads on later visits', () => {
    const h = mount({ stored: 'denied' });
    expect(h.panel.hasAttribute('hidden')).toBe(true);
    expect(h.reopen.hasAttribute('hidden')).toBe(false);
    expect(h.gtagScripts().length).toBe(0);
    expect(h.networkRequests).toEqual([]);
  });

  test('accepting loads exactly one async gtag tag for the configured id', () => {
    const h = mount();
    click(h.accept);
    expect(storedChoice(h)).toBe('granted');
    const scripts = h.gtagScripts();
    expect(scripts.length).toBe(1);
    expect(scripts[0]!.getAttribute('src')).toBe(
      `https://www.googletagmanager.com/gtag/js?id=${MEASUREMENT_ID}`,
    );
    expect(scripts[0]!.async).toBe(true);
    expect(h.networkRequests).toEqual([`https://www.googletagmanager.com/gtag/js?id=${MEASUREMENT_ID}`]);
    expect(h.panel.hasAttribute('hidden')).toBe(true);
    expect(h.reopen.hasAttribute('hidden')).toBe(false);
  });

  test('a stored acceptance loads the tag without prompting again', () => {
    const h = mount({ stored: 'granted' });
    expect(h.panel.hasAttribute('hidden')).toBe(true);
    expect(h.reopen.hasAttribute('hidden')).toBe(false);
    expect(h.gtagScripts().length).toBe(1);
  });

  test('the reopen control brings the choice back and moves focus into it', () => {
    const h = mount({ stored: 'denied' });
    click(h.reopen);
    expect(h.panel.hasAttribute('hidden')).toBe(false);
    expect(h.doc.activeElement).toBe(h.accept);
    click(h.accept);
    expect(storedChoice(h)).toBe('granted');
    expect(h.gtagScripts().length).toBe(1);
  });

  test('withdrawing consent disables the loaded tag and records the denial', () => {
    const h = mount({ stored: 'granted' });
    click(h.reopen);
    click(h.decline);
    expect(storedChoice(h)).toBe('denied');
    const updates = calls(h, 'consent').filter((args) => args[1] === 'update');
    expect(updates.length).toBeGreaterThan(0);
    expect(updates.at(-1)![2]).toMatchObject({ analytics_storage: 'denied' });
    expect((h.win as unknown as Record<string, unknown>)[`ga-disable-${MEASUREMENT_ID}`]).toBe(true);
  });
});

describe('analytics runtime — consent mounting', () => {
  function withTemplate(measurementId: string) {
    // A <template> is the only element allowed to carry this markup inside <head>, which is
    // where the Astro component renders; the host is created in <body> at runtime.
    const dom = new JSDOM(
      `<!doctype html><html><head><template id="${CONSENT_TEMPLATE_ID}" data-measurement-id="${measurementId}">` +
        consentMarkup({ privacyHref: '/privacy/' }) +
        `</template></head><body><main></main></body></html>`,
      { url: PAGE_URL, pretendToBeVisual: true },
    );
    return dom.window;
  }

  test('the host lands in the body carrying the build-time measurement id', () => {
    const win = withTemplate(MEASUREMENT_ID);
    try {
      const host = mountConsentUi(win);
      expect(host).not.toBeNull();
      expect(host!.parentElement).toBe(win.document.body);
      expect(win.document.head.contains(host!)).toBe(false);
      expect(host!.id).toBe(CONSENT_HOST_ID);
      expect(host!.getAttribute('data-measurement-id')).toBe(MEASUREMENT_ID);
      expect(host!.querySelector('[data-consent-action="granted"]')).not.toBeNull();
      expect(host!.querySelector('[data-consent-action="denied"]')).not.toBeNull();
      expect(host!.querySelector('[data-consent-action="reopen"]')).not.toBeNull();
    } finally {
      win.close();
    }
  });

  test('mounting twice keeps a single host', () => {
    const win = withTemplate(MEASUREMENT_ID);
    try {
      mountConsentUi(win);
      mountConsentUi(win);
      expect(win.document.querySelectorAll(`#${CONSENT_HOST_ID}`).length).toBe(1);
    } finally {
      win.close();
    }
  });

  test('a build without the template mounts nothing and starts nothing', () => {
    const dom = new JSDOM('<!doctype html><html><head></head><body><main></main></body></html>', {
      url: PAGE_URL,
      pretendToBeVisual: true,
    });
    try {
      expect(mountConsentUi(dom.window)).toBeNull();
      initAnalytics(dom.window);
      expect(dom.window.document.getElementById(CONSENT_HOST_ID)).toBeNull();
      expect((dom.window as unknown as { dataLayer?: unknown[] }).dataLayer).toBeUndefined();
    } finally {
      dom.window.close();
    }
  });

  test('init mounts and runs the gate in one call', () => {
    const win = withTemplate(MEASUREMENT_ID);
    try {
      win.localStorage.setItem(ANALYTICS_CONSENT_STORAGE_KEY, 'granted');
      initAnalytics(win);
      const scripts = [...win.document.querySelectorAll('script')].filter((script) =>
        (script.getAttribute('src') ?? '').includes('googletagmanager.com'),
      );
      expect(scripts.length).toBe(1);
      expect(win.document.getElementById(CONSENT_HOST_ID)?.hasAttribute('hidden')).toBe(false);
    } finally {
      win.close();
    }
  });
});

describe('analytics runtime — measurement contract', () => {
  test('consent defaults deny everything and only analytics storage is granted', () => {
    const h = mount();
    click(h.accept);
    const consentCalls = calls(h, 'consent');
    const defaults = consentCalls.find((args) => args[1] === 'default');
    expect(defaults).toBeDefined();
    expect(defaults![2]).toMatchObject({
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
      analytics_storage: 'denied',
    });
    const update = consentCalls.filter((args) => args[1] === 'update').at(-1)!;
    expect(update[2]).toMatchObject({
      analytics_storage: 'granted',
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
    });
    expect(consentCalls.indexOf(defaults!)).toBeLessThan(consentCalls.indexOf(update));
  });

  test('config disables advertising signals, the automatic page view, and pins the sanitized location', () => {
    const h = mount();
    click(h.accept);
    const config = calls(h, 'config').find((args) => args[1] === MEASUREMENT_ID);
    expect(config).toBeDefined();
    expect(config![2]).toMatchObject({
      send_page_view: false,
      allow_google_signals: false,
      allow_ad_personalization_signals: false,
      // Without this, gtag.js recomputes `dl` from location.href on every later hit.
      page_location: 'https://ferryx.dev/docs/introduction/?utm_source=news',
      page_path: '/docs/introduction/',
    });
    expect(String((config![2] as Record<string, unknown>).page_location)).not.toContain('email');
  });

  test('the page view reports an allowlisted location with no query leakage', () => {
    const h = mount();
    click(h.accept);
    const pageViews = calls(h, 'event').filter((args) => args[1] === 'page_view');
    expect(pageViews.length).toBe(1);
    const params = pageViews[0]![2] as Record<string, unknown>;
    expect(params.page_location).toBe('https://ferryx.dev/docs/introduction/?utm_source=news');
    expect(String(params.page_location)).not.toContain('email');
    expect(params.page_path).toBe('/docs/introduction/');
  });

  test('exactly one page view is sent per page load', () => {
    const h = mount({ stored: 'granted' });
    expect(calls(h, 'event').filter((args) => args[1] === 'page_view').length).toBe(1);
  });
});

describe('analytics runtime — download tracking', () => {
  function anchor(h: Harness, href: string, location?: string): HTMLAnchorElement {
    const container = h.doc.createElement('div');
    const link = h.doc.createElement('a');
    link.setAttribute('href', href);
    if (location) link.setAttribute('data-ferryx-location', location);
    link.textContent = 'Download';
    container.appendChild(link);
    (h.doc.querySelector('main') as HTMLElement).appendChild(container);
    return link;
  }

  function downloadEvents(h: Harness): Array<Record<string, unknown>> {
    return calls(h, 'event')
      .filter((args) => args[1] === 'download_click')
      .map((args) => args[2] as Record<string, unknown>);
  }

  test('a release asset click reports platform, asset and location', () => {
    const h = mount({ stored: 'granted' });
    click(anchor(h, PLATFORMS.macos.defaultAsset.url, 'hero_primary'));
    expect(downloadEvents(h)).toEqual([
      {
        platform: 'macos',
        asset_id: 'macos-universal',
        destination: 'github_release_asset',
        link_location: 'hero_primary',
      },
    ]);
  });

  test('a Microsoft Store click is counted as a windows install intent', () => {
    const h = mount({ stored: 'granted' });
    click(anchor(h, MICROSOFT_STORE_URL, 'docs_markdown'));
    expect(downloadEvents(h)).toEqual([
      {
        platform: 'windows',
        asset_id: 'windows-store',
        destination: 'microsoft_store',
        link_location: 'docs_markdown',
      },
    ]);
  });

  test('links inside markdown prose fall back to a content location', () => {
    const h = mount({ stored: 'granted' });
    click(anchor(h, PLATFORMS.linux.defaultAsset.url));
    expect(downloadEvents(h)[0]).toMatchObject({ platform: 'linux', link_location: 'content_link' });
  });

  test('an enclosing location attribute applies to nested links', () => {
    const h = mount({ stored: 'granted' });
    const link = anchor(h, PLATFORMS.linux.assets[1]!.url);
    (link.parentElement as HTMLElement).setAttribute('data-ferryx-location', 'download_menu');
    click(link);
    expect(downloadEvents(h)[0]).toMatchObject({ asset_id: 'linux-deb', link_location: 'download_menu' });
  });

  test('browsing the release listing is not a download', () => {
    const h = mount({ stored: 'granted' });
    click(anchor(h, 'https://github.com/Indosaram/ferryx/releases/latest', 'download_menu'));
    click(anchor(h, 'https://github.com/Indosaram/ferryx', 'navbar'));
    click(anchor(h, '/docs/introduction/', 'navbar'));
    expect(downloadEvents(h)).toEqual([]);
  });

  test('a click is captured even when a hydrated menu unmounts on the same click', () => {
    const h = mount({ stored: 'granted' });
    const link = anchor(h, PLATFORMS.macos.defaultAsset.url, 'download_menu');
    const container = link.parentElement as HTMLElement;
    // Mirrors React closing the menu: the subtree disappears while the click is still propagating.
    link.addEventListener('click', (event) => {
      event.stopPropagation();
      container.remove();
    });
    click(link);
    expect(h.doc.contains(link)).toBe(false);
    expect(downloadEvents(h)).toEqual([
      {
        platform: 'macos',
        asset_id: 'macos-universal',
        destination: 'github_release_asset',
        link_location: 'download_menu',
      },
    ]);
  });

  test('downloads before consent are not recorded anywhere', () => {
    const h = mount();
    click(anchor(h, PLATFORMS.macos.defaultAsset.url, 'hero_primary'));
    expect((h.win as unknown as { dataLayer?: unknown[] }).dataLayer).toBeUndefined();
    expect(h.networkRequests).toEqual([]);
    click(h.accept);
    expect(downloadEvents(h)).toEqual([]);
  });

  test('downloads after a withdrawal are not recorded', () => {
    const h = mount({ stored: 'granted' });
    click(h.reopen);
    click(h.decline);
    click(anchor(h, PLATFORMS.macos.defaultAsset.url, 'hero_primary'));
    expect(downloadEvents(h)).toEqual([]);
  });

  test('outbound download links keep their href untouched', () => {
    const h = mount({ stored: 'granted' });
    const link = anchor(h, PLATFORMS.macos.defaultAsset.url, 'hero_primary');
    click(link);
    expect(link.getAttribute('href')).toBe(PLATFORMS.macos.defaultAsset.url);
    expect(link.getAttribute('href')).not.toContain('utm_');
  });
});
