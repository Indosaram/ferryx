import {
  ANALYTICS_CONSENT_STORAGE_KEY,
  DEFAULT_LINK_LOCATION,
  DOWNLOAD_CLICK_EVENT,
  LINK_LOCATION_ATTRIBUTE,
  classifyDownloadTarget,
  downloadEventParams,
  isConsentChoice,
  resolveMeasurementId,
  sanitizePageLocation,
  type ConsentChoice,
} from './analytics';

/** Id of the element that carries the build-time measurement id and the consent UI. */
export const CONSENT_HOST_ID = 'ferryx-analytics-consent';

/** Id of the <template> the Astro component renders inside <head>. */
export const CONSENT_TEMPLATE_ID = 'ferryx-analytics-consent-template';

const TITLE_ID = 'ferryx-analytics-consent-title';
const DESCRIPTION_ID = 'ferryx-analytics-consent-description';
const BUTTON_CLASS = 'ferryx-consent__button';

export interface ConsentMarkupOptions {
  readonly privacyHref: string;
}

/**
 * Static markup for the consent choice. It ships in the HTML so the control exists without
 * JavaScript having to build a dialog, and the same string backs the DOM contract tests.
 */
export function consentMarkup({ privacyHref }: ConsentMarkupOptions): string {
  return `<div class="ferryx-consent__panel" role="dialog" aria-labelledby="${TITLE_ID}" aria-describedby="${DESCRIPTION_ID}" data-consent-panel hidden>
  <h2 class="ferryx-consent__title" id="${TITLE_ID}">Website analytics</h2>
  <p class="ferryx-consent__text" id="${DESCRIPTION_ID}">Ferryx can measure which pages and downloads this site sends people to, using Google Analytics. Nothing is loaded or stored until you choose. The Ferryx desktop app sends no telemetry either way, and the site works the same whichever you pick. <a class="ferryx-consent__link" href="${privacyHref}">Privacy declaration</a></p>
  <div class="ferryx-consent__actions">
    <button class="${BUTTON_CLASS}" type="button" data-consent-action="granted">Allow analytics</button>
    <button class="${BUTTON_CLASS}" type="button" data-consent-action="denied">Decline analytics</button>
  </div>
</div>
<button class="ferryx-consent__reopen" type="button" data-consent-action="reopen" hidden>Analytics choice</button>`;
}

/**
 * Moves the consent markup out of the head-rendered <template> into the document body.
 * Returns null for builds that shipped no template (no measurement id configured).
 */
export function mountConsentUi(win: Window = window): HTMLElement | null {
  const doc = win.document;
  const existing = doc.getElementById(CONSENT_HOST_ID);
  if (existing) return existing;
  const template = doc.getElementById(CONSENT_TEMPLATE_ID) as HTMLTemplateElement | null;
  if (!template?.content) return null;

  const host = doc.createElement('div');
  host.id = CONSENT_HOST_ID;
  host.className = 'ferryx-consent';
  host.setAttribute('data-measurement-id', template.getAttribute('data-measurement-id') ?? '');
  host.setAttribute('hidden', '');
  host.appendChild(template.content.cloneNode(true));
  doc.body.appendChild(host);
  return host;
}

/** Mounts the consent control and runs the gate. Entry point for the inline page script. */
export function initAnalytics(win: Window = window): void {
  mountConsentUi(win);
  startAnalytics(win);
}

type GtagWindow = Window &
  typeof globalThis & {
    dataLayer?: IArguments[];
    gtag?: (...args: unknown[]) => void;
  };

function readStoredChoice(win: Window): ConsentChoice | null {
  try {
    const raw = win.localStorage.getItem(ANALYTICS_CONSENT_STORAGE_KEY);
    return isConsentChoice(raw) ? raw : null;
  } catch {
    return null;
  }
}

function persistChoice(win: Window, choice: ConsentChoice): void {
  try {
    win.localStorage.setItem(ANALYTICS_CONSENT_STORAGE_KEY, choice);
  } catch {
    // Storage denial (private mode, blocked cookies) must not break the choice for this visit.
  }
}

/**
 * Wires the consent control and, only after an explicit grant, the Google tag.
 *
 * Nothing here contacts Google, defines `dataLayer`, or writes storage before the visitor
 * accepts; an unset or malformed `PUBLIC_GA_MEASUREMENT_ID` disables the whole surface.
 */
