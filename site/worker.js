import { CONSENT_REGION_ATTRIBUTE, consentRegionFor } from './src/lib/consentRegion.ts';

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.protocol !== 'https:' || url.hostname === 'www.ferryx.dev') {
      url.protocol = 'https:';
      url.hostname = 'ferryx.dev';
      return Response.redirect(url.toString(), 301);
    }
    const response = await env.ASSETS.fetch(request);
    if (!(response.headers.get('content-type') ?? '').includes('text/html')) return response;

    const region = consentRegionFor(request.cf?.country);
    const tagged = new HTMLRewriter()
      .on('html', {
        element(element) {
          element.setAttribute(CONSENT_REGION_ATTRIBUTE, region);
        },
      })
      .transform(response);
    const headers = new Headers(tagged.headers);
    headers.set('Cache-Control', 'private, no-cache');
    return new Response(tagged.body, { status: tagged.status, statusText: tagged.statusText, headers });
  },
};