export function startAnalytics(win: Window = window): void {
  const doc = win.document;
  const host = doc.getElementById(CONSENT_HOST_ID);
  if (!host) return;

  const measurementId = resolveMeasurementId(host.getAttribute('data-measurement-id'));
  if (!measurementId) {
    host.setAttribute('hidden', '');
    return;
  }

  const panel = host.querySelector<HTMLElement>('[data-consent-panel]');
  const accept = host.querySelector<HTMLButtonElement>('[data-consent-action="granted"]');
  const decline = host.querySelector<HTMLButtonElement>('[data-consent-action="denied"]');
  const reopen = host.querySelector<HTMLButtonElement>('[data-consent-action="reopen"]');
  if (!panel || !accept || !decline || !reopen) return;

  let choice = readStoredChoice(win);
  let panelOpen = choice === null;
  let tagLoaded = false;
  let pageViewSent = false;

  const render = () => {
    host.removeAttribute('hidden');
    // Only the open choice overlays the page; the reopen control afterwards sits in document
    // flow after the footer so it can never cover a call to action on a small screen.
    host.setAttribute('data-consent-state', panelOpen ? 'open' : 'closed');
    panel.toggleAttribute('hidden', !panelOpen);
    reopen.toggleAttribute('hidden', panelOpen);
  };

  // The official snippet pushes the `arguments` object itself and gtag.js relies on that
  // shape, so this stays a classic function rather than a rest-parameter arrow.
  const gtag = function gtag(): void {
    const target = win as GtagWindow;
    target.dataLayer = target.dataLayer || [];
    target.dataLayer.push(arguments);
  } as (...args: unknown[]) => void;

  const grant = () => {
    if (tagLoaded) {
      (win as unknown as Record<string, unknown>)[`ga-disable-${measurementId}`] = false;
      gtag('consent', 'update', {
        ad_storage: 'denied',
        ad_user_data: 'denied',
        ad_personalization: 'denied',
        analytics_storage: 'granted',
      });
      return;
    }
    tagLoaded = true;
    gtag('js', new Date());
    gtag('consent', 'default', {
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
      analytics_storage: 'denied',
    });

    const pageLocation = sanitizePageLocation(win.location.href);

    const script = doc.createElement('script');
    script.async = true;
    script.src = `https://www.googletagmanager.com/gtag/js?id=${measurementId}`;
    doc.head.appendChild(script);

    // page_location is pinned on the config, not just on the page_view: gtag.js otherwise
    // recomputes `dl` from location.href for every later hit and would ship the raw query.
    gtag('config', measurementId, {
      send_page_view: false,
      allow_google_signals: false,
      allow_ad_personalization_signals: false,
      page_location: pageLocation,
      page_path: win.location.pathname,
    });
    gtag('consent', 'update', {
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
      analytics_storage: 'granted',
    });

    if (!pageViewSent) {
      pageViewSent = true;
      gtag('event', 'page_view', {
        page_location: pageLocation,
        page_path: win.location.pathname,
        page_title: doc.title,
      });
    }
  };

  const withdraw = () => {
    if (!tagLoaded) return;
    (win as unknown as Record<string, unknown>)[`ga-disable-${measurementId}`] = true;
    gtag('consent', 'update', {
      ad_storage: 'denied',
      ad_user_data: 'denied',
      ad_personalization: 'denied',
      analytics_storage: 'denied',
    });
  };

  const decide = (next: ConsentChoice) => {
    choice = next;
    persistChoice(win, next);
    panelOpen = false;
    render();
    if (next === 'granted') grant();
    else withdraw();
  };

  host.addEventListener('click', (event) => {
    const action = (event.target as Element | null)?.closest?.('[data-consent-action]');
    const value = action?.getAttribute('data-consent-action');
    if (value === 'granted' || value === 'denied') {
      decide(value);
      return;
    }
    if (value === 'reopen') {
      panelOpen = true;
      render();
      accept.focus();
    }
  });

  panel.addEventListener('keydown', (event) => {
    // Dismissing without choosing leaves the visitor untracked; the reopen control stays.
    if ((event as KeyboardEvent).key !== 'Escape') return;
    panelOpen = false;
    render();
    reopen.focus();
  });

  // Capture phase on the document: a hydrated menu that unmounts itself (or stops propagation)
  // on the same click would otherwise destroy the link before a bubbling listener ever saw it.
  doc.addEventListener(
    'click',
    (event) => {
      if (choice !== 'granted' || !tagLoaded) return;
      const anchor = (event.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null;
      if (!anchor) return;
      const target = classifyDownloadTarget(anchor.href);
      if (!target) return;
      const located = anchor.closest(`[${LINK_LOCATION_ATTRIBUTE}]`);
      const location = located?.getAttribute(LINK_LOCATION_ATTRIBUTE)?.trim() || DEFAULT_LINK_LOCATION;
      gtag('event', DOWNLOAD_CLICK_EVENT, downloadEventParams(target, location));
    },
    true,
  );

  render();
  if (choice === 'granted') grant();
}
